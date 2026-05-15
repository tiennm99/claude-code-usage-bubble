// Application orchestrator: single-instance mutex, settings, polling thread,
// tray icons, context menu, message-only window for cross-thread updates, and
// dispatch from bubble UI callbacks.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::core::PCWSTR;
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::bubble;
use crate::diagnose;
use crate::localization::{self, LanguageId, Strings};
use crate::models::AppUsageData;
use crate::native_interop::{
    wide_str, TIMER_COUNTDOWN, TIMER_POLL, TIMER_RESET_POLL, TIMER_UPDATE_CHECK, WM_APP_TRAY,
    WM_APP_USAGE_UPDATED,
};
use crate::panel::{self, PanelData};
use crate::poller::{self, PollError};
use crate::settings::{self, BubblePositions, Settings, POLL_15_MIN, POLL_1_HOUR, POLL_1_MIN, POLL_5_MIN};
use crate::theme;
use crate::tray_icon::{self, TrayAction, TrayIconData, TrayIconKind};
use crate::updater::{self, InstallChannel, UpdateCheckResult};

const APP_MUTEX_NAME: &str = "Global\\ClaudeCodeUsageBubble";
const STARTUP_REGISTRY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const STARTUP_VALUE_NAME: &str = "ClaudeCodeUsageBubble";
const APP_CLASS_NAME: &str = "ClaudeCodeUsageBubbleApp";
const UPDATE_CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;
const RETRY_BASE_MS: u32 = 30_000;

// ---------- Menu command IDs ----------

const IDM_REFRESH: u16 = 1;
const IDM_EXIT: u16 = 2;

const IDM_FREQ_1MIN: u16 = 10;
const IDM_FREQ_5MIN: u16 = 11;
const IDM_FREQ_15MIN: u16 = 12;
const IDM_FREQ_1HOUR: u16 = 13;

const IDM_MODEL_CLAUDE_CODE: u16 = 20;
const IDM_MODEL_CODEX: u16 = 21;

const IDM_START_WITH_WINDOWS: u16 = 30;
const IDM_RESET_POSITION: u16 = 31;
const IDM_VERSION_ACTION: u16 = 32;

const IDM_LANG_SYSTEM: u16 = 40;
const IDM_LANG_ENGLISH: u16 = 41;
const IDM_LANG_DUTCH: u16 = 42;
const IDM_LANG_SPANISH: u16 = 43;
const IDM_LANG_FRENCH: u16 = 44;
const IDM_LANG_GERMAN: u16 = 45;
const IDM_LANG_JAPANESE: u16 = 46;
const IDM_LANG_KOREAN: u16 = 47;
const IDM_LANG_TRADITIONAL_CHINESE: u16 = 48;

// ---------- State ----------

#[derive(Clone, Copy, Default)]
struct SendHwnd(isize);
unsafe impl Send for SendHwnd {}
impl SendHwnd {
    fn from_hwnd(h: HWND) -> Self {
        Self(h.0 as isize)
    }
    fn to_hwnd(self) -> HWND {
        HWND(self.0 as *mut _)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    Available,
    Applying,
    Failed,
}

struct AppState {
    msg_hwnd: SendHwnd,
    bubbles: HashMap<TrayIconKindKey, SendHwnd>,
    settings: Settings,
    language: LanguageId,
    is_dark: bool,
    install_channel: InstallChannel,
    last_poll_ok: bool,
    retry_count: u32,
    session_text: String,
    weekly_text: String,
    codex_session_text: String,
    codex_weekly_text: String,
    session_percent: f64,
    weekly_percent: f64,
    codex_session_percent: f64,
    codex_weekly_percent: f64,
    data: AppUsageData,
    update_status: UpdateStatus,
    update_release: Option<updater::ReleaseDescriptor>,
    last_balloon_shown_at: Option<Instant>,
    auth_watch_mode: poller::CredentialWatchMode,
    auth_watch_snapshot: poller::CredentialWatchSnapshot,
    auth_error_paused_polling: bool,
}

#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug)]
enum TrayIconKindKey {
    Claude,
    Codex,
}
impl From<TrayIconKind> for TrayIconKindKey {
    fn from(k: TrayIconKind) -> Self {
        match k {
            TrayIconKind::Claude => TrayIconKindKey::Claude,
            TrayIconKind::Codex => TrayIconKindKey::Codex,
        }
    }
}
impl From<TrayIconKindKey> for TrayIconKind {
    fn from(k: TrayIconKindKey) -> Self {
        match k {
            TrayIconKindKey::Claude => TrayIconKind::Claude,
            TrayIconKindKey::Codex => TrayIconKind::Codex,
        }
    }
}

fn state() -> &'static Mutex<Option<AppState>> {
    static S: OnceLock<Mutex<Option<AppState>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(None))
}

fn lock_state() -> MutexGuard<'static, Option<AppState>> {
    state().lock().expect("app state mutex poisoned")
}

// ---------- Entry ----------

