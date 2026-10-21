// Persisted settings. The path, schema, key names and physical-px positions
// stay byte-compatible with the Win32 builds, so no migration step exists.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::geom::{
    position_on_any_monitor, Rect, DEFAULT_BUBBLE_SIZE, MAX_BUBBLE_SIZE, MIN_BUBBLE_SIZE,
};
use crate::usage::ProviderId;

pub const APP_DIR_NAME: &str = "ClaudeCodeUsageBubble";
const SETTINGS_FILE: &str = "settings.json";

pub const POLL_1_MIN: u32 = 60_000;
pub const POLL_5_MIN: u32 = 5 * 60_000;
pub const POLL_15_MIN: u32 = 15 * 60_000;
pub const POLL_1_HOUR: u32 = 60 * 60_000;
/// The poll intervals the menu offers, in menu order.
pub const POLL_INTERVALS: [u32; 4] = [POLL_1_MIN, POLL_5_MIN, POLL_15_MIN, POLL_1_HOUR];

// Update-check intervals (seconds). `None` means auto-check is disabled.
pub const UPDATE_CHECK_HOURLY_SECS: u64 = 60 * 60;
pub const UPDATE_CHECK_DAILY_SECS: u64 = 24 * 60 * 60;
pub const UPDATE_CHECK_WEEKLY_SECS: u64 = 7 * 24 * 60 * 60;
/// The auto-update check choices the menu offers, in menu order.
pub const UPDATE_CHECK_INTERVALS: [Option<u64>; 4] = [
    None,
    Some(UPDATE_CHECK_HOURLY_SECS),
    Some(UPDATE_CHECK_DAILY_SECS),
    Some(UPDATE_CHECK_WEEKLY_SECS),
];

