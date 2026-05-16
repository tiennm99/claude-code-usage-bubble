// App orchestrator.
//
// One `Mutex<Option<AppState>>` is the single source of truth. The UI
// thread runs the message loop; a background thread polls the provider
// registry and posts `WM_APP_USAGE_UPDATED` back via a hidden
// message-only window owned by this module.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::core::PCWSTR;
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::bubble;
use crate::i18n::{self, I18n, LocaleStrings};
// Win32 message + timer IDs moved inline; see constants below.
use crate::net;
use crate::os;
use crate::panel::{self, PanelData};
use crate::settings::{self, Settings, POLL_15_MIN, POLL_1_HOUR, POLL_1_MIN, POLL_5_MIN};
use crate::tray::{self, TrayAction, TrayIcon as TrayIconData};
use crate::usage::ProviderId as TrayIconKind;
use crate::tray::WM_APP_TRAY;
use crate::update::{self, Channel as InstallChannel, CheckOutcome};
use crate::usage::{self, ProviderId, Registry, UsageWindows};

// Win32 message IDs owned by this module.
pub const WM_APP_USAGE_UPDATED: u32 = 0x8001;

// Timer IDs used with `SetTimer(msg_hwnd, …)`.
const TIMER_POLL: usize = 1;
const TIMER_COUNTDOWN: usize = 2;
const TIMER_RESET_POLL: usize = 3;
const TIMER_UPDATE_CHECK: usize = 4;

const APP_MUTEX_NAME: &str = r"Global\ClaudeCodeUsageBubble";
const STARTUP_REGISTRY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const STARTUP_VALUE_NAME: &str = "ClaudeCodeUsageBubble";
const APP_CLASS_NAME: &str = "ClaudeCodeUsageBubbleApp";
const HTTP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
const UPDATE_CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;
const BALLOON_COOLDOWN: Duration = Duration::from_secs(30 * 60);
const REFRESH_TIMEOUT: Duration = Duration::from_secs(8);

// ---------- Menu IDs ----------

const IDM_REFRESH: u16 = 1;
const IDM_EXIT: u16 = 2;
const IDM_FREQ_1MIN: u16 = 10;
const IDM_FREQ_5MIN: u16 = 11;
const IDM_FREQ_15MIN: u16 = 12;
const IDM_FREQ_1HOUR: u16 = 13;
const IDM_MODEL_CLAUDE: u16 = 20;
const IDM_MODEL_CHATGPT: u16 = 21;
const IDM_START_WITH_WINDOWS: u16 = 30;
const IDM_RESET_POSITION: u16 = 31;
const IDM_VERSION_ACTION: u16 = 32;
const IDM_LANG_SYSTEM: u16 = 40;
const IDM_LANG_BASE: u16 = 41;

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
    bubbles: HashMap<TrayIconKind, SendHwnd>,
    settings: Settings,
    i18n: I18n,
    is_dark: bool,
    install_channel: InstallChannel,
    http: net::Client,
    registry: Registry,
    snapshots: HashMap<ProviderId, ProviderUiState>,
    last_poll_ok: bool,
    update_status: UpdateStatus,
    update_release: Option<update::Release>,
    last_balloon_at: Option<Instant>,
}

#[derive(Clone, Default)]
struct ProviderUiState {
    windows: UsageWindows,
    primary_text: String,
    secondary_text: String,
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