pub fn run() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    // Single-instance guard.
    let mutex_name = wide_str(APP_MUTEX_NAME);
    let _mutex = unsafe {
        let handle = CreateMutexW(None, false, PCWSTR::from_raw(mutex_name.as_ptr()));
        match handle {
            Ok(h) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    diagnose::log("startup aborted: another instance is already running");
                    return;
                }
                h
            }
            Err(e) => {
                diagnose::log_error("startup aborted: unable to create single-instance mutex", e);
                return;
            }
        }
    };

    let settings = settings::load();
    let language = localization::resolve_language(
        settings.language.as_deref().and_then(LanguageId::from_code),
    );
    let is_dark = theme::is_dark_mode();
    let install_channel = updater::current_install_channel();

    let msg_hwnd = match create_message_window() {
        Some(h) => h,
        None => {
            diagnose::log("startup aborted: unable to create app message window");
            return;
        }
    };

    *lock_state() = Some(AppState {
        msg_hwnd: SendHwnd::from_hwnd(msg_hwnd),
        bubbles: HashMap::new(),
        settings,
        language,
        is_dark,
        install_channel,
        last_poll_ok: false,
        retry_count: 0,
        session_text: String::new(),
        weekly_text: String::new(),
        codex_session_text: String::new(),
        codex_weekly_text: String::new(),
        session_percent: 0.0,
        weekly_percent: 0.0,
        codex_session_percent: 0.0,
        codex_weekly_percent: 0.0,
        data: AppUsageData::default(),
        update_status: UpdateStatus::Idle,
        update_release: None,
        last_balloon_shown_at: None,
        auth_watch_mode: poller::CredentialWatchMode::ActiveSource,
        auth_watch_snapshot: Vec::new(),
        auth_error_paused_polling: false,
    });

    create_initial_bubbles();
    refresh_tray_icons();

    // Timers
    unsafe {
        let interval = current_poll_interval_ms();
        SetTimer(msg_hwnd, TIMER_POLL, interval, None);
    }
    schedule_update_check_timer(msg_hwnd);
    spawn_poll_thread();

    diagnose::log("app::run entered message loop");
    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

// ---------- Window creation ----------

fn create_message_window() -> Option<HWND> {
    unsafe {
        let class_w = wide_str(APP_CLASS_NAME);
        let hinstance = GetModuleHandleW(PCWSTR::null()).unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(msg_wnd_proc),
            hInstance: HINSTANCE(hinstance.0),
            lpszClassName: PCWSTR::from_raw(class_w.as_ptr()),
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);
        let title_w = wide_str("Claude Code Usage Bubble");
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            PCWSTR::from_raw(class_w.as_ptr()),
            PCWSTR::from_raw(title_w.as_ptr()),
            WS_POPUP,
            -1000,
            -1000,
            1,
            1,
            HWND::default(),
            HMENU::default(),
            hinstance,
            None,
        )
        .ok()?;
        Some(hwnd)
    }
}

fn create_initial_bubbles() {
    let (settings, is_dark) = {
        let s = lock_state();
        let Some(s) = s.as_ref() else {
            return;
        };
        (s.settings.clone(), s.is_dark)
    };

    let mut to_create: Vec<(TrayIconKind, Option<(i32, i32)>)> = Vec::new();
    if settings.show_claude_code {
        to_create.push((TrayIconKind::Claude, settings.bubble_positions.get(TrayIconKind::Claude)));
    }
    if settings.show_codex {
        to_create.push((TrayIconKind::Codex, settings.bubble_positions.get(TrayIconKind::Codex)));
    }

    for (model, pos) in to_create {
        let hwnd = bubble::create(bubble::BubbleConfig {
            model,
            size_logical: settings.bubble_size_logical,
            position: pos,
            percent: None,
            is_dark,
        });
        if hwnd != HWND::default() {
            let mut state = lock_state();
            if let Some(s) = state.as_mut() {
                s.bubbles.insert(model.into(), SendHwnd::from_hwnd(hwnd));
            }
        }
    }
}

// ---------- Message-only window proc ----------

