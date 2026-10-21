//! Portable app state rules: applying poll results, alert crossings and
//! cooldown, the tray tooltip, the update status label and the update-check
//! schedule.
//!
//! None of this touches a window, tray or menu. Callers apply the returned
//! values after dropping their state lock.

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

use crate::core::i18n::{self, LocaleStrings};
use crate::usage::{ProviderId, UsageWindows};

/// Minimum gap between two cooldown-gated alerts (threshold and session
/// expired), so notifications stay calm.
pub const ALERT_COOLDOWN: Duration = Duration::from_secs(30 * 60);

/// Usage thresholds that raise an alert the poll they are crossed.
pub const ALERT_THRESHOLDS: [u8; 2] = [80, 95];

/// Shortest countdown re-render delay.
const MIN_COUNTDOWN_DELAY: Duration = Duration::from_secs(1);

/// One provider's last known usage plus its panel/tooltip texts.
#[derive(Clone, Debug, Default)]
pub struct ProviderUiState {
    pub windows: UsageWindows,
    /// `format_window` text for the session window, `"!"` after an auth
    /// failure, `"…"` after a transient error.
    pub primary_text: String,
    /// Same as `primary_text` for the weekly window.
    pub secondary_text: String,
}

/// How one provider's poll ended, independent of the HTTP error type.
#[derive(Clone, Debug)]
pub enum PollOutcome {
    Ok(UsageWindows),
    /// The credentials were rejected (auth required or token expired).
    AuthFailed,
    /// Any other failure: network, bad response, missing credentials.
    Failed,
}

/// A notification to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Alert {
    Threshold { provider: ProviderId, threshold: u8 },
    SessionExpired { provider: ProviderId },
    UpdateApplied { version: String },
}

impl Alert {
    /// `(title, body)` for the notification.
    pub fn text(&self, strings: &LocaleStrings) -> (String, String) {
        match self {
            Alert::Threshold {
                provider,
                threshold,
            } => {
                let title = format!("{} · {threshold}%", strings.provider_label(*provider));
                let body = if *threshold >= 95 {
                    strings.threshold_95_body.clone()
                } else {
                    strings.threshold_80_body.clone()
                };
                (title, body)
            }
            Alert::SessionExpired { provider } => match provider {
                ProviderId::Claude => (
                    strings.token_expired_title.clone(),
                    strings.token_expired_body.clone(),
                ),
                ProviderId::ChatGpt => (
                    strings.chatgpt_token_expired_title.clone(),
                    strings.chatgpt_token_expired_body.clone(),
                ),
                ProviderId::OpenCodeGo => (
                    strings.opencode_go_label.clone(),
                    strings.update_failed.clone(),
                ),
            },
            Alert::UpdateApplied { version } => (
                strings.update_applied_title.clone(),
                format!("{}{version}", strings.update_applied_body),
            ),
        }
    }
}

/// What a batch of poll results changed beyond the state itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    /// Threshold alerts that passed the cooldown, in firing order.
    pub alerts: Vec<Alert>,
    /// Providers whose credentials were rejected; the caller runs the CLI
    /// refresh for them.
    pub auth_failures: Vec<ProviderId>,
}

/// Usage snapshots and the alert cooldown.
#[derive(Clone, Debug, Default)]
pub struct UsageState {
    pub snapshots: HashMap<ProviderId, ProviderUiState>,
    /// True when at least one provider succeeded on the last non-empty poll.
    /// Shared by all providers, as before.
    pub last_poll_ok: bool,
    pub last_alert_at: Option<Instant>,
}

