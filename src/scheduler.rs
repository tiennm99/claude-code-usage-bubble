//! The single timer thread. It owns the poll, countdown and update-check
//! deadlines and is driven by messages over an `mpsc` channel.
//!
//! The deadline rules live in [`Deadlines`], which takes the current time as
//! an argument and so is tested without a thread, a window or a clock. The
//! thread only waits for the nearest deadline or the next message and hands
//! each due [`Action`] to its [`SchedulerHost`]. A host must return quickly:
//! slow work (HTTP, the CLI refresh) runs on worker threads it spawns.
//!
//! `Instant` stops while the machine sleeps, so the update check, which is
//! due at a wall-clock time, is kept as a unix timestamp, and the thread
//! wakes at least every [`MAX_WAIT`] to notice a resume (see
//! [`resumed_from_sleep`]).

use std::io;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::core::state::{update_check_due, UpdateCheckDue};

/// Shortest poll interval, guarding against a hand-edited `0` in the
/// settings file turning into a busy loop.
const MIN_POLL_INTERVAL: Duration = Duration::from_secs(10);

/// Shortest automatic update-check interval, for the same reason.
const MIN_UPDATE_INTERVAL: Duration = Duration::from_secs(60);

/// First retry delay after a failed update check; it doubles per failure
/// in a row and never exceeds the check interval.
const UPDATE_RETRY_BASE: Duration = Duration::from_secs(60);

/// Longest the thread sleeps between passes, so a resume from sleep or a
/// wall-clock deadline is noticed within about this long.
const MAX_WAIT: Duration = Duration::from_secs(60);

/// How far wall time may run ahead of `Instant` between two passes before
/// the gap counts as a sleep.
const RESUME_THRESHOLD: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    /// Poll at once. The periodic poll deadline is left as it is.
    PollNow,
    /// Change the poll interval and re-arm it from now, without polling.
    SetPollInterval(Duration),
    /// Re-render the countdowns after this delay; `None` clears the deadline.
    RecomputeCountdown(Option<Duration>),
    /// The auto-check interval or its last check changed: check now if one
    /// is due, otherwise wait for the remainder. `interval_secs: None`
    /// cancels the deadline.
    SetUpdateInterval {
        interval_secs: Option<u64>,
        last_check_unix: Option<u64>,
    },
    /// An update check finished. Only this re-arms the check: after the
    /// full interval when it succeeded, after a growing retry delay capped
    /// at the interval when it failed.
    UpdateCheckDone {
        interval_secs: Option<u64>,
        succeeded: bool,
    },
    /// Stop the thread.
    Shutdown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Poll,
    Countdown,
    CheckForUpdates,
}

/// Receives the due actions on the scheduler thread.
pub trait SchedulerHost {
    fn run(&self, action: Action);
}

/// Settings the deadlines start from.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub poll_interval: Duration,
    pub update_interval_secs: Option<u64>,
    pub last_update_check_unix: Option<u64>,
}

#[derive(Debug)]
pub struct Deadlines {
    poll_interval: Duration,
    poll_at: Instant,
    countdown_at: Option<Instant>,
    /// Unix seconds the next update check is due at.
    update_at_unix: Option<u64>,
    /// Failed update checks in a row, which set the retry delay.
    update_failures: u32,
}

impl Deadlines {
    /// Arm the deadlines at startup. Returns the actions due at once: the
    /// update check when the last one is older than its interval.
    pub fn new(config: Config, now: Instant, now_unix: u64) -> (Self, Vec<Action>) {
        let poll_interval = config.poll_interval.max(MIN_POLL_INTERVAL);
        let mut deadlines = Self {
            poll_interval,
            poll_at: now + poll_interval,
            countdown_at: None,
            update_at_unix: None,
            update_failures: 0,
        };
        let actions = deadlines.arm_update(
            config.update_interval_secs,
            config.last_update_check_unix,
            now_unix,
        );
        (deadlines, actions)
    }

