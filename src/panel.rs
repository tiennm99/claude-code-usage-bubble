//! The details panel: one opaque, undecorated, always-on-top webview
//! labelled `panel`, created hidden at startup and then only shown and
//! hidden, never destroyed.
//!
//! A bubble click toggles it for that bubble's provider, placed below the
//! bubble (or above it near the bottom edge). Losing focus hides it. While
//! it is open, every state push refreshes it in place.
//!
//! The page (`ui/panel.js`) only draws the [`PanelPayload`]. Every Tauri
//! call here runs on the main thread, which also serialises a toggle
//! against a refresh, and no function holds a lock across a Tauri call.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use tauri::window::Color;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Window, WindowEvent,
};

use crate::core::geom::{self, Point, Rect};
use crate::core::payload::PanelPayload;
use crate::platform;
use crate::runtime::{self, Runtime};
use crate::usage::ProviderId;

/// Window label of the panel.
pub const PANEL_LABEL: &str = "panel";

/// Event carrying a [`PanelPayload`] to the panel webview.
pub const PANEL_UPDATE_EVENT: &str = "panel:update";

const TITLE: &str = "Usage panel";
const PANEL_W_LOGICAL: i32 = 280;
const PANEL_H_LOGICAL: i32 = 120;

/// A click that reopens the panel this soon after focus loss hid it, for
/// the same provider, is the click that caused the focus loss: it closes
/// the panel instead of reopening it.
const BLUR_REOPEN_GUARD: Duration = Duration::from_millis(150);

/// Panel backgrounds, matching `ui/panel.css`, so a show never flashes white.
const DARK_BG: Color = Color(0x1F, 0x1F, 0x1F, 0xFF);
const LIGHT_BG: Color = Color(0xFA, 0xFA, 0xFA, 0xFF);

/// What the panel shows. Only the main thread changes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PanelState {
    /// The provider on screen, `None` while the panel is hidden.
    shown: Option<ProviderId>,
    /// The provider last shown, so a reloaded page can ask for its data.
    provider: Option<ProviderId>,
    /// The provider and time of the last hide caused by focus loss.
    blur_hide: Option<(ProviderId, Instant)>,
}

static STATE: Mutex<PanelState> = Mutex::new(PanelState {
    shown: None,
    provider: None,
    blur_hide: None,
});

fn with_state<T>(f: impl FnOnce(&mut PanelState) -> T) -> T {
    let mut guard = STATE.lock().unwrap_or_else(PoisonError::into_inner);
    f(&mut guard)
}

/// What a bubble click does to the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Toggle {
    Show,
    Hide,
    /// The click already closed the panel through focus loss.
    Ignore,
}

/// Decide a click on `provider`'s bubble and record the result in `state`.
fn toggle_state(state: &mut PanelState, provider: ProviderId, now: Instant) -> Toggle {
    if state.shown == Some(provider) {
        state.shown = None;
        return Toggle::Hide;
    }
    let just_blurred = state.shown.is_none()
        && state.blur_hide.is_some_and(|(p, at)| {
            p == provider && now.saturating_duration_since(at) < BLUR_REOPEN_GUARD
        });
    // Either way the guard is spent, so a later click opens the panel.
    state.blur_hide = None;
    if just_blurred {
        return Toggle::Ignore;
    }
    state.shown = Some(provider);
    state.provider = Some(provider);
    Toggle::Show
}

/// Record a hide caused by focus loss. Returns whether the panel was shown.
fn blur_state(state: &mut PanelState, now: Instant) -> bool {
    match state.shown.take() {
        Some(provider) => {
            state.blur_hide = Some((provider, now));
            true
        }
        None => false,
    }
}

/// The provider on screen, `None` while the panel is hidden.
pub fn shown_provider() -> Option<ProviderId> {
    with_state(|s| s.shown)
}

/// The provider the panel last showed, hidden or not.
pub fn current_provider() -> Option<ProviderId> {
    with_state(|s| s.provider)
}

fn runtime_of(app: &AppHandle) -> Option<Arc<Runtime>> {
    app.try_state::<Arc<Runtime>>().map(|r| Arc::clone(&r))
}

fn background(is_dark: bool) -> Color {
    if is_dark {
        DARK_BG
    } else {
        LIGHT_BG
    }
}