fn default_show_claude() -> bool {
    true
}
fn default_show_codex() -> bool {
    false
}
fn default_show_opencode_go() -> bool {
    false
}
fn default_widget_visible() -> bool {
    true
}
fn default_bubble_size() -> i32 {
    DEFAULT_BUBBLE_SIZE
}
fn default_poll_interval_ms() -> u32 {
    POLL_5_MIN
}
fn default_update_check_interval_secs() -> Option<u64> {
    Some(UPDATE_CHECK_HOURLY_SECS)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BubblePositions {
    pub claude: Option<(i32, i32)>,
    pub codex: Option<(i32, i32)>,
    pub opencode_go: Option<(i32, i32)>,
}

impl BubblePositions {
    pub fn get(&self, provider: ProviderId) -> Option<(i32, i32)> {
        match provider {
            ProviderId::Claude => self.claude,
            ProviderId::ChatGpt => self.codex,
            ProviderId::OpenCodeGo => self.opencode_go,
        }
    }
    pub fn set(&mut self, provider: ProviderId, pos: (i32, i32)) {
        match provider {
            ProviderId::Claude => self.claude = Some(pos),
            ProviderId::ChatGpt => self.codex = Some(pos),
            ProviderId::OpenCodeGo => self.opencode_go = Some(pos),
        }
    }
    pub fn reset(&mut self, provider: ProviderId) {
        match provider {
            ProviderId::Claude => self.claude = None,
            ProviderId::ChatGpt => self.codex = None,
            ProviderId::OpenCodeGo => self.opencode_go = None,
        }
    }
    pub fn reset_all(&mut self) {
        self.claude = None;
        self.codex = None;
        self.opencode_go = None;
    }

    /// Drop any saved position whose top-left no longer falls on one of
    /// `monitors`. Guards against creating a bubble on a disconnected
    /// secondary monitor, where the user can't see or recover it.
    pub fn validate(&mut self, monitors: &[Rect]) {
        for provider in ProviderId::ALL {
            if let Some((x, y)) = self.get(provider) {
                if !position_on_any_monitor(x, y, monitors) {
                    log::warn!(
                        "bubble position {} ({x},{y}) outside all monitors; resetting to default",
                        provider.slug()
                    );
                    self.reset(provider);
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_show_claude")]
    pub show_claude_code: bool,
    #[serde(default = "default_show_codex")]
    pub show_codex: bool,
    #[serde(default = "default_show_opencode_go")]
    pub show_opencode_go: bool,
    #[serde(default)]
    pub bubble_positions: BubblePositions,
    #[serde(default = "default_bubble_size")]
    pub bubble_size_logical: i32,
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u32,
    #[serde(default)]
    pub last_update_check_unix: Option<u64>,
    #[serde(default = "default_update_check_interval_secs")]
    pub update_check_interval_secs: Option<u64>,
    #[serde(default = "default_widget_visible")]
    pub widget_visible: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            show_claude_code: default_show_claude(),
            show_codex: default_show_codex(),
            show_opencode_go: default_show_opencode_go(),
            bubble_positions: BubblePositions::default(),
            bubble_size_logical: default_bubble_size(),
            poll_interval_ms: default_poll_interval_ms(),
            last_update_check_unix: None,
            update_check_interval_secs: default_update_check_interval_secs(),
            widget_visible: default_widget_visible(),
        }
    }
}

impl Settings {
    pub fn is_provider_enabled(&self, provider: ProviderId) -> bool {
        match provider {
            ProviderId::Claude => self.show_claude_code,
            ProviderId::ChatGpt => self.show_codex,
            ProviderId::OpenCodeGo => self.show_opencode_go,
        }
    }

    pub fn set_provider_enabled(&mut self, provider: ProviderId, enabled: bool) {
        match provider {
            ProviderId::Claude => self.show_claude_code = enabled,
            ProviderId::ChatGpt => self.show_codex = enabled,
            ProviderId::OpenCodeGo => self.show_opencode_go = enabled,
        }
    }

    pub fn toggle_provider(&mut self, provider: ProviderId) {
        let enabled = !self.is_provider_enabled(provider);
        self.set_provider_enabled(provider, enabled);
    }

    /// Menu toggle for a provider. Providers without live usage cannot be
    /// toggled, and unchecking the last enabled live provider is undone so
    /// the app always shows something. Returns the provider's new state.
    pub fn toggle_live_provider(&mut self, provider: ProviderId) -> bool {
        if provider.metadata().live_usage {
            self.toggle_provider(provider);
            if !self.has_enabled_live_provider() {
                self.set_provider_enabled(provider, true);
            }
        }
        self.is_provider_enabled(provider)
    }

    pub fn has_enabled_live_provider(&self) -> bool {
        ProviderId::LIVE_USAGE
            .iter()
            .any(|provider| self.is_provider_enabled(*provider))
    }
}

pub fn settings_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(APP_DIR_NAME))
}

pub fn settings_path() -> PathBuf {
    settings_dir()
        .unwrap_or_else(|| std::env::temp_dir().join(APP_DIR_NAME))
        .join(SETTINGS_FILE)
}

/// Read and normalise the settings file. A missing, unreadable or corrupt
/// file yields the defaults. Saved positions are not checked against the
/// monitors here; call [`drop_offscreen_positions`] once they are known.
pub fn load() -> Settings {
    load_from(&settings_path())
}

/// [`load`] from an explicit path.
pub fn load_from(path: &Path) -> Settings {
    match std::fs::read_to_string(path) {
        Ok(content) => parse_and_normalise(&content),
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound {
                log::warn!("settings read failed ({}): {err}", path.display());
            }
            Settings::default()
        }
    }
}

/// Parse settings JSON, falling back to the defaults when it is corrupt,
/// then enforce the invariants a hand-edited file may break.
pub fn parse_and_normalise(content: &str) -> Settings {
    let mut settings: Settings = serde_json::from_str(content).unwrap_or_else(|err| {
        log::warn!("settings file is not valid; using defaults: {err}");
        Settings::default()
    });
    // At least one live provider must be visible. Otherwise the app has nothing to show.
    if !settings.has_enabled_live_provider() {
        settings.show_claude_code = true;
    }
    // Clamp bubble size to safe range in case settings.json was hand-edited.
    settings.bubble_size_logical = settings
        .bubble_size_logical
        .clamp(MIN_BUBBLE_SIZE, MAX_BUBBLE_SIZE);
    settings
}

