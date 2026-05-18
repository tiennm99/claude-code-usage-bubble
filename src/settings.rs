use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{MonitorFromRect, MONITOR_DEFAULTTONULL};

use crate::bubble::DEFAULT_BUBBLE_SIZE;
use crate::usage::ProviderId;
type TrayIconKind = ProviderId;

// 140px matches MIN_BUBBLE_SIZE — a saved top-left a few px past the work-area
// edge still passes the validator, but a position fully on a disconnected
// monitor (the bug we're guarding against) fails.
const POSITION_PROBE_PX: i32 = 140;

const APP_DIR_NAME: &str = "ClaudeCodeUsageBubble";
const SETTINGS_FILE: &str = "settings.json";

pub const POLL_1_MIN: u32 = 60_000;
pub const POLL_5_MIN: u32 = 5 * 60_000;
pub const POLL_15_MIN: u32 = 15 * 60_000;
pub const POLL_1_HOUR: u32 = 60 * 60_000;

// Update-check intervals (seconds). `None` means auto-check is disabled.
pub const UPDATE_CHECK_HOURLY_SECS: u64 = 60 * 60;
pub const UPDATE_CHECK_DAILY_SECS: u64 = 24 * 60 * 60;
pub const UPDATE_CHECK_WEEKLY_SECS: u64 = 7 * 24 * 60 * 60;

fn default_show_claude() -> bool {
    true
}
fn default_show_codex() -> bool {
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
}

impl BubblePositions {
    pub fn get(&self, model: TrayIconKind) -> Option<(i32, i32)> {
        match model {
            ProviderId::Claude => self.claude,
            ProviderId::ChatGpt => self.codex,
        }
    }
    pub fn set(&mut self, model: TrayIconKind, pos: (i32, i32)) {
        match model {
            ProviderId::Claude => self.claude = Some(pos),
            ProviderId::ChatGpt => self.codex = Some(pos),
        }
    }
    pub fn reset(&mut self, model: TrayIconKind) {
        match model {
            ProviderId::Claude => self.claude = None,
            ProviderId::ChatGpt => self.codex = None,
        }
    }
    pub fn reset_all(&mut self) {
        self.claude = None;
        self.codex = None;
    }

    /// Drop any saved position whose top-left no longer falls on a connected
    /// monitor. Guards against `bubble::create` placing the window on a
    /// disconnected secondary monitor (where the user can't see or recover it).
    pub fn validate(&mut self) {
        if let Some((x, y)) = self.claude {
            if !position_on_any_monitor(x, y) {
                log::warn!("bubble position claude ({x},{y}) outside all monitors; resetting to default");
                self.claude = None;
            }
        }
        if let Some((x, y)) = self.codex {
            if !position_on_any_monitor(x, y) {
                log::warn!("bubble position codex ({x},{y}) outside all monitors; resetting to default");
                self.codex = None;
            }
        }
    }
}

fn position_on_any_monitor(x: i32, y: i32) -> bool {
    // MONITOR_DEFAULTTONULL returns a null HMONITOR when the rect intersects
    // no connected monitor — exactly the signal we want.
    let probe = RECT {
        left: x,
        top: y,
        right: x + POSITION_PROBE_PX,
        bottom: y + POSITION_PROBE_PX,
    };
    let monitor = unsafe { MonitorFromRect(&probe, MONITOR_DEFAULTTONULL) };
    !monitor.is_invalid()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_show_claude")]
    pub show_claude_code: bool,
    #[serde(default = "default_show_codex")]
    pub show_codex: bool,
    #[serde(default)]
    pub bubble_positions: BubblePositions,
    #[serde(default = "default_bubble_size")]
    pub bubble_size_logical: i32,
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u32,
    #[serde(default)]
    pub language: Option<String>,
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
            bubble_positions: BubblePositions::default(),
            bubble_size_logical: default_bubble_size(),
            poll_interval_ms: default_poll_interval_ms(),
            language: None,
            last_update_check_unix: None,
            update_check_interval_secs: default_update_check_interval_secs(),
            widget_visible: default_widget_visible(),
        }
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

pub fn load() -> Settings {
    let path = settings_path();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Settings::default(),
    };
    let mut settings: Settings = serde_json::from_str(&content).unwrap_or_default();
    // At least one model must be visible. Otherwise the app has nothing to show.
    if !settings.show_claude_code && !settings.show_codex {
        settings.show_claude_code = true;
    }
    // Clamp bubble size to safe range in case settings.json was hand-edited.
    settings.bubble_size_logical = settings
        .bubble_size_logical
        .clamp(crate::bubble::MIN_BUBBLE_SIZE, crate::bubble::MAX_BUBBLE_SIZE);
    // Drop positions on monitors that have since been disconnected.
    settings.bubble_positions.validate();
    settings
}

pub fn save(settings: &Settings) {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(json) = serde_json::to_string_pretty(settings) else {
        return;
    };
    // Atomic write: tmp then rename. Falls back to direct write on rename failure.
    let tmp_path = path.with_extension("json.tmp");
    if std::fs::write(&tmp_path, &json).is_ok() && std::fs::rename(&tmp_path, &path).is_ok() {
        return;
    }
    let _ = std::fs::write(&path, json);
}