/// Create the hidden panel window. Called once at startup.
pub fn create(app: &AppHandle, is_dark: bool) -> tauri::Result<()> {
    let builder = WebviewWindowBuilder::new(app, PANEL_LABEL, WebviewUrl::App("panel.html".into()))
        .title(TITLE)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .transparent(false)
        .shadow(false)
        .visible(false)
        .background_color(background(is_dark))
        .inner_size(f64::from(PANEL_W_LOGICAL), f64::from(PANEL_H_LOGICAL));
    #[cfg(windows)]
    let builder = builder.on_page_load(disable_browser_keys_after_first_load());
    let window = builder.build()?;
    prepare_native(app, &window);
    log::info!("{PANEL_LABEL} created");
    Ok(())
}

/// Windows: a page-load hook that turns off the webview's browser keys
/// once `panel.html` has finished its first load. The setting only applies
/// from the next navigation, so it comes with a reload; waiting for the
/// first load keeps that reload from racing the initial navigation, and the
/// flag keeps the reload's own load from repeating it. The work leaves the
/// navigation callback through another thread, which queues it on the main
/// thread for after the callback returns.
#[cfg(windows)]
fn disable_browser_keys_after_first_load(
) -> impl Fn(WebviewWindow, tauri::webview::PageLoadPayload<'_>) + Send + Sync + 'static {
    use std::sync::atomic::{AtomicBool, Ordering};
    use tauri::webview::PageLoadEvent;

    let done = AtomicBool::new(false);
    move |window, payload| {
        if payload.event() != PageLoadEvent::Finished
            || payload.url().scheme() == "about"
            || done.swap(true, Ordering::SeqCst)
        {
            return;
        }
        std::thread::spawn(move || {
            if let Err(err) = platform::windows::disable_browser_accelerator_keys(&window) {
                log::warn!("{PANEL_LABEL} keeps the browser accelerator keys: {err}");
            }
        });
    }
}

/// Windows: drop the panel from Alt-Tab, on the window's own (main)
/// thread, before the first show.
#[cfg(windows)]
fn prepare_native(app: &AppHandle, window: &WebviewWindow) {
    let window = window.clone();
    let queued = app.run_on_main_thread(move || {
        if let Err(err) = platform::windows::apply_toolwindow(&window) {
            log::warn!("{PANEL_LABEL} may show in Alt-Tab: {err}");
        }
    });
    if let Err(err) = queued {
        log::warn!("panel set-up could not reach the main thread: {err}");
    }
}

#[cfg(not(windows))]
fn prepare_native(_app: &AppHandle, _window: &WebviewWindow) {}

/// A click on `provider`'s bubble: hide the panel when it shows that
/// provider, otherwise move it next to the bubble and show that provider.
pub fn toggle(app: &AppHandle, provider: ProviderId) {
    let handle = app.clone();
    let queued = app.run_on_main_thread(move || toggle_on_main(&handle, provider));
    if let Err(err) = queued {
        log::warn!("panel toggle could not reach the main thread: {err}");
    }
}

fn toggle_on_main(app: &AppHandle, provider: ProviderId) {
    let Some(window) = app.get_webview_window(PANEL_LABEL) else {
        log::warn!("bubble click on {} but no panel exists", provider.slug());
        return;
    };
    match with_state(|s| toggle_state(s, provider, Instant::now())) {
        Toggle::Hide => hide(&window),
        Toggle::Ignore => log::debug!("panel just closed by this click; not reopened"),
        Toggle::Show => show_for(app, &window, provider),
    }
}

/// Fill the panel with `provider`'s data, place it next to that bubble and
/// show it with focus, so that losing focus can dismiss it.
fn show_for(app: &AppHandle, window: &WebviewWindow, provider: ProviderId) {
    let Some(runtime) = runtime_of(app) else {
        log::warn!("{PANEL_LABEL} not shown: the runtime is not ready");
        return;
    };
    let payload = runtime.panel_payload(provider);
    place_near_bubble(app, window, provider);
    if let Err(err) = window.set_background_color(Some(background(payload.is_dark))) {
        log::debug!("{PANEL_LABEL} background not set: {err}");
    }
    emit(app, &payload);
    #[cfg(windows)]
    if let Err(err) = platform::windows::round_corners(window) {
        // Windows 10 has no rounded corners.
        log::debug!("{PANEL_LABEL} keeps square corners: {err}");
    }
    if let Err(err) = window.show() {
        log::warn!("{PANEL_LABEL} not shown: {err}");
    }
    if let Err(err) = window.set_focus() {
        log::debug!("{PANEL_LABEL} not focused: {err}");
    }
}

/// Size the panel for the bubble's DPI and place it next to the bubble,
/// inside the union of all monitors. Without a bubble it stays where it is.
fn place_near_bubble(app: &AppHandle, window: &WebviewWindow, provider: ProviderId) {
    let Some(bubble) = app.get_webview_window(&runtime::bubble_label(provider)) else {
        log::warn!("no bubble for {}; panel not moved", provider.slug());
        return;
    };
    let (Ok(pos), Ok(size), Ok(scale)) = (
        bubble.outer_position(),
        bubble.outer_size(),
        bubble.scale_factor(),
    ) else {
        log::warn!(
            "bounds of the {} bubble unavailable; panel not moved",
            provider.slug()
        );
        return;
    };
    let dpi = runtime::dpi_for_scale(scale);
    let panel_size = (
        geom::scale(PANEL_W_LOGICAL, dpi),
        geom::scale(PANEL_H_LOGICAL, dpi),
    );
    let physical = PhysicalSize::new(
        panel_size.0.max(1).unsigned_abs(),
        panel_size.1.max(1).unsigned_abs(),
    );
    if !platform::positioning_supported() {
        if let Err(err) = window.set_size(physical) {
            log::warn!("{PANEL_LABEL} not resized: {err}");
        }
        return;
    }
    let anchor = Rect::from_xywh(
        pos.x,
        pos.y,
        i32::try_from(size.width).unwrap_or(i32::MAX),
        i32::try_from(size.height).unwrap_or(i32::MAX),
    );
    let bounds = virtual_screen(&runtime::monitor_rects(app)).unwrap_or(anchor);
    let target = geom::place_near(anchor, panel_size, bounds);
    // Move first so a DPI change from crossing monitors resizes the window
    // before the explicit size, then place it again at the final size.
    set_position(window, target);
    if let Err(err) = window.set_size(physical) {
        log::warn!("{PANEL_LABEL} not resized: {err}");
    }
    set_position(window, target);
}

fn set_position(window: &WebviewWindow, pos: Point) {
    if let Err(err) = window.set_position(PhysicalPosition::new(pos.x, pos.y)) {
        log::warn!("{PANEL_LABEL} not moved: {err}");
    }
}

/// The bounding rect of all monitors, `None` without any.
fn virtual_screen(monitors: &[Rect]) -> Option<Rect> {
    monitors
        .iter()
        .copied()
        .reduce(|a, b| Rect::new(a.l.min(b.l), a.t.min(b.t), a.r.max(b.r), a.b.max(b.b)))
}

fn hide(window: &WebviewWindow) {
    if let Err(err) = window.hide() {
        log::warn!("{PANEL_LABEL} not hidden: {err}");
    }
}

fn emit(app: &AppHandle, payload: &PanelPayload) {
    if let Err(err) = app.emit_to(PANEL_LABEL, PANEL_UPDATE_EVENT, payload) {
        log::warn!("emit {PANEL_UPDATE_EVENT} failed: {err}");
    }
}

/// Update the open panel in place, without moving it, when it shows
/// `payload`'s provider. Called after every state push, outside the state
/// lock; the check runs on the main thread, after any queued toggle.
pub fn refresh_if_visible(app: &AppHandle, payload: PanelPayload) {
    let handle = app.clone();
    let queued = app.run_on_main_thread(move || {
        if shown_provider() == Some(payload.provider) {
            emit(&handle, &payload);
        }
    });
    if let Err(err) = queued {
        log::warn!("panel refresh could not reach the main thread: {err}");
    }
}

/// Handle the panel's window events; other windows are ignored.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != PANEL_LABEL {
        return;
    }
    match event {
        // Alt+F4 hides the panel; it is never destroyed.
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            with_state(|s| s.shown = None);
            if let Err(err) = window.hide() {
                log::warn!("{PANEL_LABEL} not hidden: {err}");
            }
        }
        WindowEvent::Focused(false) => on_blur(window),
        _ => {}
    }
}

