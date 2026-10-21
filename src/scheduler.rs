//! The single timer thread. It owns the poll, countdown and update-check
//! deadlines and is driven by messages over an `mpsc` channel.
//!
//! The deadline rules live in [`Deadlines`], which takes the current time as
//! an argument and so is tested without a thread, a window or a clock. The
//! thread only waits for the nearest deadline or the next message and hands
//! each due [`Action`] to its [`SchedulerHost`]. A host must return quickly:
//! slow work (HTTP, the CLI refresh) runs on worker threads it spawns.

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
    /// An update check finished. Only this re-arms the full interval.
    UpdateCheckDone { interval_secs: Option<u64> },
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
    update_at: Option<Instant>,
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
            update_at: None,
        };
        let actions = deadlines.arm_update(
            config.update_interval_secs,
            config.last_update_check_unix,
            now,
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
            } => self.arm_update(interval_secs, last_check_unix, now, now_unix),
            Message::UpdateCheckDone { interval_secs } => {
                self.update_at = interval_secs.map(|secs| now + update_interval(secs));
                Vec::new()
            }
            Message::Shutdown => Vec::new(),
        }
    }

    /// Fire every deadline that has passed. The poll re-arms for a full
    /// interval; the countdown and update check stay disarmed until the
    /// host re-arms them (after the re-render, after the check finished).
    pub fn take_due(&mut self, now: Instant) -> Vec<Action> {
        let mut due = Vec::new();
        if self.poll_at <= now {
            due.push(Action::Poll);
            self.poll_at = now + self.poll_interval;
        }
        if self.countdown_at.is_some_and(|at| at <= now) {
            due.push(Action::Countdown);
            self.countdown_at = None;
        }
        if self.update_at.is_some_and(|at| at <= now) {
            due.push(Action::CheckForUpdates);
            self.update_at = None;
        }
        due
    }

    /// Time until the nearest deadline. The poll deadline is always armed,
    /// so there always is one.
    pub fn next_wakeup(&self, now: Instant) -> Duration {
        [Some(self.poll_at), self.countdown_at, self.update_at]
            .into_iter()
            .flatten()
            .min()
            .map_or(self.poll_interval, |at| at.saturating_duration_since(now))
    }

    fn arm_update(
        &mut self,
        interval_secs: Option<u64>,
        last_check_unix: Option<u64>,
        now: Instant,
        now_unix: u64,
    ) -> Vec<Action> {
        let interval_secs = interval_secs.map(|secs| secs.max(MIN_UPDATE_INTERVAL.as_secs()));
        match update_check_due(last_check_unix, interval_secs, now_unix) {
            None => {
                self.update_at = None;
                Vec::new()
            }
            Some(UpdateCheckDue::Now) => {
                self.update_at = None;
                vec![Action::CheckForUpdates]
            }
            Some(UpdateCheckDue::After(remaining)) => {
                self.update_at = Some(now + remaining);
                Vec::new()
            }
        }
    }
}