unsafe extern "system" fn msg_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP_USAGE_UPDATED => {
            apply_usage_update();
            LRESULT(0)
        }
        WM_APP_TRAY => {
            let action = tray_icon::handle_message(lparam);
            handle_tray_action(action);
            LRESULT(0)
        }
        WM_TIMER => {
            on_timer(hwnd, wparam.0);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ---------- Bubble UI callbacks (called from bubble::wnd_proc) ----------

pub fn on_bubble_click(hwnd: HWND, model: TrayIconKind) {
    // Toggle expanded panel for this model.
    let data = build_panel_data(model);
    panel::toggle(data, hwnd);
}

pub fn on_bubble_right_click(hwnd: HWND, _model: TrayIconKind, _pt: POINT) {
    show_context_menu(hwnd);
}

pub fn on_bubble_moved(model: TrayIconKind, pos: (i32, i32)) {
    let snap = {
        let mut state = lock_state();
        let Some(s) = state.as_mut() else {
            return;
        };
        s.settings.bubble_positions.set(model, pos);
        s.settings.clone()
    };
    settings::save(&snap);
}

pub fn on_bubble_resized(_model: TrayIconKind, size_logical: i32) {
    let snap = {
        let mut state = lock_state();
        let Some(s) = state.as_mut() else {
            return;
        };
        s.settings.bubble_size_logical = size_logical;
        s.settings.clone()
    };
    settings::save(&snap);
}

pub fn on_menu_command(id: u32, owner_hwnd: HWND) {
    let id = (id & 0xFFFF) as u16;
    match id {
        IDM_REFRESH => spawn_poll_thread(),
        IDM_EXIT => unsafe {
            PostQuitMessage(0);
        },
        IDM_FREQ_1MIN => set_poll_interval(POLL_1_MIN),
        IDM_FREQ_5MIN => set_poll_interval(POLL_5_MIN),
        IDM_FREQ_15MIN => set_poll_interval(POLL_15_MIN),
        IDM_FREQ_1HOUR => set_poll_interval(POLL_1_HOUR),
        IDM_MODEL_CLAUDE_CODE => toggle_model(TrayIconKind::Claude),
        IDM_MODEL_CODEX => toggle_model(TrayIconKind::Codex),
        IDM_START_WITH_WINDOWS => toggle_startup(),
        IDM_RESET_POSITION => reset_positions(),
        IDM_VERSION_ACTION => version_action(owner_hwnd),
        IDM_LANG_SYSTEM => set_language(None),
        IDM_LANG_ENGLISH => set_language(Some(LanguageId::English)),
        IDM_LANG_DUTCH => set_language(Some(LanguageId::Dutch)),
        IDM_LANG_SPANISH => set_language(Some(LanguageId::Spanish)),
        IDM_LANG_FRENCH => set_language(Some(LanguageId::French)),
        IDM_LANG_GERMAN => set_language(Some(LanguageId::German)),
        IDM_LANG_JAPANESE => set_language(Some(LanguageId::Japanese)),
        IDM_LANG_KOREAN => set_language(Some(LanguageId::Korean)),
        IDM_LANG_TRADITIONAL_CHINESE => set_language(Some(LanguageId::TraditionalChinese)),
        tray_icon::IDM_TOGGLE_WIDGET => toggle_widget_visibility(),
        _ => {}
    }
}

// ---------- Timer dispatch ----------

fn on_timer(hwnd: HWND, id: usize) {
    match id {
        TIMER_POLL => spawn_poll_thread(),
        TIMER_RESET_POLL => spawn_poll_thread(),
        TIMER_COUNTDOWN => refresh_countdowns(),
        TIMER_UPDATE_CHECK => {
            unsafe {
                let _ = KillTimer(hwnd, TIMER_UPDATE_CHECK);
            }
            begin_update_check(hwnd, false);
        }
        _ => {}
    }
}

// ---------- Poll thread / data application ----------

fn spawn_poll_thread() {
    let (show_claude, show_codex, msg_hwnd) = {
        let state = lock_state();
        let Some(s) = state.as_ref() else {
            return;
        };
        (
            s.settings.show_claude_code,
            s.settings.show_codex,
            s.msg_hwnd,
        )
    };
    std::thread::spawn(move || {
        let result = poller::poll(show_claude, show_codex);
        handle_poll_result(result, msg_hwnd);
    });
}

fn handle_poll_result(result: Result<AppUsageData, PollError>, msg_hwnd: SendHwnd) {
    match result {
        Ok(data) => {
            {
                let mut state = lock_state();
                if let Some(s) = state.as_mut() {
                    apply_data(s, data);
                    s.last_poll_ok = true;
                    s.retry_count = 0;
                    s.auth_error_paused_polling = false;
                    s.auth_watch_mode = poller::CredentialWatchMode::ActiveSource;
                    s.auth_watch_snapshot.clear();
                }
            }
            unsafe {
                let _ = PostMessageW(
                    msg_hwnd.to_hwnd(),
                    WM_APP_USAGE_UPDATED,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
        Err(error) => {
            let auth_problem = matches!(
                error,
                PollError::AuthRequired | PollError::TokenExpired | PollError::NoCredentials
            );
            {
                let mut state = lock_state();
                if let Some(s) = state.as_mut() {
                    s.last_poll_ok = false;
                    s.retry_count = s.retry_count.saturating_add(1);
                    if auth_problem {
                        let mode = if matches!(error, PollError::NoCredentials) {
                            poller::CredentialWatchMode::AllSources
                        } else {
                            poller::CredentialWatchMode::ActiveSource
                        };
                        s.auth_watch_mode = mode;
                        s.auth_watch_snapshot = poller::credential_watch_snapshot(mode);
                        s.auth_error_paused_polling = true;
                        s.session_text = "!".into();
                        s.weekly_text = "!".into();
                        s.codex_session_text = "!".into();
                        s.codex_weekly_text = "!".into();
                    } else {
                        s.session_text = "...".into();
                        s.weekly_text = "...".into();
                        s.codex_session_text = "...".into();
                        s.codex_weekly_text = "...".into();
                    }
                }
            }
            unsafe {
                let _ = PostMessageW(
                    msg_hwnd.to_hwnd(),
                    WM_APP_USAGE_UPDATED,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
            if auth_problem {
                show_token_expired_balloon();
            }
        }
    }
}

fn apply_data(s: &mut AppState, data: AppUsageData) {
    if let Some(c) = data.claude_code.as_ref() {
        s.session_percent = c.session.percentage;
        s.weekly_percent = c.weekly.percentage;
    } else if s.settings.show_claude_code {
        s.session_percent = 0.0;
        s.weekly_percent = 0.0;
    }
    if let Some(c) = data.codex.as_ref() {
        s.codex_session_percent = c.session.percentage;
        s.codex_weekly_percent = c.weekly.percentage;
    } else if s.settings.show_codex {
        s.codex_session_percent = 0.0;
        s.codex_weekly_percent = 0.0;
    }
    s.data = data;
    refresh_text_fields(s);
}

fn refresh_text_fields(s: &mut AppState) {
    let strings = s.language.strings();
    if let Some(c) = s.data.claude_code.as_ref() {
        s.session_text = poller::format_line(&c.session, strings);
        s.weekly_text = poller::format_line(&c.weekly, strings);
    }
    if let Some(c) = s.data.codex.as_ref() {
        s.codex_session_text = poller::format_line(&c.session, strings);
        s.codex_weekly_text = poller::format_line(&c.weekly, strings);
    }
}

fn refresh_countdowns() {
    {
        let mut state = lock_state();
        if let Some(s) = state.as_mut() {
            refresh_text_fields(s);
        }
    }
    apply_usage_update();
}

fn apply_usage_update() {
    let snapshot = {
        let s = lock_state();
        s.as_ref().map(|s| UsageSnapshot {
            bubbles: s.bubbles.clone(),
            session_percent: s.session_percent,
            weekly_percent: s.weekly_percent,
            codex_session_percent: s.codex_session_percent,
            codex_weekly_percent: s.codex_weekly_percent,
            session_text: s.session_text.clone(),
            weekly_text: s.weekly_text.clone(),
            codex_session_text: s.codex_session_text.clone(),
            codex_weekly_text: s.codex_weekly_text.clone(),
            language: s.language,
            is_dark: s.is_dark,
            settings: s.settings.clone(),
        })
    };
    let Some(snap) = snapshot else {
        return;
    };

    for (kind, hwnd) in snap.bubbles.iter() {
        let model = TrayIconKind::from(*kind);
        let pct = match model {
            TrayIconKind::Claude => Some(snap.session_percent),
            TrayIconKind::Codex => Some(snap.codex_session_percent),
        };
        bubble::update_percentage(hwnd.to_hwnd(), pct);
    }

    refresh_tray_icons();

    // Refresh expanded panel if showing.
    if panel::is_visible() {
        if let Some(model) = panel::current_model() {
            panel::refresh_data(build_panel_data_from(&snap, model));
        }
    }

    // Adaptive countdown timer.
    schedule_countdown_timer();
}

#[derive(Clone)]
struct UsageSnapshot {
    bubbles: HashMap<TrayIconKindKey, SendHwnd>,
    session_percent: f64,
    weekly_percent: f64,
    codex_session_percent: f64,
    codex_weekly_percent: f64,
    session_text: String,
    weekly_text: String,
    codex_session_text: String,
    codex_weekly_text: String,
    language: LanguageId,
    is_dark: bool,
    settings: Settings,
}

fn build_panel_data(model: TrayIconKind) -> PanelData {
    let snap = {
        let s = lock_state();
        s.as_ref().map(|s| UsageSnapshot {
            bubbles: s.bubbles.clone(),
            session_percent: s.session_percent,
            weekly_percent: s.weekly_percent,
            codex_session_percent: s.codex_session_percent,
            codex_weekly_percent: s.codex_weekly_percent,
            session_text: s.session_text.clone(),
            weekly_text: s.weekly_text.clone(),
            codex_session_text: s.codex_session_text.clone(),
            codex_weekly_text: s.codex_weekly_text.clone(),
            language: s.language,
            is_dark: s.is_dark,
            settings: s.settings.clone(),
        })
    }
    .unwrap_or_else(|| UsageSnapshot {
        bubbles: HashMap::new(),
        session_percent: 0.0,
        weekly_percent: 0.0,
        codex_session_percent: 0.0,
        codex_weekly_percent: 0.0,
        session_text: String::new(),
        weekly_text: String::new(),
        codex_session_text: String::new(),
        codex_weekly_text: String::new(),
        language: LanguageId::English,
        is_dark: false,
        settings: Settings::default(),
    });
    build_panel_data_from(&snap, model)
}

fn build_panel_data_from(snap: &UsageSnapshot, model: TrayIconKind) -> PanelData {
    let (sp, st, wp, wt) = match model {
        TrayIconKind::Claude => (
            snap.session_percent,
            snap.session_text.clone(),
            snap.weekly_percent,
            snap.weekly_text.clone(),
        ),
        TrayIconKind::Codex => (
            snap.codex_session_percent,
            snap.codex_session_text.clone(),
            snap.codex_weekly_percent,
            snap.codex_weekly_text.clone(),
        ),
    };
    let strings = snap.language.strings();
    PanelData {
        model,
        session_pct: sp,
        session_text: st,
        weekly_pct: wp,
        weekly_text: wt,
        is_dark: snap.is_dark,
        strings,
        claude_label: strings.claude_code_model.to_string(),
        codex_label: strings.codex_model.to_string(),
    }
}

fn schedule_countdown_timer() {
    let (msg_hwnd, ttl) = {
        let s = lock_state();
        let Some(s) = s.as_ref() else {
            return;
        };
        let mut min_ttl: Option<Duration> = None;
        if let Some(c) = s.data.claude_code.as_ref() {
            for section in [&c.session, &c.weekly] {
                if let Some(d) = poller::time_until_display_change(section.resets_at) {
                    min_ttl = Some(match min_ttl {
                        Some(prev) => prev.min(d),
                        None => d,
                    });
                }
            }
        }
        if let Some(c) = s.data.codex.as_ref() {
            for section in [&c.session, &c.weekly] {
                if let Some(d) = poller::time_until_display_change(section.resets_at) {
                    min_ttl = Some(match min_ttl {
                        Some(prev) => prev.min(d),
                        None => d,
                    });
                }
            }
        }
        (s.msg_hwnd, min_ttl)
    };
    if let Some(d) = ttl {
        let ms = (d.as_millis() as u64).min(u32::MAX as u64) as u32;
        unsafe {
            let _ = KillTimer(msg_hwnd.to_hwnd(), TIMER_COUNTDOWN);
            SetTimer(msg_hwnd.to_hwnd(), TIMER_COUNTDOWN, ms.max(1000), None);
        }
    }
}

// ---------- Tray icons ----------

fn refresh_tray_icons() {
    let (icons, msg_hwnd) = {
        let s = lock_state();
        let Some(s) = s.as_ref() else {
            return;
        };
        let strings = s.language.strings();
        let mut icons = Vec::new();
        if s.settings.show_claude_code {
            icons.push(TrayIconData {
                kind: TrayIconKind::Claude,
                percent: if s.last_poll_ok {
                    Some(s.session_percent)
                } else {
                    None
                },
                tooltip: format!(
                    "{} 5h: {} | 7d: {}",
                    strings.claude_code_model, s.session_text, s.weekly_text
                ),
            });
        }
        if s.settings.show_codex {
            icons.push(TrayIconData {
                kind: TrayIconKind::Codex,
                percent: if s.last_poll_ok {
                    Some(s.codex_session_percent)
                } else {
                    None
                },
                tooltip: format!(
                    "{} 5h: {} | 7d: {}",
                    strings.codex_model, s.codex_session_text, s.codex_weekly_text
                ),
            });
        }
        (icons, s.msg_hwnd)
    };
    tray_icon::sync(msg_hwnd.to_hwnd(), &icons);
}

fn handle_tray_action(action: TrayAction) {
    match action {
        TrayAction::None => {}
        TrayAction::ToggleWidget => toggle_widget_visibility(),
        TrayAction::ShowContextMenu => {
            // Use the first bubble as menu owner; fall back to msg_hwnd.
            let owner = {
                let s = lock_state();
                s.as_ref()
                    .and_then(|s| s.bubbles.values().next().copied())
                    .map(|h| h.to_hwnd())
                    .unwrap_or_default()
            };
            if owner != HWND::default() {
                show_context_menu(owner);
            }
        }
    }
}

fn show_token_expired_balloon() {
    let payload: Option<(SendHwnd, TrayIconKind, String, String)> = {
        let mut state = lock_state();
        let Some(s) = state.as_mut() else {
            return;
        };
        if let Some(last) = s.last_balloon_shown_at {
            if last.elapsed() < Duration::from_secs(30 * 60) {
                return;
            }
        }
        s.last_balloon_shown_at = Some(Instant::now());
        let strings = s.language.strings();
        if s.settings.show_claude_code {
            Some((
                s.msg_hwnd,
                TrayIconKind::Claude,
                strings.token_expired_title.to_string(),
                strings.token_expired_body.to_string(),
            ))
        } else {
            Some((
                s.msg_hwnd,
                TrayIconKind::Codex,
                strings.codex_token_expired_title.to_string(),
                strings.codex_token_expired_body.to_string(),
            ))
        }
    };
    if let Some((hwnd, kind, title, body)) = payload {
        tray_icon::notify_balloon(hwnd.to_hwnd(), kind, &title, &body);
    }
}

// ---------- Context menu ----------

fn show_context_menu(owner_hwnd: HWND) {
    let (strings, language, install_channel, update_status, current_interval, show_claude, show_codex, widget_visible, language_override) = {
        let s = lock_state();
        let Some(s) = s.as_ref() else {
            return;
        };
        (
            s.language.strings(),
            s.language,
            s.install_channel,
            s.update_status,
            s.settings.poll_interval_ms,
            s.settings.show_claude_code,
            s.settings.show_codex,
            s.settings.widget_visible,
            s.settings.language.as_deref().and_then(LanguageId::from_code),
        )
    };

    unsafe {
        let menu = match CreatePopupMenu() {
            Ok(m) => m,
            Err(_) => return,
        };

        append_menu_item(menu, IDM_REFRESH, strings.refresh, MENU_ITEM_FLAGS(0));

        // Update frequency submenu
        let freq_menu = CreatePopupMenu().unwrap();
        for (id, interval, label) in [
            (IDM_FREQ_1MIN, POLL_1_MIN, strings.one_minute),
            (IDM_FREQ_5MIN, POLL_5_MIN, strings.five_minutes),
            (IDM_FREQ_15MIN, POLL_15_MIN, strings.fifteen_minutes),
            (IDM_FREQ_1HOUR, POLL_1_HOUR, strings.one_hour),
        ] {
            let flags = if interval == current_interval {
                MF_CHECKED
            } else {
                MENU_ITEM_FLAGS(0)
            };
            append_menu_item(freq_menu, id, label, flags);
        }
        append_submenu(menu, freq_menu, strings.update_frequency);

        // Models submenu
        let models_menu = CreatePopupMenu().unwrap();
        append_menu_item(
            models_menu,
            IDM_MODEL_CLAUDE_CODE,
            strings.claude_code_model,
            if show_claude {
                MF_CHECKED
            } else {
                MENU_ITEM_FLAGS(0)
            },
        );
        append_menu_item(
            models_menu,
            IDM_MODEL_CODEX,
            strings.codex_model,
            if show_codex {
                MF_CHECKED
            } else {
                MENU_ITEM_FLAGS(0)
            },
        );
        append_submenu(menu, models_menu, strings.models);

        // Settings submenu
        let settings_menu = CreatePopupMenu().unwrap();
        append_menu_item(
            settings_menu,
            IDM_START_WITH_WINDOWS,
            strings.start_with_windows,
            if is_startup_enabled() {
                MF_CHECKED
            } else {
                MENU_ITEM_FLAGS(0)
            },
        );
        append_menu_item(
            settings_menu,
            IDM_RESET_POSITION,
            strings.reset_position,
            MENU_ITEM_FLAGS(0),
        );

        // Language submenu
        let lang_menu = CreatePopupMenu().unwrap();
        append_menu_item(
            lang_menu,
            IDM_LANG_SYSTEM,
            strings.system_default,
            if language_override.is_none() {
                MF_CHECKED
            } else {
                MENU_ITEM_FLAGS(0)
            },
        );
        for lang in LanguageId::ALL {
            let id = lang_menu_id_for(lang);
            let label = lang.native_name();
            let flags = if language_override == Some(lang) {
                MF_CHECKED
            } else {
                MENU_ITEM_FLAGS(0)
            };
            append_menu_item(lang_menu, id, label, flags);
        }
        append_submenu(settings_menu, lang_menu, strings.language);

        let _ = AppendMenuW(settings_menu, MF_SEPARATOR, 0, PCWSTR::null());

        let version_label = version_action_label(strings, language, install_channel, update_status);
        let version_flags = if matches!(update_status, UpdateStatus::Checking | UpdateStatus::Applying) {
            MF_GRAYED
        } else {
            MENU_ITEM_FLAGS(0)
        };
        append_menu_item(
            settings_menu,
            IDM_VERSION_ACTION,
            &version_label,
            version_flags,
        );

        append_submenu(menu, settings_menu, strings.settings);

        append_menu_item(
            menu,
            tray_icon::IDM_TOGGLE_WIDGET,
            strings.show_widget,
            if widget_visible {
                MF_CHECKED
            } else {
                MENU_ITEM_FLAGS(0)
            },
        );

        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        append_menu_item(menu, IDM_EXIT, strings.exit, MENU_ITEM_FLAGS(0));

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(owner_hwnd);
        let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON, pt.x, pt.y, 0, owner_hwnd, None);
        let _ = DestroyMenu(menu);
    }
}

fn append_menu_item(menu: HMENU, id: u16, label: &str, flags: MENU_ITEM_FLAGS) {
    let label_w = wide_str(label);
    unsafe {
        let _ = AppendMenuW(menu, flags, id as usize, PCWSTR::from_raw(label_w.as_ptr()));
    }
}

fn append_submenu(menu: HMENU, submenu: HMENU, label: &str) {
    let label_w = wide_str(label);
    unsafe {
        let _ = AppendMenuW(
            menu,
            MF_POPUP,
            submenu.0 as usize,
            PCWSTR::from_raw(label_w.as_ptr()),
        );
    }
}

fn lang_menu_id_for(lang: LanguageId) -> u16 {
    match lang {
        LanguageId::English => IDM_LANG_ENGLISH,
        LanguageId::Dutch => IDM_LANG_DUTCH,
        LanguageId::Spanish => IDM_LANG_SPANISH,
        LanguageId::French => IDM_LANG_FRENCH,
        LanguageId::German => IDM_LANG_GERMAN,
        LanguageId::Japanese => IDM_LANG_JAPANESE,
        LanguageId::Korean => IDM_LANG_KOREAN,
        LanguageId::TraditionalChinese => IDM_LANG_TRADITIONAL_CHINESE,
    }
}

fn version_action_label(
    strings: Strings,
    language: LanguageId,
    install_channel: InstallChannel,
    status: UpdateStatus,
) -> String {
    let base = match status {
        UpdateStatus::Idle => strings.check_for_updates.to_string(),
        UpdateStatus::Checking => strings.checking_for_updates.to_string(),
        UpdateStatus::UpToDate => strings.up_to_date.to_string(),
        UpdateStatus::Available => strings.update_available.to_string(),
        UpdateStatus::Applying => strings.applying_update.to_string(),
        UpdateStatus::Failed => strings.update_failed.to_string(),
    };
    match install_channel {
        InstallChannel::Winget => format!("{base} ({})", localization::update_via_winget(language)),
        InstallChannel::Portable => base,
    }
}

// ---------- Menu actions ----------

fn set_poll_interval(ms: u32) {
    let (snap, msg_hwnd) = {
        let mut state = lock_state();
        let Some(s) = state.as_mut() else {
            return;
        };
        s.settings.poll_interval_ms = ms;
        (s.settings.clone(), s.msg_hwnd)
    };
    settings::save(&snap);
    unsafe {
        let _ = KillTimer(msg_hwnd.to_hwnd(), TIMER_POLL);
        SetTimer(msg_hwnd.to_hwnd(), TIMER_POLL, ms, None);
    }
}

fn current_poll_interval_ms() -> u32 {
    lock_state()
        .as_ref()
        .map(|s| s.settings.poll_interval_ms)
        .unwrap_or(POLL_5_MIN)
}

fn toggle_model(model: TrayIconKind) {
    let (settings, is_dark) = {
        let mut state = lock_state();
        let Some(s) = state.as_mut() else {
            return;
        };
        match model {
            TrayIconKind::Claude => s.settings.show_claude_code = !s.settings.show_claude_code,
            TrayIconKind::Codex => s.settings.show_codex = !s.settings.show_codex,
        }
        if !s.settings.show_claude_code && !s.settings.show_codex {
            // Don't let user turn both off.
            match model {
                TrayIconKind::Claude => s.settings.show_claude_code = true,
                TrayIconKind::Codex => s.settings.show_codex = true,
            }
        }
        (s.settings.clone(), s.is_dark)
    };
    settings::save(&settings);

    let want_show = match model {
        TrayIconKind::Claude => settings.show_claude_code,
        TrayIconKind::Codex => settings.show_codex,
    };
    let existing = {
        let s = lock_state();
        s.as_ref()
            .and_then(|s| s.bubbles.get(&model.into()).copied())
    };

    match (want_show, existing) {
        (true, None) => {
            let hwnd = bubble::create(bubble::BubbleConfig {
                model,
                size_logical: settings.bubble_size_logical,
                position: settings.bubble_positions.get(model),
                percent: None,
                is_dark,
            });
            if hwnd != HWND::default() {
                let mut state = lock_state();
                if let Some(s) = state.as_mut() {
                    s.bubbles.insert(model.into(), SendHwnd::from_hwnd(hwnd));
                }
            }
        }
        (false, Some(hwnd)) => {
            bubble::destroy(hwnd.to_hwnd());
            let mut state = lock_state();
            if let Some(s) = state.as_mut() {
                s.bubbles.remove(&model.into());
            }
        }
        _ => {}
    }
    refresh_tray_icons();
    spawn_poll_thread();
}

fn toggle_widget_visibility() {
    let (new_visible, snap) = {
        let mut state = lock_state();
        let Some(s) = state.as_mut() else {
            return;
        };
        s.settings.widget_visible = !s.settings.widget_visible;
        (s.settings.widget_visible, s.settings.clone())
    };
    settings::save(&snap);
    let hwnds: Vec<HWND> = {
        let state = lock_state();
        state
            .as_ref()
            .map(|s| s.bubbles.values().map(|h| h.to_hwnd()).collect())
            .unwrap_or_default()
    };
    for h in hwnds {
        bubble::set_user_visible(h, new_visible);
    }
}

fn reset_positions() {
    let snap = {
        let mut state = lock_state();
        let Some(s) = state.as_mut() else {
            return;
        };
        s.settings.bubble_positions.reset_all();
        s.settings.clone()
    };
    settings::save(&snap);
    let hwnds: Vec<HWND> = {
        let state = lock_state();
        state
            .as_ref()
            .map(|s| s.bubbles.values().map(|h| h.to_hwnd()).collect())
            .unwrap_or_default()
    };
    for h in hwnds {
        bubble::destroy(h);
    }
    {
        let mut state = lock_state();
        if let Some(s) = state.as_mut() {
            s.bubbles.clear();
        }
    }
    create_initial_bubbles();
}

fn set_language(override_lang: Option<LanguageId>) {
    let snap = {
        let mut state = lock_state();
        let Some(s) = state.as_mut() else {
            return;
        };
        s.language = match override_lang {
            Some(l) => l,
            None => localization::detect_system_language(),
        };
        s.settings.language = override_lang.map(|l| l.code().to_string());
        refresh_text_fields(s);
        s.settings.clone()
    };
    settings::save(&snap);
    apply_usage_update();
}

fn version_action(_owner_hwnd: HWND) {
    enum Act {
        Apply(updater::ReleaseDescriptor, InstallChannel),
        Check(SendHwnd),
    }
    let act = {
        let s = lock_state();
        let Some(s) = s.as_ref() else {
            return;
        };
        match (s.update_status, s.update_release.as_ref()) {
            (UpdateStatus::Available, Some(release)) => {
                Act::Apply(release.clone(), s.install_channel)
            }
            _ => Act::Check(s.msg_hwnd),
        }
    };
    match act {
        Act::Apply(release, channel) => {
            {
                let mut state = lock_state();
                if let Some(s) = state.as_mut() {
                    s.update_status = UpdateStatus::Applying;
                }
            }
            let result = match channel {
                InstallChannel::Winget => updater::begin_winget_update(),
                InstallChannel::Portable => updater::begin_self_update(&release),
            };
            match result {
                Ok(()) => unsafe {
                    PostQuitMessage(0);
                },
                Err(error) => {
                    diagnose::log(format!("update apply failed: {error}"));
                    let mut state = lock_state();
                    if let Some(s) = state.as_mut() {
                        s.update_status = UpdateStatus::Failed;
                    }
                }
            }
        }
        Act::Check(hwnd) => {
            begin_update_check(hwnd.to_hwnd(), true);
        }
    }
}

// ---------- Updates ----------

fn schedule_update_check_timer(hwnd: HWND) {
    let last = {
        let s = lock_state();
        s.as_ref().and_then(|s| s.settings.last_update_check_unix)
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let due = last.map_or(true, |t| now.saturating_sub(t) >= UPDATE_CHECK_INTERVAL_SECS);
    if due {
        begin_update_check(hwnd, false);
    } else {
        let remaining = UPDATE_CHECK_INTERVAL_SECS.saturating_sub(now.saturating_sub(last.unwrap_or(0)));
        let ms = (remaining.saturating_mul(1000)).min(u32::MAX as u64) as u32;
        unsafe {
            SetTimer(hwnd, TIMER_UPDATE_CHECK, ms, None);
        }
    }
}

fn begin_update_check(hwnd: HWND, _user_initiated: bool) {
    {
        let mut state = lock_state();
        if let Some(s) = state.as_mut() {
            s.update_status = UpdateStatus::Checking;
        }
    }
    let send_hwnd = SendHwnd::from_hwnd(hwnd);
    std::thread::spawn(move || {
        let result = updater::check_for_updates();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let snap_opt: Option<Settings> = {
            let mut state = lock_state();
            state.as_mut().map(|s| {
                s.settings.last_update_check_unix = Some(now);
                match result {
                    Ok(UpdateCheckResult::UpToDate) => {
                        s.update_status = UpdateStatus::UpToDate;
                        s.update_release = None;
                    }
                    Ok(UpdateCheckResult::Available(release)) => {
                        s.update_status = UpdateStatus::Available;
                        s.update_release = Some(release);
                    }
                    Err(_) => {
                        s.update_status = UpdateStatus::Failed;
                    }
                }
                s.settings.clone()
            })
        };
        if let Some(snap) = snap_opt {
            settings::save(&snap);
        }
        unsafe {
            SetTimer(
                send_hwnd.to_hwnd(),
                TIMER_UPDATE_CHECK,
                (UPDATE_CHECK_INTERVAL_SECS as u32) * 1000,
                None,
            );
        }
    });
}

// ---------- Start-with-Windows registry ----------

fn is_startup_enabled() -> bool {
    let path_w = wide_str(STARTUP_REGISTRY_PATH);
    let name_w = wide_str(STARTUP_VALUE_NAME);
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR::from_raw(path_w.as_ptr()),
            0,
            KEY_READ,
            &mut hkey,
        )
        .is_err()
        {
            return false;
        }
        let mut buf = [0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        let res = RegQueryValueExW(
            hkey,
            PCWSTR::from_raw(name_w.as_ptr()),
            None,
            None,
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut size),
        );
        let _ = RegCloseKey(hkey);
        res.is_ok()
    }
}

fn toggle_startup() {
    let enabled = is_startup_enabled();
    let path_w = wide_str(STARTUP_REGISTRY_PATH);
    let name_w = wide_str(STARTUP_VALUE_NAME);
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR::from_raw(path_w.as_ptr()),
            0,
            KEY_WRITE,
            &mut hkey,
        )
        .is_err()
        {
            return;
        }
        if enabled {
            let _ = RegDeleteValueW(hkey, PCWSTR::from_raw(name_w.as_ptr()));
        } else {
            if let Ok(exe) = std::env::current_exe() {
                let exe_str = format!("\"{}\"", exe.to_string_lossy());
                let exe_w = wide_str(&exe_str);
                let bytes = std::slice::from_raw_parts(
                    exe_w.as_ptr() as *const u8,
                    exe_w.len() * 2,
                );
                let _ = RegSetValueExW(
                    hkey,
                    PCWSTR::from_raw(name_w.as_ptr()),
                    0,
                    REG_SZ,
                    Some(bytes),
                );
            }
        }
        let _ = RegCloseKey(hkey);
    }
}