/// Focus left the panel: hide it if it was open, and remember when, so
/// the bubble click that took the focus does not reopen it.
fn on_blur(window: &Window) {
    if !with_state(|s| blur_state(s, Instant::now())) {
        return;
    }
    if let Err(err) = window.hide() {
        log::warn!("{PANEL_LABEL} not hidden: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HIDDEN: PanelState = PanelState {
        shown: None,
        provider: None,
        blur_hide: None,
    };

    #[test]
    fn a_click_opens_the_hidden_panel() {
        let mut state = HIDDEN;
        let now = Instant::now();
        assert_eq!(
            toggle_state(&mut state, ProviderId::Claude, now),
            Toggle::Show
        );
        assert_eq!(state.shown, Some(ProviderId::Claude));
        assert_eq!(state.provider, Some(ProviderId::Claude));
    }

    #[test]
    fn a_second_click_on_the_same_bubble_closes_it() {
        let mut state = HIDDEN;
        let now = Instant::now();
        toggle_state(&mut state, ProviderId::Claude, now);
        assert_eq!(
            toggle_state(&mut state, ProviderId::Claude, now),
            Toggle::Hide
        );
        assert_eq!(state.shown, None);
        // The last provider is kept for a reloaded page.
        assert_eq!(state.provider, Some(ProviderId::Claude));
    }

    #[test]
    fn a_click_on_the_other_bubble_swaps_the_content() {
        let mut state = HIDDEN;
        let now = Instant::now();
        toggle_state(&mut state, ProviderId::Claude, now);
        assert_eq!(
            toggle_state(&mut state, ProviderId::ChatGpt, now),
            Toggle::Show
        );
        assert_eq!(state.shown, Some(ProviderId::ChatGpt));
    }

    #[test]
    fn a_click_right_after_a_blur_hide_does_not_reopen() {
        let mut state = HIDDEN;
        let t0 = Instant::now();
        toggle_state(&mut state, ProviderId::Claude, t0);
        assert!(blur_state(&mut state, t0));
        assert_eq!(state.shown, None);
        assert_eq!(
            toggle_state(
                &mut state,
                ProviderId::Claude,
                t0 + Duration::from_millis(149)
            ),
            Toggle::Ignore
        );
        assert_eq!(state.shown, None);
        // The guard is spent: the next click opens the panel again.
        assert_eq!(
            toggle_state(
                &mut state,
                ProviderId::Claude,
                t0 + Duration::from_millis(160)
            ),
            Toggle::Show
        );
    }

    #[test]
    fn the_blur_guard_expires_and_ignores_other_providers() {
        let t0 = Instant::now();
        let mut state = HIDDEN;
        toggle_state(&mut state, ProviderId::Claude, t0);
        blur_state(&mut state, t0);
        assert_eq!(
            toggle_state(&mut state, ProviderId::Claude, t0 + BLUR_REOPEN_GUARD),
            Toggle::Show
        );

        let mut state = HIDDEN;
        toggle_state(&mut state, ProviderId::Claude, t0);
        blur_state(&mut state, t0);
        assert_eq!(
            toggle_state(
                &mut state,
                ProviderId::ChatGpt,
                t0 + Duration::from_millis(10)
            ),
            Toggle::Show
        );
        assert_eq!(state.shown, Some(ProviderId::ChatGpt));
    }

    #[test]
    fn a_blur_while_hidden_records_nothing() {
        let mut state = HIDDEN;
        assert!(!blur_state(&mut state, Instant::now()));
        assert_eq!(state.blur_hide, None);
    }

    #[test]
    fn virtual_screen_is_the_union_of_the_monitors() {
        assert_eq!(virtual_screen(&[]), None);
        let primary = Rect::new(0, 0, 1920, 1080);
        assert_eq!(virtual_screen(&[primary]), Some(primary));
        let left_above = Rect::new(-1920, -200, 0, 880);
        assert_eq!(
            virtual_screen(&[primary, left_above]),
            Some(Rect::new(-1920, -200, 1920, 1080))
        );
    }

    #[test]
    fn panel_backgrounds_match_the_palette() {
        assert_eq!(background(true), Color(0x1F, 0x1F, 0x1F, 0xFF));
        assert_eq!(background(false), Color(0xFA, 0xFA, 0xFA, 0xFF));
    }
}