    let mutex_name_w = os::to_utf16_nul(APP_MUTEX_NAME);
    let _mutex = unsafe {
        let handle = CreateMutexW(None, false, PCWSTR::from_raw(mutex_name_w.as_ptr()));
        match handle {
            Ok(h) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    log::info!("another instance already running; exiting");
                    return;
                }
                h
            }
            Err(e) => {
                log::error!("CreateMutex failed: {e}");
                return;
            }
        }
    };

    let settings = settings::load();
    let i18n = I18n::load(settings.language.as_deref());
    let is_dark = os::theme::is_dark();
    let install_channel = update::current_channel();
    let http = match net::Client::new(HTTP_USER_AGENT) {
        Ok(c) => c,
        Err(e) => {
            log::error!("HTTP client init failed: {e}");
            return;
        }
    };
    let msg_hwnd = match create_message_window() {
        Some(h) => h,
        None => {
            log::error!("failed to create app message window");
            return;
        }
    };

    *lock_state() = Some(AppState {
        msg_hwnd: SendHwnd::from_hwnd(msg_hwnd),
        bubbles: HashMap::new(),
        settings,
        i18n,
        is_dark,
        install_channel,
        http,
        registry: Registry::with_defaults(),
        snapshots: HashMap::new(),
        last_poll_ok: false,
        update_status: UpdateStatus::Idle,
        update_release: None,
        last_balloon_at: None,
    });

    create_initial_bubbles();
    refresh_tray_icons();

    let poll_interval = lock_state()
        .as_ref()
        .map(|s| s.settings.poll_interval_ms)
        .unwrap_or(POLL_5_MIN);
    unsafe {
        SetTimer(msg_hwnd, TIMER_POLL, poll_interval, None);
    }
    schedule_update_check_timer(msg_hwnd);
    spawn_poll_thread();

    log::info!("app::run entered message loop");
    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn create_message_window() -> Option<HWND> {
    let class_w = os::to_utf16_nul(APP_CLASS_NAME);
    let title_w = os::to_utf16_nul("Claude Code Usage Bubble");
    unsafe {
        let hinstance = GetModuleHandleW(PCWSTR::null()).unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(msg_wnd_proc),
            hInstance: HINSTANCE(hinstance.0),
            lpszClassName: PCWSTR::from_raw(class_w.as_ptr()),
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);
        CreateWindowExW(
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
        .ok()
    }
}

fn create_initial_bubbles() {
    let (settings, is_dark) = match lock_state().as_ref() {
        Some(s) => (s.settings.clone(), s.is_dark),
        None => return,
    };
    if settings.show_claude_code {
        spawn_bubble(ProviderId::Claude, &settings, is_dark);
    }
    if settings.show_codex {
        spawn_bubble(ProviderId::ChatGpt, &settings, is_dark);
    }
}

