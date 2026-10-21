//! App runtime: start-up, the shared state, the poll worker and the bridge
//! that pushes state to the webviews.
//!
//! Locking rule: no Tauri window, tray, menu or emit API is called while the
//! [`AppState`] mutex is held. Code reads state through
//! [`Runtime::with_state`], which returns owned data, and calls Tauri after
//! the guard is gone. Tauri calls from a worker block on the main thread, so
//! holding the lock across one can deadlock against a main-thread handler.

use std::error::Error;
use std::ffi::OsString;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use tauri::{
    App, AppHandle, Emitter, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, Window,
    WindowEvent,
};

use crate::cli::{self, CliArgs};
use crate::core::geom::{self, Rect};
use crate::core::i18n::{I18n, LocaleStrings};
use crate::core::payload::{self, BubblePayload, Canvas};
use crate::core::settings::{self, Settings};
use crate::core::state::{Alert, PollOutcome, UpdateStatus, UsageState};
use crate::scheduler::{self, Action, Message, SchedulerHost};
use crate::usage::{self, refresh, ProviderId, Registry};
use crate::{diag, handoff, net, platform};

/// Event carrying a [`BubblePayload`] to a bubble webview.
pub const BUBBLE_UPDATE_EVENT: &str = "bubble:update";

/// Label prefix of every bubble window.
pub const BUBBLE_LABEL_PREFIX: &str = "bubble-";

/// How long a relaunched instance waits for its parent to exit. The
/// single-instance plugin cannot retry, so this must outlast a slow
/// WebView2 teardown.
const WAIT_PID_TIMEOUT: Duration = Duration::from_secs(15);

/// Per-provider budget for the CLI credential refresh.
const REFRESH_TIMEOUT: Duration = Duration::from_secs(8);

#[cfg(windows)]
const WEBVIEW2_MISSING_TITLE: &str = "Claude Code Usage Bubble";
#[cfg(windows)]
const WEBVIEW2_MISSING_TEXT: &str = "Claude Code Usage Bubble needs the Microsoft Edge \
WebView2 Runtime.\n\nDownload the Evergreen runtime from:\n\
https://developer.microsoft.com/microsoft-edge/webview2/";

/// Everything the app knows, behind one mutex in [`Runtime`].
#[derive(Debug)]
pub struct AppState {
    pub settings: Settings,
    pub strings: LocaleStrings,
    pub is_dark: bool,
    pub usage: UsageState,
    pub update_status: UpdateStatus,
    /// The "update applied" alert from `--updated-to`, shown once the
    /// notification backend is up.
    pub pending_update_alert: Option<Alert>,
}

/// Holds the `AppHandle`; every push to a webview goes through it.
pub struct UiBridge {
    app: AppHandle,
}

impl UiBridge {
    pub fn app(&self) -> &AppHandle {
        &self.app
    }

    /// Send each bubble its payload. The window lookups and emits run on the
    /// main thread; `snapshot` is owned, so no state lock is involved.
    fn emit_bubble_updates(&self, snapshot: BubbleSnapshot) {
        let app = self.app.clone();
        let dispatched = self.app.run_on_main_thread(move || {
            let now = SystemTime::now();
            for provider in &snapshot.providers {
                let label = bubble_label(*provider);
                let Some(window) = app.get_webview_window(&label) else {
                    continue;
                };
                let scale = match window.scale_factor() {
                    Ok(scale) => scale,
                    Err(err) => {
                        log::warn!("scale factor of {label} unavailable: {err}");
                        continue;
                    }
                };
                let payload = snapshot.payload(*provider, scale, now);
                if let Err(err) = app.emit_to(label.as_str(), BUBBLE_UPDATE_EVENT, payload) {
                    log::warn!("emit {BUBBLE_UPDATE_EVENT} to {label} failed: {err}");
                }
            }
        });
        if let Err(err) = dispatched {
            log::warn!("bubble update could not reach the main thread: {err}");
        }
    }
}

/// An owned copy of what the bubble payloads need.
struct BubbleSnapshot {
    providers: Vec<ProviderId>,
    usage: UsageState,
    strings: LocaleStrings,
    is_dark: bool,
    size_logical: i32,
}

impl BubbleSnapshot {
    fn of(state: &AppState) -> Self {
        Self {
            providers: enabled_live_providers(&state.settings),
            usage: state.usage.clone(),
            strings: state.strings.clone(),
            is_dark: state.is_dark,
            size_logical: state.settings.bubble_size_logical,
        }
    }