impl UsageState {
    /// Apply one poll's results. A provider fires an alert only the poll its
    /// session usage crosses 80 or 95, and each alert must pass the
    /// cooldown. An empty batch changes nothing.
    pub fn apply_results(
        &mut self,
        results: Vec<(ProviderId, PollOutcome)>,
        strings: &LocaleStrings,
        now: Instant,
    ) -> Applied {
        let mut applied = Applied::default();
        if results.is_empty() {
            return applied;
        }
        let mut crossings: Vec<(ProviderId, u8)> = Vec::new();
        let mut any_ok = false;
        for (id, outcome) in results {
            let entry = self.snapshots.entry(id).or_default();
            match outcome {
                PollOutcome::Ok(windows) => {
                    let old_pct = entry.windows.primary.utilization;
                    let new_pct = windows.primary.utilization;
                    for threshold in ALERT_THRESHOLDS {
                        if old_pct < threshold as f64 && new_pct >= threshold as f64 {
                            crossings.push((id, threshold));
                        }
                    }
                    entry.windows = windows;
                    entry.primary_text = i18n::format_window(&windows.primary, strings);
                    entry.secondary_text = i18n::format_window(&windows.secondary, strings);
                    any_ok = true;
                }
                PollOutcome::AuthFailed => {
                    applied.auth_failures.push(id);
                    entry.primary_text = "!".into();
                    entry.secondary_text = "!".into();
                }
                PollOutcome::Failed => {
                    entry.primary_text = "…".into();
                    entry.secondary_text = "…".into();
                }
            }
        }
        self.last_poll_ok = any_ok;
        for (provider, threshold) in crossings {
            if self.admit_alert(now) {
                applied.alerts.push(Alert::Threshold {
                    provider,
                    threshold,
                });
            }
        }
        applied
    }

    /// Claim the alert cooldown. Returns false while the previous gated
    /// alert is younger than [`ALERT_COOLDOWN`].
    pub fn admit_alert(&mut self, now: Instant) -> bool {
        if let Some(last) = self.last_alert_at {
            if now.saturating_duration_since(last) < ALERT_COOLDOWN {
                return false;
            }
        }
        self.last_alert_at = Some(now);
        true
    }

    /// The session-expired alert for `provider`, if the cooldown allows one.
    pub fn session_expired_alert(&mut self, provider: ProviderId, now: Instant) -> Option<Alert> {
        self.admit_alert(now)
            .then_some(Alert::SessionExpired { provider })
    }

    /// Re-format every countdown text after time passed. Error markers are
    /// replaced too, matching the countdown timer of the Win32 build.
    pub fn refresh_texts(&mut self, strings: &LocaleStrings) {
        for entry in self.snapshots.values_mut() {
            entry.primary_text = i18n::format_window(&entry.windows.primary, strings);
            entry.secondary_text = i18n::format_window(&entry.windows.secondary, strings);
        }
    }

    /// Delay until any displayed countdown changes, at least one second.
    /// `None` when no window has a reset time ahead.
    pub fn countdown_delay(&self, now: SystemTime) -> Option<Duration> {
        self.snapshots
            .values()
            .flat_map(|e| [e.windows.primary.resets_at, e.windows.secondary.resets_at])
            .filter_map(|reset| i18n::time_until_display_change_at(reset, now))
            .min()
            .map(|d| d.max(MIN_COUNTDOWN_DELAY))
    }
}