fn update_interval(secs: u64) -> Duration {
    Duration::from_secs(secs).max(MIN_UPDATE_INTERVAL)
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
    loop {
        for action in deadlines.take_due(Instant::now()) {
            host.run(action);
        }
        let message = match rx.recv_timeout(deadlines.next_wakeup(Instant::now())) {
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
        assert_eq!(d.next_wakeup(t0), 5 * MIN);
        assert!(d.take_due(t0 + 4 * MIN).is_empty());
        assert_eq!(d.take_due(t0 + 5 * MIN), [Action::Poll]);
        assert_eq!(d.next_wakeup(t0 + 5 * MIN), 5 * MIN);
        assert_eq!(d.take_due(t0 + 10 * MIN), [Action::Poll]);
    }

    #[test]
    fn poll_now_polls_without_moving_the_periodic_deadline() {
        let t0 = Instant::now();
        let (mut d, _) = Deadlines::new(config(None, None), t0, NOW_UNIX);
        assert_eq!(
            d.handle(Message::PollNow, t0 + 2 * MIN, NOW_UNIX),
            [Action::Poll]
        );
        assert_eq!(d.take_due(t0 + 5 * MIN), [Action::Poll]);
    }

    #[test]
    fn new_poll_interval_rearms_from_now_without_polling() {
        let t0 = Instant::now();
        let (mut d, _) = Deadlines::new(config(None, None), t0, NOW_UNIX);
        let t1 = t0 + 4 * MIN;
        assert!(d
            .handle(Message::SetPollInterval(MIN * 15), t1, NOW_UNIX)
            .is_empty());
        assert!(d.take_due(t0 + 5 * MIN).is_empty());
        assert_eq!(d.next_wakeup(t1), 15 * MIN);
        assert_eq!(d.take_due(t1 + 15 * MIN), [Action::Poll]);
    }

    #[test]
    fn a_zero_poll_interval_is_floored() {
        let t0 = Instant::now();
        let mut cfg = config(None, None);
        cfg.poll_interval = Duration::ZERO;
        let (d, _) = Deadlines::new(cfg, t0, NOW_UNIX);
        assert_eq!(d.next_wakeup(t0), MIN_POLL_INTERVAL);
    }

    #[test]
    fn countdown_fires_once_until_rearmed() {
        let t0 = Instant::now();
        let (mut d, _) = Deadlines::new(config(None, None), t0, NOW_UNIX);
        let delay = Duration::from_secs(31);
        d.handle(Message::RecomputeCountdown(Some(delay)), t0, NOW_UNIX);
        assert_eq!(d.next_wakeup(t0), delay);
        assert_eq!(d.take_due(t0 + delay), [Action::Countdown]);
        assert!(d.take_due(t0 + 2 * delay).is_empty());

        d.handle(Message::RecomputeCountdown(Some(delay)), t0, NOW_UNIX);
        d.handle(Message::RecomputeCountdown(None), t0, NOW_UNIX);
        assert!(d.take_due(t0 + 2 * delay).is_empty());
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
        let remaining = Duration::from_secs(3_000);
        assert!(!d
            .take_due(t0 + remaining - MIN)
            .contains(&Action::CheckForUpdates));
        assert_eq!(d.next_wakeup(t0 + remaining - MIN), MIN);
        assert_eq!(d.take_due(t0 + remaining), [Action::CheckForUpdates]);
    }

    #[test]
    fn only_a_finished_check_rearms_the_full_interval() {
        let t0 = Instant::now();
        let hour = Duration::from_secs(UPDATE_CHECK_HOURLY_SECS);
        let (mut d, initial) =
            Deadlines::new(config(Some(UPDATE_CHECK_HOURLY_SECS), None), t0, NOW_UNIX);
        assert_eq!(initial, [Action::CheckForUpdates]);
        // While the check runs nothing else is scheduled for it.
        assert!(!d.take_due(t0 + 3 * hour).contains(&Action::CheckForUpdates));

        let t1 = t0 + 3 * hour;
        d.handle(
            Message::UpdateCheckDone {
                interval_secs: Some(UPDATE_CHECK_HOURLY_SECS),
            },
            t1,
            NOW_UNIX,
        );
        assert!(!d.take_due(t1 + hour / 2).contains(&Action::CheckForUpdates));
        assert!(d.take_due(t1 + hour).contains(&Action::CheckForUpdates));
        // It repeats every interval, not just once.
        let t2 = t1 + hour;
        d.handle(
            Message::UpdateCheckDone {
                interval_secs: Some(UPDATE_CHECK_HOURLY_SECS),
            },
            t2,
            NOW_UNIX,
        );
        assert!(d.take_due(t2 + hour).contains(&Action::CheckForUpdates));
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
        let remaining = Duration::from_secs(5 * DAY);
        assert!(!d
            .take_due(t0 + remaining - MIN)
            .contains(&Action::CheckForUpdates));
        assert!(d
            .take_due(t0 + remaining)
            .contains(&Action::CheckForUpdates));
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
        assert!(!d
            .take_due(t0 + Duration::from_secs(7 * DAY))
            .contains(&Action::CheckForUpdates));
        // A check that finishes after auto-check was turned off stays off.
        d.handle(
            Message::UpdateCheckDone {
                interval_secs: None,
            },
            t0,
            NOW_UNIX,
        );
        assert!(!d
            .take_due(t0 + Duration::from_secs(14 * DAY))
            .contains(&Action::CheckForUpdates));
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