/// Drop positions on monitors that have since been disconnected.
/// `monitors` are the full monitor rects in physical px. An empty list means
/// the monitors could not be enumerated, so every position is kept rather
/// than all of them being thrown away.
pub fn drop_offscreen_positions(settings: &mut Settings, monitors: &[Rect]) {
    if monitors.is_empty() {
        log::warn!("no monitors reported; keeping saved bubble positions");
        return;
    }
    settings.bubble_positions.validate(monitors);
}

/// Persist `settings` with an atomic write (tmp file, then rename). Falls
/// back to a direct write when the rename fails.
pub fn save(settings: &Settings) -> std::io::Result<()> {
    save_to(&settings_path(), settings)
}

/// [`save`] to an explicit path.
pub fn save_to(path: &Path, settings: &Settings) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(std::io::Error::other)?;
    let tmp_path = path.with_extension("json.tmp");
    let atomic = std::fs::write(&tmp_path, &json).and_then(|()| std::fs::rename(&tmp_path, path));
    match atomic {
        Ok(()) => Ok(()),
        Err(err) => {
            log::warn!("atomic settings save failed, writing directly: {err}");
            let _ = std::fs::remove_file(&tmp_path);
            std::fs::write(path, json)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_two_provider_settings_json_still_loads() {
        let json = r#"{
            "show_claude_code": false,
            "show_codex": true,
            "bubble_positions": {
                "claude": [10, 20],
                "codex": [30, 40]
            }
        }"#;

        let settings: Settings =
            serde_json::from_str(json).expect("old settings json should parse");

        assert!(!settings.show_claude_code);
        assert!(settings.show_codex);
        assert!(!settings.show_opencode_go);
        assert_eq!(
            settings.bubble_positions.get(ProviderId::Claude),
            Some((10, 20))
        );
        assert_eq!(
            settings.bubble_positions.get(ProviderId::ChatGpt),
            Some((30, 40))
        );
        assert_eq!(settings.bubble_positions.get(ProviderId::OpenCodeGo), None);
    }

    #[test]
    fn three_provider_settings_json_loads() {
        let json = r#"{
            "show_claude_code": true,
            "show_codex": false,
            "show_opencode_go": true,
            "bubble_positions": {
                "opencode_go": [50, 60]
            }
        }"#;

        let settings: Settings =
            serde_json::from_str(json).expect("new settings json should parse");

        assert!(settings.show_claude_code);
        assert!(!settings.show_codex);
        assert!(settings.show_opencode_go);
        assert_eq!(
            settings.bubble_positions.get(ProviderId::OpenCodeGo),
            Some((50, 60))
        );
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let settings = parse_and_normalise("{ not json");
        assert!(settings.show_claude_code);
        assert!(!settings.show_codex);
        assert_eq!(settings.bubble_size_logical, DEFAULT_BUBBLE_SIZE);
        assert_eq!(settings.poll_interval_ms, POLL_5_MIN);
        assert_eq!(
            settings.update_check_interval_secs,
            Some(UPDATE_CHECK_HOURLY_SECS)
        );
        assert!(settings.widget_visible);
    }

    #[test]
    fn normalise_restores_a_live_provider_and_clamps_size() {
        let settings = parse_and_normalise(
            r#"{"show_claude_code": false, "show_codex": false, "show_opencode_go": true,
                "bubble_size_logical": 9000}"#,
        );
        assert!(settings.show_claude_code);
        assert_eq!(settings.bubble_size_logical, MAX_BUBBLE_SIZE);
        let small = parse_and_normalise(r#"{"bubble_size_logical": 10}"#);
        assert_eq!(small.bubble_size_logical, MIN_BUBBLE_SIZE);
    }

    #[test]
    fn serialised_keys_stay_compatible() {
        let value = serde_json::to_value(Settings::default()).expect("serialise");
        let mut keys: Vec<_> = value.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "bubble_positions",
                "bubble_size_logical",
                "last_update_check_unix",
                "poll_interval_ms",
                "show_claude_code",
                "show_codex",
                "show_opencode_go",
                "update_check_interval_secs",
                "widget_visible",
            ]
        );
        let mut s = Settings::default();
        s.bubble_positions.set(ProviderId::ChatGpt, (-1900, 40));
        let json = serde_json::to_string(&s).expect("serialise");
        assert!(json.contains(r#""codex":[-1900,40]"#), "{json}");
    }

    #[test]
    fn offscreen_positions_are_dropped_against_monitors() {
        let mut settings = parse_and_normalise(
            r#"{"bubble_positions": {"claude": [100, 100], "codex": [-3000, 100],
                "opencode_go": [2000, 500]}}"#,
        );
        let monitors = [Rect::new(0, 0, 1920, 1080), Rect::new(1920, 0, 3840, 1080)];
        drop_offscreen_positions(&mut settings, &monitors);
        let p = &settings.bubble_positions;
        assert_eq!(p.get(ProviderId::Claude), Some((100, 100)));
        assert_eq!(p.get(ProviderId::ChatGpt), None);
        assert_eq!(p.get(ProviderId::OpenCodeGo), Some((2000, 500)));
    }

    #[test]
    fn no_monitors_keeps_saved_positions() {
        let mut settings = Settings::default();
        settings
            .bubble_positions
            .set(ProviderId::Claude, (-3000, 100));
        drop_offscreen_positions(&mut settings, &[]);
        assert_eq!(
            settings.bubble_positions.get(ProviderId::Claude),
            Some((-3000, 100))
        );
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = std::env::temp_dir().join(format!(
            "ccub-settings-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let path = dir.join(SETTINGS_FILE);
        let mut settings = Settings {
            show_codex: true,
            poll_interval_ms: POLL_15_MIN,
            update_check_interval_secs: None,
            last_update_check_unix: Some(1_700_000_000),
            ..Settings::default()
        };
        settings.bubble_positions.set(ProviderId::Claude, (12, 34));
        save_to(&path, &settings).expect("save");
        assert!(!path.with_extension("json.tmp").exists());
        let loaded = load_from(&path);
        assert!(loaded.show_codex);
        assert_eq!(loaded.poll_interval_ms, POLL_15_MIN);
        assert_eq!(loaded.update_check_interval_secs, None);
        assert_eq!(loaded.last_update_check_unix, Some(1_700_000_000));
        assert_eq!(
            loaded.bubble_positions.get(ProviderId::Claude),
            Some((12, 34))
        );
        std::fs::remove_dir_all(&dir).expect("cleanup");
        // A missing file loads the defaults.
        assert!(load_from(&path).show_claude_code);
    }

    #[test]
    fn toggle_keeps_provider_flags_independent() {
        let mut settings = Settings::default();
        settings.toggle_provider(ProviderId::ChatGpt);
        assert!(settings.is_provider_enabled(ProviderId::ChatGpt));
        settings.toggle_provider(ProviderId::Claude);
        assert!(!settings.is_provider_enabled(ProviderId::Claude));
        assert!(settings.has_enabled_live_provider());
        settings.toggle_provider(ProviderId::ChatGpt);
        assert!(!settings.has_enabled_live_provider());
    }

    #[test]
    fn live_toggle_never_disables_the_last_live_provider() {
        let mut settings = Settings::default();
        assert!(settings.toggle_live_provider(ProviderId::Claude));
        assert!(settings.show_claude_code);
        assert!(settings.toggle_live_provider(ProviderId::ChatGpt));
        assert!(!settings.toggle_live_provider(ProviderId::Claude));
        assert!(settings.toggle_live_provider(ProviderId::ChatGpt));
        // OpenCode Go has no live usage, so its flag never changes.
        assert!(!settings.toggle_live_provider(ProviderId::OpenCodeGo));
        assert!(!settings.show_opencode_go);
    }
}