/// Four-line tray tooltip: label, session, weekly and the click hint.
pub fn tray_tooltip(
    label: &str,
    entry: Option<&ProviderUiState>,
    strings: &LocaleStrings,
) -> String {
    let session = entry
        .map(|e| e.primary_text.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("...");
    let weekly = entry
        .map(|e| e.secondary_text.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("...");
    format!(
        "{label}\n{}: {session}\n{}: {weekly}\n{}",
        strings.session_window, strings.weekly_window, strings.tray_left_click
    )
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available,
    Applying,
    Failed,
}

impl UpdateStatus {
    /// Whether a check or apply is running, which disables the menu item.
    pub fn is_busy(self) -> bool {
        matches!(self, UpdateStatus::Checking | UpdateStatus::Applying)
    }
}

/// The version menu item: `"<status> · v<version>"`.
pub fn version_action_label(status: UpdateStatus, strings: &LocaleStrings) -> String {
    let base = match status {
        UpdateStatus::Idle => &strings.check_for_updates,
        UpdateStatus::Checking => &strings.checking_for_updates,
        UpdateStatus::UpToDate => &strings.up_to_date,
        UpdateStatus::Available => &strings.update_available,
        UpdateStatus::Applying => &strings.applying_update,
        UpdateStatus::Failed => &strings.update_failed,
    };
    // The running binary's version, so the user can see what they are on
    // without an About dialog. The middle dot matches the countdown text.
    format!("{base} \u{00b7} v{}", env!("CARGO_PKG_VERSION"))
}

/// When the next automatic update check runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateCheckDue {
    Now,
    After(Duration),
}

/// Schedule the automatic update check from the last check time (unix
/// seconds). `None` when auto-check is disabled. Due when never checked or
/// when `now - last >= interval`; otherwise after the remainder.
pub fn update_check_due(
    last_check_unix: Option<u64>,
    interval_secs: Option<u64>,
    now_unix: u64,
) -> Option<UpdateCheckDue> {
    let interval = interval_secs?;
    let Some(last) = last_check_unix else {
        return Some(UpdateCheckDue::Now);
    };
    let elapsed = now_unix.saturating_sub(last);
    if elapsed >= interval {
        Some(UpdateCheckDue::Now)
    } else {
        Some(UpdateCheckDue::After(Duration::from_secs(
            interval - elapsed,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::i18n::I18n;
    use crate::usage::Window;

    fn windows(primary: f64) -> UsageWindows {
        UsageWindows {
            primary: Window {
                utilization: primary,
                resets_at: None,
            },
            secondary: Window {
                utilization: 10.0,
                resets_at: None,
            },
        }
    }

    fn poll(state: &mut UsageState, pct: f64, now: Instant) -> Applied {
        let strings = I18n::load().strings().clone();
        state.apply_results(
            vec![(ProviderId::Claude, PollOutcome::Ok(windows(pct)))],
            &strings,
            now,
        )
    }

    fn threshold(t: u8) -> Alert {
        Alert::Threshold {
            provider: ProviderId::Claude,
            threshold: t,
        }
    }

    #[test]
    fn crossing_eighty_fires_once() {
        let now = Instant::now();
        let mut state = UsageState::default();
        assert!(poll(&mut state, 79.0, now).alerts.is_empty());
        assert_eq!(poll(&mut state, 80.0, now).alerts, [threshold(80)]);
        // Parked above the threshold: no new crossing, even after the cooldown.
        let later = now + ALERT_COOLDOWN;
        assert!(poll(&mut state, 85.0, later).alerts.is_empty());
    }

    #[test]
    fn double_crossing_is_cut_to_one_by_the_cooldown() {
        let now = Instant::now();
        let mut state = UsageState::default();
        poll(&mut state, 79.0, now);
        assert_eq!(poll(&mut state, 96.0, now).alerts, [threshold(80)]);
    }

    #[test]
    fn first_poll_above_threshold_alerts() {
        let mut state = UsageState::default();
        assert_eq!(
            poll(&mut state, 90.0, Instant::now()).alerts,
            [threshold(80)]
        );
    }

    #[test]
    fn cooldown_expires_after_thirty_minutes() {
        let now = Instant::now();
        let mut state = UsageState::default();
        assert!(state.admit_alert(now));
        assert!(!state.admit_alert(now + ALERT_COOLDOWN - Duration::from_secs(1)));
        assert_eq!(state.session_expired_alert(ProviderId::ChatGpt, now), None);
        assert_eq!(
            state.session_expired_alert(ProviderId::ChatGpt, now + ALERT_COOLDOWN),
            Some(Alert::SessionExpired {
                provider: ProviderId::ChatGpt
            })
        );
    }

    #[test]
    fn errors_set_marker_texts_and_auth_failures() {
        let strings = I18n::load().strings().clone();
        let mut state = UsageState::default();
        poll(&mut state, 40.0, Instant::now());
        let applied = state.apply_results(
            vec![
                (ProviderId::Claude, PollOutcome::AuthFailed),
                (ProviderId::ChatGpt, PollOutcome::Failed),
            ],
            &strings,
            Instant::now(),
        );
        assert_eq!(applied.auth_failures, [ProviderId::Claude]);
        assert!(!state.last_poll_ok);
        let claude = &state.snapshots[&ProviderId::Claude];
        assert_eq!(claude.primary_text, "!");
        assert_eq!(claude.secondary_text, "!");
        // The stored windows survive the error.
        assert_eq!(claude.windows.primary.utilization, 40.0);
        let codex = &state.snapshots[&ProviderId::ChatGpt];
        assert_eq!(codex.primary_text, "…");
        assert_eq!(codex.secondary_text, "…");
    }

    #[test]
    fn last_poll_ok_is_global_and_empty_batches_change_nothing() {
        let strings = I18n::load().strings().clone();
        let mut state = UsageState::default();
        let applied = state.apply_results(
            vec![
                (ProviderId::Claude, PollOutcome::Failed),
                (ProviderId::ChatGpt, PollOutcome::Ok(windows(5.0))),
            ],
            &strings,
            Instant::now(),
        );
        assert!(state.last_poll_ok);
        assert!(applied.auth_failures.is_empty());
        assert_eq!(state.snapshots[&ProviderId::ChatGpt].primary_text, "5%");
        assert_eq!(
            state.apply_results(Vec::new(), &strings, Instant::now()),
            Applied::default()
        );
        assert!(state.last_poll_ok);
    }

    #[test]
    fn alert_texts() {
        let strings = I18n::load().strings().clone();
        assert_eq!(
            threshold(95).text(&strings),
            (
                "Claude Code · 95%".to_string(),
                "Limit is close - consider easing up.".to_string()
            )
        );
        assert_eq!(
            Alert::SessionExpired {
                provider: ProviderId::ChatGpt
            }
            .text(&strings)
            .0,
            "Codex session expired"
        );
        assert_eq!(
            Alert::UpdateApplied {
                version: "0.6.0".into()
            }
            .text(&strings)
            .1,
            "Updated to v0.6.0"
        );
    }

    #[test]
    fn tooltip_uses_dots_for_empty_texts() {
        let strings = I18n::load().strings().clone();
        assert_eq!(
            tray_tooltip("Claude Code", None, &strings),
            "Claude Code\n5h: ...\n7d: ...\nLeft-click: show/hide"
        );
        let entry = ProviderUiState {
            primary_text: "73% · 2h".into(),
            secondary_text: String::new(),
            ..ProviderUiState::default()
        };
        assert_eq!(
            tray_tooltip("Codex", Some(&entry), &strings),
            "Codex\n5h: 73% · 2h\n7d: ...\nLeft-click: show/hide"
        );
    }

    #[test]
    fn version_label_carries_status_and_version() {
        let strings = I18n::load().strings().clone();
        let v = env!("CARGO_PKG_VERSION");
        assert_eq!(
            version_action_label(UpdateStatus::Idle, &strings),
            format!("Check for updates · v{v}")
        );
        assert_eq!(
            version_action_label(UpdateStatus::Applying, &strings),
            format!("Applying update... · v{v}")
        );
        assert!(UpdateStatus::Checking.is_busy());
        assert!(!UpdateStatus::Available.is_busy());
    }

    #[test]
    fn update_schedule_due_and_remaining() {
        let hour = 3_600;
        assert_eq!(update_check_due(None, None, 10_000), None);
        assert_eq!(update_check_due(Some(9_000), None, 10_000), None);
        assert_eq!(
            update_check_due(None, Some(hour), 10_000),
            Some(UpdateCheckDue::Now)
        );
        assert_eq!(
            update_check_due(Some(10_000 - hour), Some(hour), 10_000),
            Some(UpdateCheckDue::Now)
        );
        assert_eq!(
            update_check_due(Some(10_000 - 600), Some(hour), 10_000),
            Some(UpdateCheckDue::After(Duration::from_secs(3_000)))
        );
        // A clock that moved backwards counts as zero elapsed.
        assert_eq!(
            update_check_due(Some(20_000), Some(hour), 10_000),
            Some(UpdateCheckDue::After(Duration::from_secs(hour)))
        );
    }

    #[test]
    fn countdown_delay_takes_the_nearest_change_with_a_floor() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let mut state = UsageState::default();
        assert_eq!(state.countdown_delay(now), None);
        let mut w = windows(10.0);
        w.primary.resets_at = Some(now + Duration::from_secs(2 * 3_600 + 30));
        w.secondary.resets_at = Some(now + Duration::from_secs(3 * 86_400 + 90));
        state.snapshots.insert(
            ProviderId::Claude,
            ProviderUiState {
                windows: w,
                ..ProviderUiState::default()
            },
        );
        assert_eq!(state.countdown_delay(now), Some(Duration::from_secs(31)));
        w.primary.resets_at = Some(now + Duration::from_secs(20));
        state
            .snapshots
            .get_mut(&ProviderId::Claude)
            .expect("entry")
            .windows = w;
        assert_eq!(state.countdown_delay(now), Some(Duration::from_secs(1)));
    }
}