    fn payload(&self, provider: ProviderId, scale_factor: f64, now: SystemTime) -> BubblePayload {
        payload::bubble_payload(
            provider,
            self.usage.snapshots.get(&provider),
            &self.strings,
            self.is_dark,
            bubble_canvas(self.size_logical, scale_factor),
            now,
        )
    }
}

/// Shared runtime, managed by Tauri as `Arc<Runtime>`.
pub struct Runtime {
    ui: UiBridge,
    state: Mutex<AppState>,
    http: net::Client,
    registry: Mutex<Registry>,
    /// At most one poll runs at a time; a Refresh spam or a timer firing
    /// during a slow poll is dropped instead of stacking requests.
    poll_in_flight: AtomicBool,
    scheduler: Sender<Message>,
}

impl Runtime {
    /// Run `f` on the state and return what it returns. Return owned data
    /// and call Tauri only after this returns (see the module docs).
    pub fn with_state<T>(&self, f: impl FnOnce(&mut AppState) -> T) -> T {
        let mut guard = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut guard)
    }

    pub fn ui(&self) -> &UiBridge {
        &self.ui
    }

    /// Hand a message to the scheduler thread.
    pub fn send(&self, message: Message) {
        if self.scheduler.send(message).is_err() {
            log::warn!("scheduler is not running; message dropped");
        }
    }

    /// The payload for one bubble at the given window scale factor.
    pub fn bubble_payload(&self, provider: ProviderId, scale_factor: f64) -> BubblePayload {
        let snapshot = self.with_state(|s| BubbleSnapshot::of(s));
        snapshot.payload(provider, scale_factor, SystemTime::now())
    }

    /// Push the current state to every UI surface, then re-arm the
    /// countdown for the next visible change.
    pub fn propagate(&self) {
        let (snapshot, countdown) = self.with_state(|s| {
            (
                BubbleSnapshot::of(s),
                s.usage.countdown_delay(SystemTime::now()),
            )
        });
        self.ui.emit_bubble_updates(snapshot);
        self.send(Message::RecomputeCountdown(countdown));
    }

    /// Start a poll on a worker thread unless one is already running.
    pub fn spawn_poll(self: &Arc<Self>) {
        if self
            .poll_in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            log::debug!("poll already in flight; skipped");
            return;
        }
        let runtime = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("poll".into())
            .spawn(move || {
                runtime.poll_once();
                runtime.poll_in_flight.store(false, Ordering::Release);
                runtime.propagate();
            });
        if let Err(err) = spawned {
            log::error!("poll thread could not start: {err}");
            self.poll_in_flight.store(false, Ordering::Release);
        }
    }

    /// Poll every enabled provider, apply the results, then run the CLI
    /// credential refresh for the providers whose credentials were rejected.
    /// Blocking; runs on the poll worker.
    fn poll_once(&self) {
        let settings = self.with_state(|s| s.settings.clone());
        let results = self
            .registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .poll_enabled(&self.http, &settings);
        let outcomes = results
            .into_iter()
            .map(|(provider, result)| (provider, poll_outcome(provider, result)))
            .collect();
        let applied =
            self.with_state(|s| s.usage.apply_results(outcomes, &s.strings, Instant::now()));
        self.deliver_alerts(applied.alerts);
        if !applied.auth_failures.is_empty() {
            self.refresh_credentials(&applied.auth_failures);
        }
    }

    /// Run the CLI refresh for each provider in order. The session-expired
    /// alert names the first provider that did not refresh; the next poll
    /// picks the new credentials up, there is no immediate re-poll.
    fn refresh_credentials(&self, failures: &[ProviderId]) {
        let orchestrator = refresh::Orchestrator::new(REFRESH_TIMEOUT);
        let mut expired: Option<ProviderId> = None;
        for &provider in failures {
            let outcome = self
                .registry
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .try_refresh(provider, &orchestrator);
            log::info!("credential refresh for {}: {outcome:?}", provider.slug());
            if outcome != refresh::Outcome::Refreshed {
                expired.get_or_insert(provider);
            }
        }
        if let Some(provider) = expired {
            let alert =
                self.with_state(|s| s.usage.session_expired_alert(provider, Instant::now()));
            self.deliver_alerts(alert.into_iter().collect());
        }
    }

    /// Show alerts. Until a notification backend is registered they only
    /// reach the diagnostic log.
    fn deliver_alerts(&self, alerts: Vec<Alert>) {
        if alerts.is_empty() {
            return;
        }
        let strings = self.with_state(|s| s.strings.clone());
        for alert in alerts {
            let (title, body) = alert.text(&strings);
            log::info!("alert: {title}: {body}");
        }
    }

    /// Re-format the countdown texts after time passed and push them out.
    fn refresh_countdowns(&self) {
        self.with_state(|s| s.usage.refresh_texts(&s.strings));
        self.propagate();
    }

    /// The automatic update check. No updater is registered in this build,
    /// so the deadline stays disarmed until a check reports back.
    fn check_for_updates(&self) {
        log::debug!("update check due; no updater is registered");
    }

    /// Spawn the next instance with `--wait-pid` plus `extra_args`, then
    /// exit. On a failed spawn this instance keeps running.
    pub fn relaunch_and_exit(&self, extra_args: &[OsString]) -> io::Result<()> {
        handoff::relaunch(extra_args)?;
        self.exit();
        Ok(())
    }

    /// Save the settings and start a fresh instance.
    pub fn restart(&self) {
        let snapshot = self.with_state(|s| s.settings.clone());
        if let Err(err) = settings::save(&snapshot) {
            log::warn!("restart: settings save failed: {err}");
        }
        if let Err(err) = self.relaunch_and_exit(&[]) {
            log::error!("restart: relaunch failed, keeping this instance: {err}");
        }
    }

    /// End the process. The scheduler stops from the `Exit` run event.
    pub fn exit(&self) {
        self.ui.app.exit(0);
    }
}