fn spawn_bubble(kind: TrayIconKind, settings: &Settings, is_dark: bool) {
    // "…" matches the in-flight/transient-error placeholder used by
    // `apply_results`, so the bubble has visible feedback during the first
    // poll rather than rendering with two empty grey tracks.
    let placeholder = "…".to_string();
    let hwnd = bubble::create(bubble::BubbleConfig {
        model: kind,
        size_logical: settings.bubble_size_logical,
        position: settings.bubble_positions.get(kind),
        session_pct: None,
        session_text: placeholder.clone(),
        weekly_pct: None,
        weekly_text: placeholder,
        is_dark,
    });
    if hwnd != HWND::default() {
        if let Some(s) = lock_state().as_mut() {
            s.bubbles.insert(kind, SendHwnd::from_hwnd(hwnd));
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
            propagate_to_ui();
            LRESULT(0)
        }
        WM_APP_TRAY => {
            let action = tray::callback::handle(lparam);
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

// ---------- Bubble callbacks ----------

pub fn on_bubble_click(hwnd: HWND, model: TrayIconKind) {
    let data = build_panel_data(model);
    panel::toggle(data, hwnd);
}

pub fn on_bubble_right_click(hwnd: HWND, _model: TrayIconKind, _pt: POINT) {
    show_context_menu(hwnd);
}

pub fn on_bubble_moved(model: TrayIconKind, pos: (i32, i32)) {
    let snap = {
        let mut s = lock_state();
        let Some(s) = s.as_mut() else {
            return;
        };
        s.settings.bubble_positions.set(model, pos);
        s.settings.clone()
    };
    settings::save(&snap);
}

pub fn on_bubble_resized(_model: TrayIconKind, size_logical: i32) {
    let snap = {
        let mut s = lock_state();
        let Some(s) = s.as_mut() else {
            return;
        };
        s.settings.bubble_size_logical = size_logical;
        s.settings.clone()
    };
    settings::save(&snap);
}

pub fn on_menu_command(id: u32, _owner_hwnd: HWND) {
    let id = (id & 0xFFFF) as u16;
    match id {
        IDM_REFRESH => spawn_poll_thread(),
        IDM_EXIT => unsafe { PostQuitMessage(0) },
        IDM_FREQ_1MIN => set_poll_interval(POLL_1_MIN),
        IDM_FREQ_5MIN => set_poll_interval(POLL_5_MIN),
        IDM_FREQ_15MIN => set_poll_interval(POLL_15_MIN),
        IDM_FREQ_1HOUR => set_poll_interval(POLL_1_HOUR),
        IDM_MODEL_CLAUDE => toggle_model(ProviderId::Claude),
        IDM_MODEL_CHATGPT => toggle_model(ProviderId::ChatGpt),
        IDM_START_WITH_WINDOWS => toggle_startup(),
        IDM_RESET_POSITION => reset_positions(),
        IDM_VERSION_ACTION => version_action(),
        IDM_LANG_SYSTEM => set_language(None),
        x if x >= IDM_LANG_BASE => set_language_by_index((x - IDM_LANG_BASE) as usize),
        tray::IDM_TOGGLE_WIDGET => toggle_widget_visibility(),
        _ => {}
    }
}

// ---------- Timers ----------

fn on_timer(hwnd: HWND, id: usize) {
    match id {
        TIMER_POLL | TIMER_RESET_POLL => spawn_poll_thread(),
        TIMER_COUNTDOWN => refresh_countdowns(),
        TIMER_UPDATE_CHECK => {
            unsafe {
                let _ = KillTimer(hwnd, TIMER_UPDATE_CHECK);
            }
            begin_update_check(hwnd);
        }
        _ => {}
    }
}

// ---------- Poll thread ----------

fn spawn_poll_thread() {
    let msg_hwnd = match lock_state().as_ref() {
        Some(s) => s.msg_hwnd,
        None => return,
    };
    std::thread::spawn(move || {
        do_poll();
        unsafe {
            let _ = PostMessageW(
                msg_hwnd.to_hwnd(),
                WM_APP_USAGE_UPDATED,
                WPARAM(0),
                LPARAM(0),
            );
        }
    });
}

fn do_poll() {
    let results = {
        let mut s = lock_state();
        let Some(s) = s.as_mut() else {
            return;
        };
        let settings = s.settings.clone();
        s.registry.poll_enabled(&s.http, &settings)
    };
    let auth_failures = apply_results(results);
    if !auth_failures.is_empty() {
        attempt_refresh(auth_failures);
    }
}

fn apply_results(
    results: Vec<(ProviderId, Result<UsageWindows, usage::Error>)>,
) -> Vec<ProviderId> {
    let mut auth_failures = Vec::new();
    let mut s = lock_state();
    let Some(s) = s.as_mut() else {
        return auth_failures;
    };
    if results.is_empty() {
        return auth_failures;
    }
    let strings = s.i18n.strings().clone();
    let mut any_ok = false;
    for (id, outcome) in results {
        match outcome {
            Ok(windows) => {
                let entry = s.snapshots.entry(id).or_default();
                entry.windows = windows;
                entry.primary_text = i18n::format_window(&windows.primary, &strings);
                entry.secondary_text = i18n::format_window(&windows.secondary, &strings);
                any_ok = true;
            }
            Err(usage::Error::AuthRequired | usage::Error::TokenExpired) => {
                auth_failures.push(id);
                let entry = s.snapshots.entry(id).or_default();
                entry.primary_text = "!".into();
                entry.secondary_text = "!".into();
            }
            Err(e) => {
                log::warn!("provider {id:?} poll failed: {e}");
                let entry = s.snapshots.entry(id).or_default();
                entry.primary_text = "…".into();
                entry.secondary_text = "…".into();
            }
        }
    }
    s.last_poll_ok = any_ok;
    auth_failures
}

fn attempt_refresh(failures: Vec<ProviderId>) {
    let orchestrator = usage::refresh::Orchestrator::new(REFRESH_TIMEOUT);
    let mut needs_balloon = false;
    for id in failures {
        let outcome = match lock_state().as_ref() {
            Some(s) => s.registry.try_refresh(id, &orchestrator),
            None => return,
        };
        log::info!("refresh for {id:?}: {outcome:?}");
        if !matches!(outcome, usage::refresh::Outcome::Refreshed) {
            needs_balloon = true;
        }
    }
    if needs_balloon {
        show_token_expired_balloon();
    }
}

fn refresh_countdowns() {
    {
        let mut s = lock_state();
        let Some(s) = s.as_mut() else {
            return;
        };
        let strings = s.i18n.strings().clone();
        for entry in s.snapshots.values_mut() {
            entry.primary_text = i18n::format_window(&entry.windows.primary, &strings);
            entry.secondary_text = i18n::format_window(&entry.windows.secondary, &strings);
        }
    }
    propagate_to_ui();
}

fn propagate_to_ui() {
    let snapshot = {
        let s = lock_state();
        s.as_ref().map(|s| UiSnapshot {
            bubbles: s.bubbles.clone(),
            snapshots: s.snapshots.clone(),
            settings: s.settings.clone(),
            i18n_strings: s.i18n.strings().clone(),
            is_dark: s.is_dark,
            msg_hwnd: s.msg_hwnd,
            last_poll_ok: s.last_poll_ok,
        })
    };
    let Some(snap) = snapshot else {
        return;
    };

    for (kind, hwnd) in snap.bubbles.iter() {
        let id = kind_to_provider(*kind);
        let entry = snap.snapshots.get(&id);
        let session_pct = entry.map(|s| s.windows.primary.utilization);
        let weekly_pct = entry.map(|s| s.windows.secondary.utilization);
        let session_text = entry.map(|s| s.primary_text.clone()).unwrap_or_default();
        let weekly_text = entry.map(|s| s.secondary_text.clone()).unwrap_or_default();
        bubble::update_data(
            hwnd.to_hwnd(),
            session_pct,
            session_text,
            weekly_pct,
            weekly_text,
        );
    }
    refresh_tray_icons_with(&snap);

    if panel::is_visible() {
        if let Some(model) = panel::current_model() {
            let id = kind_to_provider(model);
            if let Some(provider_state) = snap.snapshots.get(&id) {
                panel::refresh_data(build_panel_data_from(&snap, model, provider_state));
            }
        }
    }
    schedule_countdown_timer(&snap);
}

#[derive(Clone)]
struct UiSnapshot {
    bubbles: HashMap<TrayIconKind, SendHwnd>,
    snapshots: HashMap<ProviderId, ProviderUiState>,
    settings: Settings,
    i18n_strings: LocaleStrings,
    is_dark: bool,
    msg_hwnd: SendHwnd,
    last_poll_ok: bool,
}

fn kind_to_provider(k: TrayIconKind) -> ProviderId {
    match k {
        ProviderId::Claude => ProviderId::Claude,
        ProviderId::ChatGpt => ProviderId::ChatGpt,
    }
}

fn build_panel_data(model: TrayIconKind) -> PanelData {
    let s = lock_state();
    let Some(s) = s.as_ref() else {
        return placeholder_panel(model);
    };
    let id = kind_to_provider(model);
    let strings = s.i18n.strings().clone();
    let provider_state = s.snapshots.get(&id).cloned().unwrap_or_default();
    PanelData {
        model,
        session_pct: provider_state.windows.primary.utilization,
        session_text: provider_state.primary_text,
        weekly_pct: provider_state.windows.secondary.utilization,
        weekly_text: provider_state.secondary_text,
        is_dark: s.is_dark,
        strings,
    }
}

fn build_panel_data_from(snap: &UiSnapshot, model: TrayIconKind, p: &ProviderUiState) -> PanelData {
    PanelData {
        model,
        session_pct: p.windows.primary.utilization,
        session_text: p.primary_text.clone(),
        weekly_pct: p.windows.secondary.utilization,
        weekly_text: p.secondary_text.clone(),
        is_dark: snap.is_dark,
        strings: snap.i18n_strings.clone(),
    }
}

fn placeholder_panel(model: TrayIconKind) -> PanelData {
    let strings = i18n::I18n::load(None).strings().clone();
    PanelData {
        model,
        session_pct: 0.0,
        session_text: String::new(),
        weekly_pct: 0.0,
        weekly_text: String::new(),
        is_dark: false,
        strings,
    }
}

fn schedule_countdown_timer(snap: &UiSnapshot) {
    let mut min_ttl: Option<Duration> = None;
    for entry in snap.snapshots.values() {
        for w in [&entry.windows.primary, &entry.windows.secondary] {
            if let Some(d) = i18n::time_until_display_change(w.resets_at) {
                min_ttl = Some(min_ttl.map_or(d, |prev| prev.min(d)));
            }
        }
    }
    if let Some(d) = min_ttl {
        let ms = (d.as_millis().min(u32::MAX as u128) as u32).max(1_000);
        unsafe {
            let _ = KillTimer(snap.msg_hwnd.to_hwnd(), TIMER_COUNTDOWN);
            SetTimer(snap.msg_hwnd.to_hwnd(), TIMER_COUNTDOWN, ms, None);
        }
    }
}

// ---------- Tray icons ----------

fn refresh_tray_icons() {
    let snap = {
        let s = lock_state();
        s.as_ref().map(|s| UiSnapshot {
            bubbles: s.bubbles.clone(),
            snapshots: s.snapshots.clone(),
            settings: s.settings.clone(),
            i18n_strings: s.i18n.strings().clone(),
            is_dark: s.is_dark,
            msg_hwnd: s.msg_hwnd,
            last_poll_ok: s.last_poll_ok,
        })
    };
    if let Some(snap) = snap {
        refresh_tray_icons_with(&snap);
    }
}

fn refresh_tray_icons_with(snap: &UiSnapshot) {
    let mut icons = Vec::new();
    if snap.settings.show_claude_code {
        let entry = snap.snapshots.get(&ProviderId::Claude);
        icons.push(TrayIconData {
            kind: ProviderId::Claude,
            percent: if snap.last_poll_ok {
                entry.map(|e| e.windows.primary.utilization)
            } else {
                None
            },
            tooltip: format!(
                "{} {}: {} | {}: {}",
                snap.i18n_strings.claude_label,
                snap.i18n_strings.session_window,
                entry.map(|e| e.primary_text.as_str()).unwrap_or(""),
                snap.i18n_strings.weekly_window,
                entry.map(|e| e.secondary_text.as_str()).unwrap_or(""),
            ),
        });
    }
    if snap.settings.show_codex {
        let entry = snap.snapshots.get(&ProviderId::ChatGpt);
        icons.push(TrayIconData {
            kind: ProviderId::ChatGpt,
            percent: if snap.last_poll_ok {
                entry.map(|e| e.windows.primary.utilization)
            } else {
                None
            },
            tooltip: format!(
                "{} {}: {} | {}: {}",
                snap.i18n_strings.chatgpt_label,
                snap.i18n_strings.session_window,
                entry.map(|e| e.primary_text.as_str()).unwrap_or(""),
                snap.i18n_strings.weekly_window,
                entry.map(|e| e.secondary_text.as_str()).unwrap_or(""),
            ),
        });
    }
    tray::sync(snap.msg_hwnd.to_hwnd(), &icons);
}

fn handle_tray_action(action: TrayAction) {
    match action {
        TrayAction::None => {}
        TrayAction::ToggleWidget => toggle_widget_visibility(),
        TrayAction::ShowContextMenu => {
            let owner = lock_state()
                .as_ref()
                .and_then(|s| s.bubbles.values().next().copied())
                .map(|h| h.to_hwnd())
                .unwrap_or_default();
            if owner != HWND::default() {
                show_context_menu(owner);
            }
        }
    }
}

fn show_token_expired_balloon() {
    let payload = {
        let mut s = lock_state();
        let Some(s) = s.as_mut() else {
            return;
        };
        if let Some(last) = s.last_balloon_at {
            if last.elapsed() < BALLOON_COOLDOWN {
                return;
            }
        }
        s.last_balloon_at = Some(Instant::now());
        let strings = s.i18n.strings();
        let (kind, title, body) = if s.settings.show_claude_code {
            (
                ProviderId::Claude,
                strings.token_expired_title.clone(),
                strings.token_expired_body.clone(),
            )
        } else {
            (
                ProviderId::ChatGpt,
                strings.chatgpt_token_expired_title.clone(),
                strings.chatgpt_token_expired_body.clone(),
            )
        };
        (s.msg_hwnd, kind, title, body)
    };
    tray::notify(payload.0.to_hwnd(), payload.1, &payload.2, &payload.3);
}

// ---------- Context menu ----------

struct ContextMenuSnapshot {
    strings: LocaleStrings,
    available: Vec<(String, String)>,
    language_override: Option<String>,
    current_interval: u32,
    show_claude: bool,
    show_chatgpt: bool,
    widget_visible: bool,
    install_channel: InstallChannel,
    update_status: UpdateStatus,
}

fn show_context_menu(owner_hwnd: HWND) {
    let snap = match lock_state().as_ref() {
        Some(s) => ContextMenuSnapshot {
            strings: s.i18n.strings().clone(),
            available: s
                .i18n
                .available()
                .map(|(c, n)| (c.to_string(), n.to_string()))
                .collect(),
            language_override: s.settings.language.clone(),
            current_interval: s.settings.poll_interval_ms,
            show_claude: s.settings.show_claude_code,
            show_chatgpt: s.settings.show_codex,
            widget_visible: s.settings.widget_visible,
            install_channel: s.install_channel,
            update_status: s.update_status,
        },
        None => return,
    };

    unsafe {
        let menu = match CreatePopupMenu() {
            Ok(m) => m,
            Err(_) => return,
        };

        append_item(menu, IDM_REFRESH, &snap.strings.refresh, MENU_ITEM_FLAGS(0));

        let freq = CreatePopupMenu().unwrap();
        for (id, interval, label) in [
            (IDM_FREQ_1MIN, POLL_1_MIN, &snap.strings.one_minute),
            (IDM_FREQ_5MIN, POLL_5_MIN, &snap.strings.five_minutes),
            (IDM_FREQ_15MIN, POLL_15_MIN, &snap.strings.fifteen_minutes),
            (IDM_FREQ_1HOUR, POLL_1_HOUR, &snap.strings.one_hour),
        ] {
            let flags = if interval == snap.current_interval {
                MF_CHECKED
            } else {
                MENU_ITEM_FLAGS(0)
            };
            append_item(freq, id, label, flags);
        }
        append_submenu(menu, freq, &snap.strings.update_frequency);

        let models = CreatePopupMenu().unwrap();
        append_item(
            models,
            IDM_MODEL_CLAUDE,
            &snap.strings.claude_label,
            if snap.show_claude { MF_CHECKED } else { MENU_ITEM_FLAGS(0) },
        );
        append_item(
            models,
            IDM_MODEL_CHATGPT,
            &snap.strings.chatgpt_label,
            if snap.show_chatgpt { MF_CHECKED } else { MENU_ITEM_FLAGS(0) },
        );
        append_submenu(menu, models, &snap.strings.models);

        let settings_menu = CreatePopupMenu().unwrap();
        append_item(
            settings_menu,
            IDM_START_WITH_WINDOWS,
            &snap.strings.start_with_windows,
            if is_startup_enabled() { MF_CHECKED } else { MENU_ITEM_FLAGS(0) },
        );
        append_item(
            settings_menu,
            IDM_RESET_POSITION,
            &snap.strings.reset_position,
            MENU_ITEM_FLAGS(0),
        );

        let lang = CreatePopupMenu().unwrap();
        append_item(
            lang,
            IDM_LANG_SYSTEM,
            &snap.strings.system_default,
            if snap.language_override.is_none() { MF_CHECKED } else { MENU_ITEM_FLAGS(0) },
        );
        for (i, (code, name)) in snap.available.iter().enumerate() {
            let id = IDM_LANG_BASE + i as u16;
            let flags = if snap
                .language_override
                .as_deref()
                .map(|c| c == code)
                .unwrap_or(false)
            {
                MF_CHECKED
            } else {
                MENU_ITEM_FLAGS(0)
            };
            append_item(lang, id, name, flags);
        }
        append_submenu(settings_menu, lang, &snap.strings.language);
        let _ = AppendMenuW(settings_menu, MF_SEPARATOR, 0, PCWSTR::null());

        let version_label = version_action_label(&snap);
        let version_flags = if matches!(
            snap.update_status,
            UpdateStatus::Checking | UpdateStatus::Applying
        ) {
            MF_GRAYED
        } else {
            MENU_ITEM_FLAGS(0)
        };
        append_item(settings_menu, IDM_VERSION_ACTION, &version_label, version_flags);
        append_submenu(menu, settings_menu, &snap.strings.settings);

        append_item(
            menu,
            tray::IDM_TOGGLE_WIDGET,
            &snap.strings.show_widget,
            if snap.widget_visible { MF_CHECKED } else { MENU_ITEM_FLAGS(0) },
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        append_item(menu, IDM_EXIT, &snap.strings.exit, MENU_ITEM_FLAGS(0));

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(owner_hwnd);
        let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON, pt.x, pt.y, 0, owner_hwnd, None);
        let _ = DestroyMenu(menu);
    }
}

fn append_item(menu: HMENU, id: u16, label: &str, flags: MENU_ITEM_FLAGS) {
    let w = os::to_utf16_nul(label);
    unsafe {
        let _ = AppendMenuW(menu, flags, id as usize, PCWSTR::from_raw(w.as_ptr()));
    }
}

fn append_submenu(menu: HMENU, submenu: HMENU, label: &str) {
    let w = os::to_utf16_nul(label);
    unsafe {
        let _ = AppendMenuW(menu, MF_POPUP, submenu.0 as usize, PCWSTR::from_raw(w.as_ptr()));
    }
}

fn version_action_label(snap: &ContextMenuSnapshot) -> String {
    let base = match snap.update_status {
        UpdateStatus::Idle => snap.strings.check_for_updates.clone(),
        UpdateStatus::Checking => snap.strings.checking_for_updates.clone(),
        UpdateStatus::UpToDate => snap.strings.up_to_date.clone(),
        UpdateStatus::Available => snap.strings.update_available.clone(),
        UpdateStatus::Applying => snap.strings.applying_update.clone(),
        UpdateStatus::Failed => snap.strings.update_failed.clone(),
    };
    match snap.install_channel {
        InstallChannel::Winget => format!("{base} ({})", snap.strings.update_via_winget),
        InstallChannel::Portable => base,
    }
}

// ---------- Menu actions ----------

fn set_poll_interval(ms: u32) {
    let (snap, msg_hwnd) = {
        let mut s = lock_state();
        let Some(s) = s.as_mut() else {
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

fn toggle_model(model: TrayIconKind) {
    let (settings, is_dark) = {
        let mut s = lock_state();
        let Some(s) = s.as_mut() else {
            return;
        };
        match model {
            ProviderId::Claude => s.settings.show_claude_code = !s.settings.show_claude_code,
            ProviderId::ChatGpt => s.settings.show_codex = !s.settings.show_codex,
        }
        if !s.settings.show_claude_code && !s.settings.show_codex {
            match model {
                ProviderId::Claude => s.settings.show_claude_code = true,
                ProviderId::ChatGpt => s.settings.show_codex = true,
            }
        }
        (s.settings.clone(), s.is_dark)
    };
    settings::save(&settings);

    let want = match model {
        ProviderId::Claude => settings.show_claude_code,
        ProviderId::ChatGpt => settings.show_codex,
    };
    let existing = lock_state().as_ref().and_then(|s| s.bubbles.get(&model).copied());
    match (want, existing) {
        (true, None) => spawn_bubble(model, &settings, is_dark),
        (false, Some(h)) => {
            bubble::destroy(h.to_hwnd());
            if let Some(s) = lock_state().as_mut() {
                s.bubbles.remove(&model);
            }
        }
        _ => {}
    }
    refresh_tray_icons();
    spawn_poll_thread();
}

fn toggle_widget_visibility() {
    let (visible, snap) = {
        let mut s = lock_state();
        let Some(s) = s.as_mut() else {
            return;
        };
        s.settings.widget_visible = !s.settings.widget_visible;
        (s.settings.widget_visible, s.settings.clone())
    };
    settings::save(&snap);
    let hwnds: Vec<HWND> = lock_state()
        .as_ref()
        .map(|s| s.bubbles.values().map(|h| h.to_hwnd()).collect())
        .unwrap_or_default();
    for h in hwnds {
        bubble::set_user_visible(h, visible);
    }
}

fn reset_positions() {
    let snap = {
        let mut s = lock_state();
        let Some(s) = s.as_mut() else {
            return;
        };
        s.settings.bubble_positions.reset_all();
        s.settings.clone()
    };
    settings::save(&snap);
    let hwnds: Vec<HWND> = lock_state()
        .as_ref()
        .map(|s| s.bubbles.values().map(|h| h.to_hwnd()).collect())
        .unwrap_or_default();
    for h in hwnds {
        bubble::destroy(h);
    }
    if let Some(s) = lock_state().as_mut() {
        s.bubbles.clear();
    }
    create_initial_bubbles();
}

fn set_language(_dummy: Option<()>) {
    let snap = {
        let mut s = lock_state();
        let Some(s) = s.as_mut() else {
            return;
        };
        s.i18n.set_active(None);
        s.settings.language = None;
        s.settings.clone()
    };
    settings::save(&snap);
    propagate_to_ui();
}

fn set_language_by_index(idx: usize) {
    let snap = {
        let mut s = lock_state();
        let Some(s) = s.as_mut() else {
            return;
        };
        let code = s.i18n.available().nth(idx).map(|(c, _)| c.to_string());
        if let Some(c) = code.as_deref() {
            s.i18n.set_active(Some(c));
        }
        s.settings.language = code;
        s.settings.clone()
    };
    settings::save(&snap);
    propagate_to_ui();
}

fn version_action() {
    enum Act {
        Apply(update::Release, InstallChannel),
        Check(SendHwnd),
    }
    let act = match lock_state().as_ref() {
        Some(s) => match (s.update_status, s.update_release.as_ref()) {
            (UpdateStatus::Available, Some(r)) => Act::Apply(r.clone(), s.install_channel),
            _ => Act::Check(s.msg_hwnd),
        },
        None => return,
    };
    match act {
        Act::Apply(release, channel) => {
            if let Some(s) = lock_state().as_mut() {
                s.update_status = UpdateStatus::Applying;
            }
            let result: Result<(), Box<dyn std::error::Error>> = match channel {
                InstallChannel::Winget => {
                    // Winget channel is reserved for future use; until a
                    // winget package ships, this branch is unreachable.
                    Err("winget channel not supported yet".into())
                }
                InstallChannel::Portable => {
                    match net::Client::new(HTTP_USER_AGENT) {
                        Ok(c) => update::install::begin(&c, &release).map_err(|e| e.into()),
                        Err(e) => Err(e.into()),
                    }
                }
            };
            match result {
                Ok(()) => unsafe { PostQuitMessage(0) },
                Err(e) => {
                    log::error!("update apply failed: {e}");
                    if let Some(s) = lock_state().as_mut() {
                        s.update_status = UpdateStatus::Failed;
                    }
                }
            }
        }
        Act::Check(hwnd) => begin_update_check(hwnd.to_hwnd()),
    }
}

fn schedule_update_check_timer(hwnd: HWND) {
    let last = lock_state()
        .as_ref()
        .and_then(|s| s.settings.last_update_check_unix);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let due = last.map_or(true, |t| now.saturating_sub(t) >= UPDATE_CHECK_INTERVAL_SECS);
    if due {
        begin_update_check(hwnd);
    } else {
        let remaining = UPDATE_CHECK_INTERVAL_SECS.saturating_sub(now.saturating_sub(last.unwrap_or(0)));
        let ms = (remaining.saturating_mul(1000).min(u32::MAX as u64)) as u32;
        unsafe {
            SetTimer(hwnd, TIMER_UPDATE_CHECK, ms, None);
        }
    }
}

fn begin_update_check(hwnd: HWND) {
    if let Some(s) = lock_state().as_mut() {
        s.update_status = UpdateStatus::Checking;
    }
    let send_hwnd = SendHwnd::from_hwnd(hwnd);
    std::thread::spawn(move || {
        let result = match net::Client::new(HTTP_USER_AGENT) {
            Ok(c) => update::release::fetch_latest(&c),
            Err(e) => Err(update::Error::Network(e)),
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let snap_opt = {
            let mut s = lock_state();
            s.as_mut().map(|s| {
                s.settings.last_update_check_unix = Some(now);
                match result {
                    Ok(CheckOutcome::UpToDate) => {
                        s.update_status = UpdateStatus::UpToDate;
                        s.update_release = None;
                    }
                    Ok(CheckOutcome::Available(r)) => {
                        s.update_status = UpdateStatus::Available;
                        s.update_release = Some(r);
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

// ---------- Start-with-Windows ----------

fn is_startup_enabled() -> bool {
    os::registry::value_exists(STARTUP_REGISTRY_PATH, STARTUP_VALUE_NAME)
}

fn toggle_startup() {
    if is_startup_enabled() {
        let _ = os::registry::delete_value(STARTUP_REGISTRY_PATH, STARTUP_VALUE_NAME);
    } else if let Ok(exe) = std::env::current_exe() {
        let quoted = format!("\"{}\"", exe.to_string_lossy());
        let _ = os::registry::write_string(STARTUP_REGISTRY_PATH, STARTUP_VALUE_NAME, &quoted);
    }
}