    /// Apply one message. Returns the actions to run at once.
    pub fn handle(&mut self, message: Message, now: Instant, now_unix: u64) -> Vec<Action> {
        match message {
            Message::PollNow => vec![Action::Poll],
            Message::SetPollInterval(interval) => {
                self.poll_interval = interval.max(MIN_POLL_INTERVAL);
                self.poll_at = now + self.poll_interval;
                Vec::new()
            }
            Message::RecomputeCountdown(delay) => {
                self.countdown_at = delay.map(|d| now + d);
                Vec::new()
            }
            Message::SetUpdateInterval {
                interval_secs,
                last_check_unix,
            } => self.arm_update(interval_secs, last_check_unix, now_unix),
            Message::UpdateCheckDone {
                interval_secs,
                succeeded,
            } => {
                let delay = if succeeded {
                    self.update_failures = 0;
                    interval_secs.map(update_interval)
                } else {
                    let delay = interval_secs.map(|secs| self.retry_delay(secs));
                    self.update_failures = self.update_failures.saturating_add(1);
                    delay
                };
                self.update_at_unix = delay.map(|d| now_unix.saturating_add(d.as_secs()));
                Vec::new()
            }
            Message::Shutdown => Vec::new(),
        }
    }

    /// Fire every deadline that has passed. The poll re-arms for a full
    /// interval; the countdown and update check stay disarmed until the
    /// host re-arms them (after the re-render, after the check finished).
    pub fn take_due(&mut self, now: Instant, now_unix: u64) -> Vec<Action> {
        let mut due = Vec::new();
        if self.poll_at <= now {
            due.push(Action::Poll);
            self.poll_at = now + self.poll_interval;
        }
        if self.countdown_at.is_some_and(|at| at <= now) {
            due.push(Action::Countdown);
            self.countdown_at = None;
        }
        if self.update_at_unix.is_some_and(|at| at <= now_unix) {
            due.push(Action::CheckForUpdates);
            self.update_at_unix = None;
        }
        due
    }

    /// The machine woke from sleep: poll at once and re-arm the poll for a
    /// full interval from now. The update check needs nothing here, since
    /// [`Self::take_due`] compares its wall-clock deadline with `now_unix`.
    pub fn resume(&mut self, now: Instant) -> Vec<Action> {
        self.poll_at = now + self.poll_interval;
        vec![Action::Poll]
    }

    /// Time until the nearest deadline. The poll deadline is always armed,
    /// so there always is one.
    pub fn next_wakeup(&self, now: Instant, now_unix: u64) -> Duration {
        let update = self
            .update_at_unix
            .map(|at| Duration::from_secs(at.saturating_sub(now_unix)));
        [
            Some(self.poll_at.saturating_duration_since(now)),
            self.countdown_at
                .map(|at| at.saturating_duration_since(now)),
            update,
        ]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(self.poll_interval)
    }

    fn arm_update(
        &mut self,
        interval_secs: Option<u64>,
        last_check_unix: Option<u64>,
        now_unix: u64,
    ) -> Vec<Action> {
        let interval_secs = interval_secs.map(|secs| secs.max(MIN_UPDATE_INTERVAL.as_secs()));
        match update_check_due(last_check_unix, interval_secs, now_unix) {
            None => {
                self.update_at_unix = None;
                Vec::new()
            }
            Some(UpdateCheckDue::Now) => {
                self.update_at_unix = None;
                vec![Action::CheckForUpdates]
            }
            Some(UpdateCheckDue::After(remaining)) => {
                self.update_at_unix = Some(now_unix.saturating_add(remaining.as_secs()));
                Vec::new()
            }
        }
    }

    /// Delay before the next try after the current run of failures:
    /// `UPDATE_RETRY_BASE * 2^failures`, capped at the check interval.
    fn retry_delay(&self, interval_secs: u64) -> Duration {
        let factor = 1u32.checked_shl(self.update_failures).unwrap_or(u32::MAX);
        UPDATE_RETRY_BASE
            .saturating_mul(factor)
            .min(update_interval(interval_secs))
    }
}

fn update_interval(secs: u64) -> Duration {
    Duration::from_secs(secs).max(MIN_UPDATE_INTERVAL)
}