impl SchedulerHost for Arc<Runtime> {
    fn run(&self, action: Action) {
        match action {
            Action::Poll => self.spawn_poll(),
            Action::Countdown => self.refresh_countdowns(),
            Action::CheckForUpdates => self.check_for_updates(),
        }
    }
}

/// Map a provider error onto the portable outcome, logging transient ones.
fn poll_outcome(
    provider: ProviderId,
    result: Result<usage::UsageWindows, usage::Error>,
) -> PollOutcome {
    match result {
        Ok(windows) => {
            log::info!(
                "poll {}: session {:.1}%, weekly {:.1}%",
                provider.slug(),
                windows.primary.utilization,
                windows.secondary.utilization
            );
            PollOutcome::Ok(windows)
        }
        Err(usage::Error::AuthRequired | usage::Error::TokenExpired) => {
            log::info!("poll {}: credentials rejected", provider.slug());
            PollOutcome::AuthFailed
        }
        Err(err) => {
            log::warn!("poll {} failed: {err}", provider.slug());
            PollOutcome::Failed
        }
    }
}

/// Window label of a provider's bubble.
pub fn bubble_label(provider: ProviderId) -> String {
    format!("{BUBBLE_LABEL_PREFIX}{}", provider.slug())
}

/// Live providers the user has enabled, in display order.
pub fn enabled_live_providers(settings: &Settings) -> Vec<ProviderId> {
    ProviderId::LIVE_USAGE
        .into_iter()
        .filter(|p| settings.is_provider_enabled(*p))
        .collect()
}

/// The bubble canvas in physical px for a window scale factor.
pub fn bubble_canvas(size_logical: i32, scale_factor: f64) -> Canvas {
    let dpi = if scale_factor.is_finite() && scale_factor > 0.0 {
        (scale_factor * f64::from(geom::BASE_DPI)).round() as u32
    } else {
        geom::BASE_DPI
    };
    let (w, h) = geom::bubble_size_px(size_logical, dpi);
    Canvas {
        w: u32::try_from(w).unwrap_or(1),
        h: u32::try_from(h).unwrap_or(1),
        dpi,
    }
}

/// Whether to run GTK on X11 (Xwayland) under a Wayland session:
/// positioning, snapping and always-on-top only work there. An explicit
/// `GDK_BACKEND` wins, and without an X server (`DISPLAY` unset) GTK would
/// fail to start, so the native Wayland backend stays.
pub fn needs_x11_backend(
    wayland_display: Option<OsString>,
    gdk_backend: Option<OsString>,
    display: Option<OsString>,
) -> bool {
    let set = |v: &Option<OsString>| v.as_ref().is_some_and(|v| !v.is_empty());
    set(&wayland_display) && !set(&gdk_backend) && set(&display)
}

/// Process entry: everything that must happen before the Tauri builder,
/// then the builder itself.
pub fn run() {
    let cli = cli::from_env();
    match diag::init(cli.diagnose) {
        Ok(Some(path)) => log::info!(
            "startup args={:?} log_path={}",
            std::env::args_os().collect::<Vec<_>>(),
            path.display()
        ),
        Ok(None) => {}
        Err(err) => eprintln!("diagnostic log could not be opened: {err}"),
    }

    #[cfg(windows)]
    if !platform::windows::webview2_available() {
        log::error!("WebView2 runtime missing; exiting");
        platform::windows::message_box_error(WEBVIEW2_MISSING_TITLE, WEBVIEW2_MISSING_TEXT);
        std::process::exit(0);
    }

    if let Some(pid) = cli.wait_pid {
        log::info!("waiting up to {WAIT_PID_TIMEOUT:?} for pid {pid} to exit");
        let started = Instant::now();
        if platform::wait_pid(pid, WAIT_PID_TIMEOUT) {
            log::info!("pid {pid} gone after {:?}", started.elapsed());
        } else {
            log::warn!("pid {pid} still running after {WAIT_PID_TIMEOUT:?}; starting anyway");
        }
    }

    #[cfg(target_os = "linux")]
    if needs_x11_backend(
        std::env::var_os("WAYLAND_DISPLAY"),
        std::env::var_os("GDK_BACKEND"),
        std::env::var_os("DISPLAY"),
    ) {
        log::info!("Wayland session with Xwayland: using GDK_BACKEND=x11");
        // No other thread exists yet, so changing the environment is safe.
        std::env::set_var("GDK_BACKEND", "x11");
    }

    let app = tauri::Builder::default()
        // First, so a second instance exits before anything else starts.
        .plugin(tauri_plugin_single_instance::init(|_app, _argv, _cwd| {}))
        .invoke_handler(tauri::generate_handler![
            crate::commands::get_bubble_state,
            crate::commands::get_strings,
            crate::commands::log_js,
            crate::commands::bubble_ready,
        ])
        .on_window_event(on_window_event)
        .setup(move |app| setup(app, &cli))
        .build(tauri::generate_context!());

    let app = match app {
        Ok(app) => app,
        Err(err) => {
            log::error!("tauri builder failed: {err}");
            eprintln!("Claude Code Usage Bubble failed to start: {err}");
            std::process::exit(1);
        }
    };

    app.run(|app, event| match event {
        // Destroying the last window must not end the process; only an
        // explicit `exit` (which carries an exit code) does.
        RunEvent::ExitRequested {
            code: None, api, ..
        } => api.prevent_exit(),
        RunEvent::Exit => {
            if let Some(runtime) = app.try_state::<Arc<Runtime>>() {
                runtime.send(Message::Shutdown);
            }
        }
        _ => {}
    });
}

/// Bubbles ignore close requests (Alt+F4); they are only ever destroyed.
fn on_window_event(window: &Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        if window.label().starts_with(BUBBLE_LABEL_PREFIX) {
            api.prevent_close();
        }
    }
}

fn setup(app: &mut App, cli: &CliArgs) -> Result<(), Box<dyn Error>> {
    let mut settings = settings::load();
    settings::drop_offscreen_positions(&mut settings, &monitor_rects(app.handle()));
    let is_dark = platform::system_is_dark();
    let http = net::Client::new(net::USER_AGENT)?;

    let state = AppState {
        strings: I18n::load().strings().clone(),
        is_dark,
        usage: UsageState::default(),
        update_status: UpdateStatus::Idle,
        pending_update_alert: cli
            .updated_to
            .clone()
            .map(|version| Alert::UpdateApplied { version }),
        settings,
    };
    let config = scheduler::Config {
        poll_interval: Duration::from_millis(u64::from(state.settings.poll_interval_ms)),
        update_interval_secs: state.settings.update_check_interval_secs,
        last_update_check_unix: state.settings.last_update_check_unix,
    };
    let first_provider = enabled_live_providers(&state.settings)
        .first()
        .copied()
        .unwrap_or(ProviderId::Claude);

    let (tx, rx) = mpsc::channel();
    let runtime = Arc::new(Runtime {
        ui: UiBridge {
            app: app.handle().clone(),
        },
        state: Mutex::new(state),
        http,
        registry: Mutex::new(Registry::with_defaults()),
        poll_in_flight: AtomicBool::new(false),
        scheduler: tx,
    });
    app.manage(Arc::clone(&runtime));

    match std::env::current_exe() {
        Ok(exe) => handoff::cleanup_stale_files(&exe, handoff::updates_dir().as_deref()),
        Err(err) => log::warn!("startup cleanup skipped, no executable path: {err}"),
    }

    // The thread is detached; it stops on `Message::Shutdown` at exit.
    scheduler::spawn(config, rx, Arc::clone(&runtime))?;
    runtime.send(Message::PollNow);

    create_test_window(app, first_provider)?;
    Ok(())
}