/// Whether the wall clock ran ahead of `Instant` by more than
/// [`RESUME_THRESHOLD`] between two passes, which means the machine slept
/// (or the clock was set forward). `wall_elapsed` is `None` when the wall
/// clock went backwards.
pub fn resumed_from_sleep(instant_elapsed: Duration, wall_elapsed: Option<Duration>) -> bool {
    wall_elapsed.is_some_and(|wall| wall.saturating_sub(instant_elapsed) > RESUME_THRESHOLD)
}

/// Seconds since the Unix epoch; 0 for a clock set before 1970.
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Start the scheduler thread reading `rx`. Create the channel first, so
/// the sender exists before the host runs its first action.
/// [`Message::Shutdown`] or dropping every sender stops the thread.
pub fn spawn<H>(config: Config, rx: Receiver<Message>, host: H) -> io::Result<JoinHandle<()>>
where
    H: SchedulerHost + Send + 'static,
{
    std::thread::Builder::new()
        .name("scheduler".into())
        .spawn(move || {
            let (deadlines, initial) = Deadlines::new(config, Instant::now(), unix_now());
            for action in initial {
                host.run(action);
            }
            run_loop(deadlines, &rx, &host);
            log::info!("scheduler stopped");
        })
}

fn run_loop<H: SchedulerHost>(mut deadlines: Deadlines, rx: &Receiver<Message>, host: &H) {
    let mut last_instant = Instant::now();
    let mut last_wall = SystemTime::now();
    loop {
        let (now, wall) = (Instant::now(), SystemTime::now());
        let instant_elapsed = now.saturating_duration_since(last_instant);
        let wall_elapsed = wall.duration_since(last_wall).ok();
        (last_instant, last_wall) = (now, wall);
        if resumed_from_sleep(instant_elapsed, wall_elapsed) {
            log::info!(
                "wall clock moved {wall_elapsed:?} while Instant moved {instant_elapsed:?}; \
                 treating it as a resume from sleep"
            );
            for action in deadlines.resume(now) {
                host.run(action);
            }
        }
        for action in deadlines.take_due(now, unix_now()) {
            host.run(action);
        }
        let wait = deadlines
            .next_wakeup(Instant::now(), unix_now())
            .min(MAX_WAIT);
        let message = match rx.recv_timeout(wait) {
            Ok(Message::Shutdown) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(message) => message,
            Err(RecvTimeoutError::Timeout) => continue,
        };
        for action in deadlines.handle(message, Instant::now(), unix_now()) {
            host.run(action);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{self, Sender};

    use crate::core::settings::{
        UPDATE_CHECK_DAILY_SECS, UPDATE_CHECK_HOURLY_SECS, UPDATE_CHECK_WEEKLY_SECS,
    };

    const MIN: Duration = Duration::from_secs(60);
    const NOW_UNIX: u64 = 1_800_000_000;
    const DAY: u64 = 24 * 60 * 60;

    fn config(update: Option<u64>, last: Option<u64>) -> Config {
        Config {
            poll_interval: 5 * MIN,
            update_interval_secs: update,
            last_update_check_unix: last,
        }
    }

    #[test]
    fn poll_fires_every_interval() {
        let t0 = Instant::now();
        let (mut d, initial) = Deadlines::new(config(None, None), t0, NOW_UNIX);
        assert!(initial.is_empty());
        assert_eq!(d.next_wakeup(t0, NOW_UNIX), 5 * MIN);
        assert!(d.take_due(t0 + 4 * MIN, NOW_UNIX).is_empty());
        assert_eq!(d.take_due(t0 + 5 * MIN, NOW_UNIX), [Action::Poll]);
        assert_eq!(d.next_wakeup(t0 + 5 * MIN, NOW_UNIX), 5 * MIN);
        assert_eq!(d.take_due(t0 + 10 * MIN, NOW_UNIX), [Action::Poll]);
    }

    #[test]
    fn poll_now_polls_without_moving_the_periodic_deadline() {
        let t0 = Instant::now();
        let (mut d, _) = Deadlines::new(config(None, None), t0, NOW_UNIX);
        assert_eq!(
            d.handle(Message::PollNow, t0 + 2 * MIN, NOW_UNIX),
            [Action::Poll]
        );
        assert_eq!(d.take_due(t0 + 5 * MIN, NOW_UNIX), [Action::Poll]);
    }

    #[test]
    fn new_poll_interval_rearms_from_now_without_polling() {
        let t0 = Instant::now();
        let (mut d, _) = Deadlines::new(config(None, None), t0, NOW_UNIX);
        let t1 = t0 + 4 * MIN;
        assert!(d
            .handle(Message::SetPollInterval(MIN * 15), t1, NOW_UNIX)
            .is_empty());
        assert!(d.take_due(t0 + 5 * MIN, NOW_UNIX).is_empty());
        assert_eq!(d.next_wakeup(t1, NOW_UNIX), 15 * MIN);
        assert_eq!(d.take_due(t1 + 15 * MIN, NOW_UNIX), [Action::Poll]);
    }

    #[test]
    fn a_zero_poll_interval_is_floored() {
        let t0 = Instant::now();
        let mut cfg = config(None, None);
        cfg.poll_interval = Duration::ZERO;
        let (d, _) = Deadlines::new(cfg, t0, NOW_UNIX);
        assert_eq!(d.next_wakeup(t0, NOW_UNIX), MIN_POLL_INTERVAL);
    }

    #[test]
    fn countdown_fires_once_until_rearmed() {
        let t0 = Instant::now();
        let (mut d, _) = Deadlines::new(config(None, None), t0, NOW_UNIX);
        let delay = Duration::from_secs(31);
        d.handle(Message::RecomputeCountdown(Some(delay)), t0, NOW_UNIX);
        assert_eq!(d.next_wakeup(t0, NOW_UNIX), delay);
        assert_eq!(d.take_due(t0 + delay, NOW_UNIX), [Action::Countdown]);
        assert!(d.take_due(t0 + 2 * delay, NOW_UNIX).is_empty());

        d.handle(Message::RecomputeCountdown(Some(delay)), t0, NOW_UNIX);
        d.handle(Message::RecomputeCountdown(None), t0, NOW_UNIX);
        assert!(d.take_due(t0 + 2 * delay, NOW_UNIX).is_empty());
    }

    #[test]
    fn startup_checks_for_updates_when_due() {
        let t0 = Instant::now();
        let hourly = Some(UPDATE_CHECK_HOURLY_SECS);
        let (_, never_checked) = Deadlines::new(config(hourly, None), t0, NOW_UNIX);
        assert_eq!(never_checked, [Action::CheckForUpdates]);
        let (_, stale) = Deadlines::new(config(hourly, Some(NOW_UNIX - 2 * 3_600)), t0, NOW_UNIX);
        assert_eq!(stale, [Action::CheckForUpdates]);
        let (_, disabled) = Deadlines::new(config(None, None), t0, NOW_UNIX);
        assert!(disabled.is_empty());
    }

    #[test]
    fn startup_waits_for_the_remainder_when_not_due() {
        let t0 = Instant::now();
        let (mut d, initial) = Deadlines::new(
            config(Some(UPDATE_CHECK_HOURLY_SECS), Some(NOW_UNIX - 600)),
            t0,
            NOW_UNIX,
        );
        assert!(initial.is_empty());
        let remaining = 3_000;
        assert!(!d
            .take_due(t0, NOW_UNIX + remaining - 60)
            .contains(&Action::CheckForUpdates));
        assert_eq!(d.next_wakeup(t0, NOW_UNIX + remaining - 60), MIN);
        assert_eq!(
            d.take_due(t0, NOW_UNIX + remaining),
            [Action::CheckForUpdates]
        );
    }

    /// Whether a check is due at `now_unix`, with `Instant` left at `t0`
    /// to show the update deadline follows the wall clock alone.
    fn check_due(d: &mut Deadlines, t0: Instant, now_unix: u64) -> bool {
        d.take_due(t0, now_unix).contains(&Action::CheckForUpdates)
    }

    fn done(interval_secs: Option<u64>, succeeded: bool) -> Message {
        Message::UpdateCheckDone {
            interval_secs,
            succeeded,
        }
    }

    #[test]
    fn only_a_finished_check_rearms_the_full_interval() {
        let t0 = Instant::now();
        let hour = UPDATE_CHECK_HOURLY_SECS;
        let (mut d, initial) = Deadlines::new(config(Some(hour), None), t0, NOW_UNIX);
        assert_eq!(initial, [Action::CheckForUpdates]);
        // While the check runs nothing else is scheduled for it.
        assert!(!check_due(&mut d, t0, NOW_UNIX + 3 * hour));

        let t1 = NOW_UNIX + 3 * hour;
        d.handle(done(Some(hour), true), t0, t1);
        assert!(!check_due(&mut d, t0, t1 + hour / 2));
        assert!(check_due(&mut d, t0, t1 + hour));
        // It repeats every interval, not just once.
        let t2 = t1 + hour;
        d.handle(done(Some(hour), true), t0, t2);
        assert!(check_due(&mut d, t0, t2 + hour));
    }

    #[test]
    fn a_failed_check_retries_with_a_doubling_delay_capped_at_the_interval() {
        let t0 = Instant::now();
        let hour = UPDATE_CHECK_HOURLY_SECS;
        let (mut d, _) = Deadlines::new(config(Some(hour), None), t0, NOW_UNIX);
        let mut now = NOW_UNIX;
        for expected in [60, 120, 240, 480, 960, 1_920, hour, hour] {
            d.handle(done(Some(hour), false), t0, now);
            assert!(!check_due(&mut d, t0, now + expected - 1));
            assert!(check_due(&mut d, t0, now + expected));
            now += expected;
        }
        // A success resets the delay: the next failure retries after a minute.
        d.handle(done(Some(hour), true), t0, now);
        assert!(check_due(&mut d, t0, now + hour));
        now += hour;
        d.handle(done(Some(hour), false), t0, now);
        assert!(check_due(&mut d, t0, now + 60));
    }

    #[test]
    fn a_failed_check_with_auto_check_off_stays_off() {
        let t0 = Instant::now();
        let (mut d, _) = Deadlines::new(config(None, None), t0, NOW_UNIX);
        d.handle(done(None, false), t0, NOW_UNIX);
        assert!(!check_due(&mut d, t0, NOW_UNIX + 7 * DAY));
    }

    #[test]
    fn the_update_deadline_follows_the_wall_clock_across_a_sleep() {
        // `Instant` does not advance while the machine sleeps, but the
        // check is still due once the wall clock passes its time.
        let t0 = Instant::now();
        let day = UPDATE_CHECK_DAILY_SECS;
        let (mut d, _) = Deadlines::new(config(Some(day), Some(NOW_UNIX)), t0, NOW_UNIX);
        assert!(!check_due(&mut d, t0, NOW_UNIX + day - 1));
        assert!(check_due(
            &mut d,
            t0 + Duration::from_secs(1),
            NOW_UNIX + day
        ));
    }

    #[test]
    fn resume_polls_at_once_and_rearms_the_poll() {
        let t0 = Instant::now();
        let (mut d, _) = Deadlines::new(config(None, None), t0, NOW_UNIX);
        let t1 = t0 + 2 * MIN;
        assert_eq!(d.resume(t1), [Action::Poll]);
        assert!(d.take_due(t0 + 5 * MIN, NOW_UNIX).is_empty());
        assert_eq!(d.next_wakeup(t1, NOW_UNIX), 5 * MIN);
        assert_eq!(d.take_due(t1 + 5 * MIN, NOW_UNIX), [Action::Poll]);
    }

    #[test]
    fn a_wall_clock_jump_past_instant_counts_as_a_resume() {
        let secs = Duration::from_secs;
        assert!(!resumed_from_sleep(MIN, Some(MIN)));
        // Ordinary scheduling slack is not a resume.
        assert!(!resumed_from_sleep(MIN, Some(MIN + secs(5))));
        assert!(!resumed_from_sleep(MIN, Some(MIN + RESUME_THRESHOLD)));
        assert!(resumed_from_sleep(
            MIN,
            Some(MIN + RESUME_THRESHOLD + secs(1))
        ));
        assert!(resumed_from_sleep(secs(1), Some(secs(8 * 3_600))));
        // A clock set back is not a resume.
        assert!(!resumed_from_sleep(MIN, None));
        assert!(!resumed_from_sleep(MIN, Some(Duration::ZERO)));
    }

    #[test]
    fn weekly_to_daily_with_the_last_check_two_days_ago_checks_at_once() {
        let t0 = Instant::now();
        let last = Some(NOW_UNIX - 2 * DAY);
        let (mut d, initial) =
            Deadlines::new(config(Some(UPDATE_CHECK_WEEKLY_SECS), last), t0, NOW_UNIX);
        assert!(initial.is_empty());
        assert_eq!(
            d.handle(
                Message::SetUpdateInterval {
                    interval_secs: Some(UPDATE_CHECK_DAILY_SECS),
                    last_check_unix: last,
                },
                t0,
                NOW_UNIX,
            ),
            [Action::CheckForUpdates]
        );
    }

    #[test]
    fn daily_to_weekly_waits_for_the_new_remainder() {
        let t0 = Instant::now();
        let last = Some(NOW_UNIX - 2 * DAY);
        let (mut d, _) = Deadlines::new(config(Some(UPDATE_CHECK_HOURLY_SECS), last), t0, NOW_UNIX);
        let actions = d.handle(
            Message::SetUpdateInterval {
                interval_secs: Some(UPDATE_CHECK_WEEKLY_SECS),
                last_check_unix: last,
            },
            t0,
            NOW_UNIX,
        );
        assert!(actions.is_empty());
        let remaining = 5 * DAY;
        assert!(!check_due(&mut d, t0, NOW_UNIX + remaining - 60));
        assert!(check_due(&mut d, t0, NOW_UNIX + remaining));
    }

    #[test]
    fn disabling_auto_check_cancels_the_deadline() {
        let t0 = Instant::now();
        let (mut d, _) = Deadlines::new(
            config(Some(UPDATE_CHECK_HOURLY_SECS), Some(NOW_UNIX - 600)),
            t0,
            NOW_UNIX,
        );
        d.handle(
            Message::SetUpdateInterval {
                interval_secs: None,
                last_check_unix: Some(NOW_UNIX - 600),
            },
            t0,
            NOW_UNIX,
        );
        assert!(!check_due(&mut d, t0, NOW_UNIX + 7 * DAY));
        // A check that finishes after auto-check was turned off stays off.
        d.handle(done(None, true), t0, NOW_UNIX);
        assert!(!check_due(&mut d, t0, NOW_UNIX + 14 * DAY));
    }

    /// Records actions on a channel, standing in for the runtime.
    struct FakeHost(Sender<Action>);

    impl SchedulerHost for FakeHost {
        fn run(&self, action: Action) {
            let _ = self.0.send(action);
        }
    }

    #[test]
    fn thread_runs_actions_and_stops_on_shutdown() {
        let (action_tx, action_rx) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        let handle = spawn(
            config(Some(UPDATE_CHECK_HOURLY_SECS), None),
            rx,
            FakeHost(action_tx),
        )
        .expect("spawn scheduler");
        let wait = Duration::from_secs(5);
        assert_eq!(action_rx.recv_timeout(wait), Ok(Action::CheckForUpdates));
        tx.send(Message::PollNow).expect("send");
        assert_eq!(action_rx.recv_timeout(wait), Ok(Action::Poll));
        tx.send(Message::RecomputeCountdown(Some(Duration::from_millis(20))))
            .expect("send");
        assert_eq!(action_rx.recv_timeout(wait), Ok(Action::Countdown));
        tx.send(Message::Shutdown).expect("send");
        handle.join().expect("join scheduler");
    }

    #[test]
    fn thread_stops_when_every_sender_is_dropped() {
        let (action_tx, _action_rx) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        let handle = spawn(config(None, None), rx, FakeHost(action_tx)).expect("spawn");
        drop(tx);
        handle.join().expect("join scheduler");
    }
}