/// Monitor rects in physical px. Empty when they cannot be enumerated,
/// which keeps every saved position.
fn monitor_rects(app: &AppHandle) -> Vec<Rect> {
    match app.available_monitors() {
        Ok(monitors) => monitors
            .iter()
            .map(|m| {
                let pos = m.position();
                let size = m.size();
                Rect::from_xywh(
                    pos.x,
                    pos.y,
                    i32::try_from(size.width).unwrap_or(i32::MAX),
                    i32::try_from(size.height).unwrap_or(i32::MAX),
                )
            })
            .collect(),
        Err(err) => {
            log::warn!("monitors could not be enumerated: {err}");
            Vec::new()
        }
    }
}

/// A plain window for the first enabled provider that renders its payload
/// as JSON, standing in for the real bubble window.
fn create_test_window(app: &App, provider: ProviderId) -> tauri::Result<()> {
    WebviewWindowBuilder::new(
        app,
        bubble_label(provider),
        WebviewUrl::App("bubble.html".into()),
    )
    .title("Claude Code Usage Bubble")
    .inner_size(460.0, 420.0)
    .always_on_top(true)
    .skip_taskbar(true)
    .build()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(v: &str) -> Option<OsString> {
        Some(OsString::from(v))
    }

    #[test]
    fn x11_backend_only_under_wayland_with_xwayland_and_no_override() {
        assert!(needs_x11_backend(os("wayland-0"), None, os(":0")));
        assert!(needs_x11_backend(os("wayland-0"), os(""), os(":0")));
        assert!(!needs_x11_backend(os("wayland-0"), os("wayland"), os(":0")));
        assert!(!needs_x11_backend(None, None, os(":0")));
        assert!(!needs_x11_backend(os(""), None, os(":0")));
        // No X server: forcing x11 would stop GTK from starting.
        assert!(!needs_x11_backend(os("wayland-0"), None, None));
    }

    #[test]
    fn canvas_follows_the_scale_factor() {
        assert_eq!(
            bubble_canvas(200, 1.0),
            Canvas {
                w: 200,
                h: 66,
                dpi: 96
            }
        );
        assert_eq!(
            bubble_canvas(200, 1.5),
            Canvas {
                w: 300,
                h: 99,
                dpi: 144
            }
        );
        assert_eq!(bubble_canvas(200, 1.25).dpi, 120);
        assert_eq!(bubble_canvas(200, f64::NAN).dpi, 96);
        assert_eq!(bubble_canvas(200, 0.0).dpi, 96);
    }

    #[test]
    fn enabled_providers_keep_display_order_and_skip_non_live_ones() {
        let mut settings = Settings {
            show_codex: true,
            show_opencode_go: true,
            ..Settings::default()
        };
        assert_eq!(
            enabled_live_providers(&settings),
            [ProviderId::Claude, ProviderId::ChatGpt]
        );
        settings.show_claude_code = false;
        assert_eq!(enabled_live_providers(&settings), [ProviderId::ChatGpt]);
    }

    #[test]
    fn bubble_labels_use_the_slug() {
        assert_eq!(bubble_label(ProviderId::ChatGpt), "bubble-chatgpt");
    }

    #[test]
    fn auth_errors_become_auth_failures() {
        assert!(matches!(
            poll_outcome(ProviderId::Claude, Err(usage::Error::TokenExpired)),
            PollOutcome::AuthFailed
        ));
        assert!(matches!(
            poll_outcome(ProviderId::Claude, Err(usage::Error::AuthRequired)),
            PollOutcome::AuthFailed
        ));
        assert!(matches!(
            poll_outcome(ProviderId::ChatGpt, Err(usage::Error::NoCredentials)),
            PollOutcome::Failed
        ));
    }

    #[test]
    fn snapshot_payload_uses_the_window_canvas() {
        let state = AppState {
            settings: Settings::default(),
            strings: I18n::load().strings().clone(),
            is_dark: true,
            usage: UsageState::default(),
            update_status: UpdateStatus::Idle,
            pending_update_alert: None,
        };
        let snapshot = BubbleSnapshot::of(&state);
        assert_eq!(snapshot.providers, [ProviderId::Claude]);
        let p = snapshot.payload(ProviderId::Claude, 2.0, SystemTime::now());
        assert_eq!((p.canvas_w, p.canvas_h, p.dpi), (400, 132, 192));
        assert_eq!(p.head_label, "…");
    }
}
