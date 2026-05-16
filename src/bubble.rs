// Floating rounded-rectangle bubble window.
//
// Top-level window with WS_POPUP + WS_EX_LAYERED + WS_EX_TOPMOST + WS_EX_NOACTIVATE.
// Shape is achieved via per-pixel alpha (alpha=0 outside the rounded rect) and
// confirmed via WM_NCHITTEST returning HTCAPTION inside the rect, HTTRANSPARENT
// outside. The OS handles drag automatically because HTCAPTION inside the
// rect puts the click into the system move loop.
//
// Layout: two horizontal bars stacked vertically — top = session (5h), bottom =
// weekly (7d) — each followed by right-aligned "X% · Yh Zm" text. The aspect
// ratio is fixed at BUBBLE_ASPECT (3:1) so `size_logical` is interpreted as
// width and the height is derived.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Mutex, MutexGuard, OnceLock};

use windows::core::PCWSTR;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Shell::{
    ExtractIconExW, SHAppBarMessage, ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_GETTASKBARPOS,
    APPBARDATA,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::os::{to_utf16_nul as wide_str, Rgb as Color};

const TIMER_FULLSCREEN_CHECK: usize = 5;
const TIMER_PULSE: usize = 6;
const PULSE_INTERVAL_MS: u32 = 80;
use crate::usage::ProviderId;
type TrayIconKind = ProviderId;

// ---------- Public types & API ----------

// Width clamps in logical pixels. Height is derived per width (see
// `bubble_height_logical`) — aspect tapers from 3:1 at the small end toward
// 2.6:1 at the large end so the bars look proportionally chunkier as the
// bubble grows.
pub const MIN_BUBBLE_SIZE: i32 = 140;
pub const MAX_BUBBLE_SIZE: i32 = 360;
pub const DEFAULT_BUBBLE_SIZE: i32 = 200;
const RESIZE_STEP: i32 = 20;
const SNAP_ZONE_LOGICAL: i32 = 12;
const CORNER_SNAP_ZONE_LOGICAL: i32 = 32;
const CORNER_INSET_LOGICAL: i32 = 12;
const TASKBAR_GAP_LOGICAL: i32 = 4;
const PEER_ALIGN_TOLERANCE_LOGICAL: i32 = 8;
const CLASS_NAME: &str = "ClaudeCodeUsageBubble";
const FULLSCREEN_POLL_MS: u32 = 1500;

/// Per-width breakpoint defining bar height, text font size, and row gap in
/// *logical* pixels. Picked so that the smallest bubble is still legible and
/// the largest doesn't waste vertical space.
#[derive(Clone, Copy)]
struct Breakpoint {
    bar_h: i32,
    font: i32,
    row_gap: i32,
}

fn breakpoint_for_width_logical(w: i32) -> Breakpoint {
    if w <= 140 {
        Breakpoint { bar_h: 12, font: 11, row_gap: 4 }
    } else if w <= 200 {
        Breakpoint { bar_h: 16, font: 13, row_gap: 6 }
    } else if w <= 280 {
        Breakpoint { bar_h: 20, font: 15, row_gap: 8 }
    } else {
        Breakpoint { bar_h: 24, font: 17, row_gap: 10 }
    }
}

/// (num, den) such that bubble_height = (width * den) / num. 3:1 below 200,
/// 2.8:1 below 280, 2.6:1 above — the bubble gets a touch taller as it
/// grows so the wider bars don't look anaemic.
fn aspect_at_width(w_logical: i32) -> (i32, i32) {
    if w_logical <= 200 {
        (3, 1)
    } else if w_logical <= 280 {
        (14, 5) // 2.8 : 1
    } else {
        (13, 5) // 2.6 : 1
    }
}

pub struct BubbleConfig {
    pub model: TrayIconKind,
    pub size_logical: i32,
    pub position: Option<(i32, i32)>,
    pub session_pct: Option<f64>,
    pub session_text: String,
    pub weekly_pct: Option<f64>,
    pub weekly_text: String,
    pub is_dark: bool,
}

fn bubble_height_logical(width_logical: i32) -> i32 {
    let (num, den) = aspect_at_width(width_logical);
    ((width_logical * den) / num).max(20)
}

/// Register the bubble window class. Idempotent; safe to call before the first
/// `create()` from the UI thread.
pub fn register_class() {
    static REGISTERED: OnceLock<()> = OnceLock::new();
    REGISTERED.get_or_init(|| unsafe {
        let class_w = wide_str(CLASS_NAME);
        let hinstance = GetModuleHandleW(PCWSTR::null()).unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: HINSTANCE(hinstance.0),
            hCursor: LoadCursorW(HINSTANCE::default(), IDC_SIZEALL).unwrap_or_default(),
            hbrBackground: HBRUSH(std::ptr::null_mut()),
            lpszClassName: PCWSTR::from_raw(class_w.as_ptr()),
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            log::error!("bubble RegisterClassExW returned 0");
        }
    });
}

/// Create a bubble window. Returns the HWND. The caller (app::run) owns the
/// message-loop dispatch.
pub fn create(config: BubbleConfig) -> HWND {
    register_class();
    let initial_size_logical = config
        .size_logical
        .clamp(MIN_BUBBLE_SIZE, MAX_BUBBLE_SIZE);
    let hwnd = unsafe {
        let class_w = wide_str(CLASS_NAME);
        let title_w = wide_str("Claude Code Usage Bubble");
        let hinstance = GetModuleHandleW(PCWSTR::null()).unwrap_or_default();
        let dpi = primary_dpi();
        let width_px = scale_to_dpi(initial_size_logical, dpi);
        let height_px = scale_to_dpi(bubble_height_logical(initial_size_logical), dpi);
        let (x, y) = config
            .position
            .unwrap_or_else(|| default_position(width_px, height_px, config.model));
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            PCWSTR::from_raw(class_w.as_ptr()),
            PCWSTR::from_raw(title_w.as_ptr()),
            WS_POPUP,
            x,
            y,
            width_px,
            height_px,
            HWND::default(),
            HMENU::default(),
            hinstance,
            None,
        )
        .unwrap_or_default()
    };

    if hwnd == HWND::default() {
        log::error!("bubble CreateWindowExW failed");
        return hwnd;
    }

    // Embed app icon in window non-client (mostly cosmetic; toolwindows
    // don't show captions but the icon helps in dev tooling).
    unsafe {
        let mut large_icon = HICON::default();
        let mut small_icon = HICON::default();
        let mut exe = [0u16; 260];
        GetModuleFileNameW(HMODULE::default(), &mut exe);
        let _ = ExtractIconExW(
            PCWSTR::from_raw(exe.as_ptr()),
            0,
            Some(&mut large_icon),
            Some(&mut small_icon),
            1,
        );
        if !large_icon.is_invalid() {
            let _ = SendMessageW(
                hwnd,
                WM_SETICON,
                WPARAM(ICON_BIG as usize),
                LPARAM(large_icon.0 as isize),
            );
        }
        if !small_icon.is_invalid() {
            let _ = SendMessageW(
                hwnd,
                WM_SETICON,
                WPARAM(ICON_SMALL as usize),
                LPARAM(small_icon.0 as isize),
            );
        }
    }

    let dpi = unsafe { GetDpiForWindow(hwnd).max(96) };
    lock_bubbles().insert(
        hwnd.0 as isize,
        BubbleState {
            model: config.model,
            size_logical: initial_size_logical,
            dpi,
            session_pct: config.session_pct,
            session_text: config.session_text,
            weekly_pct: config.weekly_pct,
            weekly_text: config.weekly_text,
            is_dark: config.is_dark,
            drag_start_pos: None,
            hidden_by_fullscreen: false,
            user_hidden: false,
            pulse_phase: 0,
            pulse_timer_armed: false,
        },
    );

    render(hwnd);
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        // Periodic fullscreen-foreground check.
        SetTimer(hwnd, TIMER_FULLSCREEN_CHECK, FULLSCREEN_POLL_MS, None);
    }

    hwnd
}

pub fn destroy(hwnd: HWND) {
    unsafe {
        let _ = KillTimer(hwnd, TIMER_FULLSCREEN_CHECK);
        let _ = KillTimer(hwnd, TIMER_PULSE);
        let _ = DestroyWindow(hwnd);
    }
}

pub fn update_data(
    hwnd: HWND,
    session_pct: Option<f64>,
    session_text: String,
    weekly_pct: Option<f64>,
    weekly_text: String,
) {
    {
        let mut bubbles = lock_bubbles();
        let Some(b) = bubbles.get_mut(&(hwnd.0 as isize)) else {
            return;
        };
        b.session_pct = session_pct;
        b.session_text = session_text;
        b.weekly_pct = weekly_pct;
        b.weekly_text = weekly_text;
    }
    sync_pulse_timer(hwnd);
    render(hwnd);
}

fn any_pct_in_alarm(b: &BubbleState) -> bool {
    b.session_pct.is_some_and(|p| p >= 95.0) || b.weekly_pct.is_some_and(|p| p >= 95.0)
}

fn sync_pulse_timer(hwnd: HWND) {
    let (should_be_armed, currently_armed) = {
        let bubbles = lock_bubbles();
        let Some(b) = bubbles.get(&(hwnd.0 as isize)) else {
            return;
        };
        (any_pct_in_alarm(b), b.pulse_timer_armed)
    };
    if should_be_armed == currently_armed {
        return;
    }
    unsafe {
        if should_be_armed {
            SetTimer(hwnd, TIMER_PULSE, PULSE_INTERVAL_MS, None);
        } else {
            let _ = KillTimer(hwnd, TIMER_PULSE);
        }
    }
    if let Some(b) = lock_bubbles().get_mut(&(hwnd.0 as isize)) {
        b.pulse_timer_armed = should_be_armed;
        if !should_be_armed {
            b.pulse_phase = 0;
        }
    }
}

pub fn update_dark_mode(hwnd: HWND, is_dark: bool) {
    {
        let mut bubbles = lock_bubbles();
        let Some(b) = bubbles.get_mut(&(hwnd.0 as isize)) else {
            return;
        };
        b.is_dark = is_dark;
    }
    render(hwnd);
}

pub fn set_user_visible(hwnd: HWND, visible: bool) {
    {
        let mut bubbles = lock_bubbles();
        let Some(b) = bubbles.get_mut(&(hwnd.0 as isize)) else {
            return;
        };
        b.user_hidden = !visible;
    }
    unsafe {
        let cmd = if visible { SW_SHOWNOACTIVATE } else { SW_HIDE };
        let _ = ShowWindow(hwnd, cmd);
    }
}

pub fn position(hwnd: HWND) -> Option<(i32, i32)> {
    let mut r = RECT::default();
    unsafe {
        if GetWindowRect(hwnd, &mut r).is_err() {
            return None;
        }
    }
    Some((r.left, r.top))
}

pub fn model(hwnd: HWND) -> Option<TrayIconKind> {
    lock_bubbles()
        .get(&(hwnd.0 as isize))
        .map(|b| b.model)
}

pub fn size_logical(hwnd: HWND) -> Option<i32> {
    lock_bubbles()
        .get(&(hwnd.0 as isize))
        .map(|b| b.size_logical)
}

// ---------- State ----------

struct BubbleState {
    model: TrayIconKind,
    size_logical: i32,
    dpi: u32,
    session_pct: Option<f64>,
    session_text: String,
    weekly_pct: Option<f64>,
    weekly_text: String,
    is_dark: bool,
    drag_start_pos: Option<(i32, i32)>,
    hidden_by_fullscreen: bool,
    user_hidden: bool,
    /// Frame counter for the ≥95% pulse animation. Increments on each
    /// TIMER_PULSE tick when at least one bar is in the alarm band.
    pulse_phase: u32,
    /// Whether TIMER_PULSE is currently armed for this bubble.
    pulse_timer_armed: bool,
}

fn bubbles() -> &'static Mutex<HashMap<isize, BubbleState>> {
    static BUBBLES: OnceLock<Mutex<HashMap<isize, BubbleState>>> = OnceLock::new();
    BUBBLES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_bubbles() -> MutexGuard<'static, HashMap<isize, BubbleState>> {
    bubbles().lock().expect("bubble state mutex poisoned")
}

// ---------- Window proc ----------

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCHITTEST => hit_test(hwnd, lparam),
        WM_ENTERSIZEMOVE => {
            let mut r = RECT::default();
            let _ = GetWindowRect(hwnd, &mut r);
            if let Some(b) = lock_bubbles().get_mut(&(hwnd.0 as isize)) {
                b.drag_start_pos = Some((r.left, r.top));
            }
            LRESULT(0)
        }
        WM_EXITSIZEMOVE => {
            // WM_NCLBUTTONUP isn't reliably delivered for HTCAPTION drags; instead
            // we infer click-vs-drag from whether the window actually moved.
            let start = {
                let mut bubbles = lock_bubbles();
                let start = bubbles
                    .get(&(hwnd.0 as isize))
                    .and_then(|b| b.drag_start_pos);
                if let Some(b) = bubbles.get_mut(&(hwnd.0 as isize)) {
                    b.drag_start_pos = None;
                }
                start
            };
            let mut current = RECT::default();
            let _ = GetWindowRect(hwnd, &mut current);
            let moved = match start {
                Some((sx, sy)) => (current.left - sx).abs() >= 3 || (current.top - sy).abs() >= 3,
                None => false,
            };
            if moved {
                snap_to_edge(hwnd);
                if let Some(model) = model(hwnd) {
                    if let Some(pos) = position(hwnd) {
                        crate::app::on_bubble_moved(model, pos);
                    }
                }
            } else if let Some(model) = model(hwnd) {
                crate::app::on_bubble_click(hwnd, model);
            }
            LRESULT(0)
        }
        WM_NCRBUTTONUP => {
            if let Some(model) = model(hwnd) {
                let pt = lparam_to_point(lparam);
                crate::app::on_bubble_right_click(hwnd, model, pt);
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let modifiers = (wparam.0 & 0xFFFF) as u32;
            const MK_CONTROL: u32 = 0x0008;
            if modifiers & MK_CONTROL != 0 {
                let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
                let step = if delta > 0 { RESIZE_STEP } else { -RESIZE_STEP };
                resize_step(hwnd, step);
                LRESULT(0)
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        WM_DPICHANGED => {
            let new_dpi = ((wparam.0 >> 16) & 0xFFFF) as u32;
            if let Some(b) = lock_bubbles().get_mut(&(hwnd.0 as isize)) {
                b.dpi = new_dpi;
            }
            let rect_ptr = lparam.0 as *const RECT;
            if !rect_ptr.is_null() {
                let r = *rect_ptr;
                let _ = SetWindowPos(
                    hwnd,
                    HWND::default(),
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            render(hwnd);
            LRESULT(0)
        }
        WM_TIMER => {
            match wparam.0 {
                w if w == TIMER_FULLSCREEN_CHECK => check_fullscreen(hwnd),
                w if w == TIMER_PULSE => {
                    if let Some(b) = lock_bubbles().get_mut(&(hwnd.0 as isize)) {
                        b.pulse_phase = b.pulse_phase.wrapping_add(1);
                    }
                    render(hwnd);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            crate::app::on_menu_command(wparam.0 as u32, hwnd);
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            // Taskbar move / auto-hide toggle / DPI change all post this.
            // Just re-clamp into the new work area so the bubble can't end up
            // hidden behind the new taskbar position.
            clamp_into_work_area(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            lock_bubbles().remove(&(hwnd.0 as isize));
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn hit_test(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let pt = lparam_to_point(lparam);
    let mut r = RECT::default();
    unsafe {
        if GetWindowRect(hwnd, &mut r).is_err() {
            return LRESULT(HTNOWHERE as isize);
        }
    }
    let w = r.right - r.left;
    let h = r.bottom - r.top;
    let radius = corner_radius_px(h);
    // Local coordinates relative to top-left of the bubble.
    let lx = pt.x - r.left;
    let ly = pt.y - r.top;
    if point_in_rounded_rect(lx, ly, w, h, radius) {
        LRESULT(HTCAPTION as isize)
    } else {
        LRESULT(HTTRANSPARENT as isize)
    }
}

fn corner_radius_px(height_px: i32) -> i32 {
    (height_px / 3).max(4)
}

fn point_in_rounded_rect(x: i32, y: i32, w: i32, h: i32, r: i32) -> bool {
    if x < 0 || y < 0 || x >= w || y >= h {
        return false;
    }
    // The straight horizontal and vertical strips are always inside; only the
    // four corner squares need the circular falloff check.
    let in_x_strip = x >= r && x < w - r;
    let in_y_strip = y >= r && y < h - r;
    if in_x_strip || in_y_strip {
        return true;
    }
    let cx = if x < r { r } else { w - 1 - r };
    let cy = if y < r { r } else { h - 1 - r };
    let dx = x - cx;
    let dy = y - cy;
    dx * dx + dy * dy <= r * r
}

fn lparam_to_point(lparam: LPARAM) -> POINT {
    let lo = (lparam.0 & 0xFFFF) as i16 as i32;
    let hi = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
    POINT { x: lo, y: hi }
}

// ---------- Resize / snap ----------

fn resize_step(hwnd: HWND, delta: i32) {
    let (new_logical, dpi) = {
        let mut bubbles = lock_bubbles();
        let Some(b) = bubbles.get_mut(&(hwnd.0 as isize)) else {
            return;
        };
        let new_logical = (b.size_logical + delta).clamp(MIN_BUBBLE_SIZE, MAX_BUBBLE_SIZE);
        if new_logical == b.size_logical {
            return;
        }
        b.size_logical = new_logical;
        (new_logical, b.dpi)
    };
    let width_px = scale_to_dpi(new_logical, dpi);
    let height_px = scale_to_dpi(bubble_height_logical(new_logical), dpi);
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut r);
        // Resize centered on existing center.
        let cx = (r.left + r.right) / 2;
        let cy = (r.top + r.bottom) / 2;
        let new_x = cx - width_px / 2;
        let new_y = cy - height_px / 2;
        let _ = SetWindowPos(
            hwnd,
            HWND::default(),
            new_x,
            new_y,
            width_px,
            height_px,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    render(hwnd);
    if let Some(m) = model(hwnd) {
        crate::app::on_bubble_resized(m, new_logical);
    }
}

fn snap_to_edge(hwnd: HWND) {
    let dpi = lock_bubbles()
        .get(&(hwnd.0 as isize))
        .map(|b| b.dpi)
        .unwrap_or(96);
    let edge_zone = scale_to_dpi(SNAP_ZONE_LOGICAL, dpi);
    let corner_zone = scale_to_dpi(CORNER_SNAP_ZONE_LOGICAL, dpi);
    let corner_inset = scale_to_dpi(CORNER_INSET_LOGICAL, dpi);
    let taskbar_gap = scale_to_dpi(TASKBAR_GAP_LOGICAL, dpi);
    let peer_tolerance = scale_to_dpi(PEER_ALIGN_TOLERANCE_LOGICAL, dpi);

    let mut r = RECT::default();
    let monitor;
    unsafe {
        if GetWindowRect(hwnd, &mut r).is_err() {
            return;
        }
        monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    }
    if monitor.is_invalid() {
        return;
    }
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
    }
    let wa = info.rcWork;
    let w = r.right - r.left;
    let h = r.bottom - r.top;
    let mut nx = r.left;
    let mut ny = r.top;

    // 1. Corner snap — if the bubble's nearest-corner distance is under the
    //    32-px corner zone, slam it into the corner with the 12-px inset.
    let snapped_to_corner = try_corner_snap(&mut nx, &mut ny, &wa, w, h, corner_zone, corner_inset);

    if !snapped_to_corner {
        // 2. Edge snap (existing behavior) — also handles taskbar-adjacency
        //    when the taskbar steals from the work area on the same edge.
        let taskbar = read_taskbar();
        snap_to_work_area_edges(&mut nx, &mut ny, &wa, w, h, edge_zone);
        if let Some(tb) = taskbar {
            snap_alongside_taskbar(&mut nx, &mut ny, &tb, &wa, w, h, edge_zone, taskbar_gap);
        }

        // 3. Peer vertical alignment — when the other bubble is within ±8 px
        //    on Y, snap the dragged bubble to share its baseline.
        align_with_peer(hwnd, &mut ny, peer_tolerance);
    }

    // Clamp into the work area in any case (so the bubble can't be lost off-screen).
    nx = nx.clamp(wa.left, (wa.right - w).max(wa.left));
    ny = ny.clamp(wa.top, (wa.bottom - h).max(wa.top));

    if nx != r.left || ny != r.top {
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                HWND::default(),
                nx,
                ny,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
}

fn try_corner_snap(
    nx: &mut i32,
    ny: &mut i32,
    wa: &RECT,
    w: i32,
    h: i32,
    zone: i32,
    inset: i32,
) -> bool {
    // Distance from each work-area corner to the bubble's nearest corner.
    let tl = (*nx - wa.left).abs() + (*ny - wa.top).abs();
    let tr = (wa.right - (*nx + w)).abs() + (*ny - wa.top).abs();
    let bl = (*nx - wa.left).abs() + (wa.bottom - (*ny + h)).abs();
    let br = (wa.right - (*nx + w)).abs() + (wa.bottom - (*ny + h)).abs();
    let min = tl.min(tr).min(bl).min(br);
    if min > zone * 2 {
        return false;
    }
    if min == tl {
        *nx = wa.left + inset;
        *ny = wa.top + inset;
    } else if min == tr {
        *nx = wa.right - inset - w;
        *ny = wa.top + inset;
    } else if min == bl {
        *nx = wa.left + inset;
        *ny = wa.bottom - inset - h;
    } else {
        *nx = wa.right - inset - w;
        *ny = wa.bottom - inset - h;
    }
    true
}

fn snap_to_work_area_edges(nx: &mut i32, ny: &mut i32, wa: &RECT, w: i32, h: i32, zone: i32) {
    if (*nx - wa.left).abs() < zone {
        *nx = wa.left;
    } else if (wa.right - (*nx + w)).abs() < zone {
        *nx = wa.right - w;
    }
    if (*ny - wa.top).abs() < zone {
        *ny = wa.top;
    } else if (wa.bottom - (*ny + h)).abs() < zone {
        *ny = wa.bottom - h;
    }
}

struct Taskbar {
    rect: RECT,
    edge: u32,
}

fn read_taskbar() -> Option<Taskbar> {
    let mut abd = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        ..Default::default()
    };
    let res = unsafe { SHAppBarMessage(ABM_GETTASKBARPOS, &mut abd) };
    if res == 0 {
        return None;
    }
    Some(Taskbar {
        rect: abd.rc,
        edge: abd.uEdge,
    })
}

fn snap_alongside_taskbar(
    nx: &mut i32,
    ny: &mut i32,
    tb: &Taskbar,
    wa: &RECT,
    w: i32,
    h: i32,
    zone: i32,
    gap: i32,
) {
    // Only snap on the taskbar's docked edge. The bubble docks against the
    // inner face of the taskbar with a 4-px gap so it visually leans on it.
    match tb.edge {
        e if e == ABE_BOTTOM => {
            let target = tb.rect.top - gap - h;
            if (*ny - target).abs() < zone {
                *ny = target.max(wa.top);
            }
        }
        e if e == ABE_TOP => {
            let target = tb.rect.bottom + gap;
            if (*ny - target).abs() < zone {
                *ny = target.min(wa.bottom - h);
            }
        }
        e if e == ABE_LEFT => {
            let target = tb.rect.right + gap;
            if (*nx - target).abs() < zone {
                *nx = target.min(wa.right - w);
            }
        }
        e if e == ABE_RIGHT => {
            let target = tb.rect.left - gap - w;
            if (*nx - target).abs() < zone {
                *nx = target.max(wa.left);
            }
        }
        _ => {}
    }
}

fn clamp_into_work_area(hwnd: HWND) {
    let mut r = RECT::default();
    let monitor;
    unsafe {
        if GetWindowRect(hwnd, &mut r).is_err() {
            return;
        }
        monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    }
    if monitor.is_invalid() {
        return;
    }
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
    }
    let wa = info.rcWork;
    let w = r.right - r.left;
    let h = r.bottom - r.top;
    let nx = r.left.clamp(wa.left, (wa.right - w).max(wa.left));
    let ny = r.top.clamp(wa.top, (wa.bottom - h).max(wa.top));
    if nx != r.left || ny != r.top {
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                HWND::default(),
                nx,
                ny,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
}

fn align_with_peer(this_hwnd: HWND, ny: &mut i32, tolerance: i32) {
    let bubbles = lock_bubbles();
    for (id, _) in bubbles.iter() {
        if *id == this_hwnd.0 as isize {
            continue;
        }
        let peer_hwnd = HWND(*id as *mut c_void);
        let mut pr = RECT::default();
        unsafe {
            if GetWindowRect(peer_hwnd, &mut pr).is_err() {
                continue;
            }
        }
        if (*ny - pr.top).abs() <= tolerance {
            *ny = pr.top;
            return;
        }
    }
}

// ---------- Fullscreen detection ----------

fn check_fullscreen(bubble_hwnd: HWND) {
    let fg = unsafe { GetForegroundWindow() };
    if fg == HWND::default() || fg == bubble_hwnd {
        return;
    }
    let mut fr = RECT::default();
    unsafe {
        if GetWindowRect(fg, &mut fr).is_err() {
            return;
        }
    }
    let monitor = unsafe { MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_invalid() {
        return;
    }
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let ok = unsafe { GetMonitorInfoW(monitor, &mut info).as_bool() };
    if !ok {
        return;
    }
    let mr = info.rcMonitor;
    let is_fullscreen =
        fr.left <= mr.left && fr.top <= mr.top && fr.right >= mr.right && fr.bottom >= mr.bottom;

    let (was_hidden_by_fs, user_hidden) = {
        let bubbles = lock_bubbles();
        let Some(b) = bubbles.get(&(bubble_hwnd.0 as isize)) else {
            return;
        };
        (b.hidden_by_fullscreen, b.user_hidden)
    };

    if is_fullscreen && !was_hidden_by_fs {
        unsafe {
            let _ = ShowWindow(bubble_hwnd, SW_HIDE);
        }
        if let Some(b) = lock_bubbles().get_mut(&(bubble_hwnd.0 as isize)) {
            b.hidden_by_fullscreen = true;
        }
    } else if !is_fullscreen && was_hidden_by_fs {
        if !user_hidden {
            unsafe {
                let _ = ShowWindow(bubble_hwnd, SW_SHOWNOACTIVATE);
            }
        }
        if let Some(b) = lock_bubbles().get_mut(&(bubble_hwnd.0 as isize)) {
            b.hidden_by_fullscreen = false;
        }
    }
}

// ---------- Painting ----------

const ACCENT_STRIPE_W_LOGICAL: i32 = 4;
const LABEL_PAD_LOGICAL: i32 = 6;
// Worst-case width-probe for the right-side countdown column. The bubble
// renders countdown-only (percent moved inline), so this is just "N{suffix}"
// for the longest reasonable duration. Bumped from "100% · 23h" which had
// been sized for the old combined string and left a big empty gap.
const COUNTDOWN_TEMPLATE: &str = "999d";

struct BarLayout {
    /// Bubble width in pixels.
    canvas_w: i32,
    /// Bubble height in pixels.
    canvas_h: i32,
    /// Corner radius of the rounded rectangle.
    corner_radius: i32,
    /// Accent stripe (left edge) in pixels — Claude orange or Codex green.
    accent_right: i32,
    /// Label column ("5h" / "7d").
    label_left: i32,
    label_right: i32,
    /// Bar geometry.
    bar_left: i32,
    bar_right: i32,
    bar_h: i32,
    /// Right-side countdown text column.
    right_text_left: i32,
    right_text_right: i32,
    /// Vertical positions (top edge of each row's bar).
    row1_y: i32,
    row2_y: i32,
    /// Font size for the main text (countdown + inline percent).
    font_px: i32,
    /// Font size for the muted row labels — a notch smaller than `font_px`.
    label_font_px: i32,
}

fn compute_layout(size_logical: i32, dpi: u32, mem_dc: HDC) -> BarLayout {
    let bp = breakpoint_for_width_logical(size_logical);
    let width_px = scale_to_dpi(size_logical, dpi);
    let height_px = scale_to_dpi(bubble_height_logical(size_logical), dpi);
    let bar_h = scale_to_dpi(bp.bar_h, dpi);
    let row_gap = scale_to_dpi(bp.row_gap, dpi);
    let pad_x = scale_to_dpi(10, dpi);
    let pad_y = ((height_px - bar_h * 2 - row_gap) / 2).max(scale_to_dpi(6, dpi));
    let accent_w = scale_to_dpi(ACCENT_STRIPE_W_LOGICAL, dpi);
    let label_pad = scale_to_dpi(LABEL_PAD_LOGICAL, dpi);
    let corner_radius = scale_to_dpi((bp.bar_h + bp.row_gap).max(8), dpi).min(height_px / 2);

    // Measure the worst-case strings against the real font so the columns are
    // exactly wide enough — no more, no less.
    let font_px = scale_to_dpi(bp.font, dpi).max(scale_to_dpi(11, dpi));
    let label_font_px = scale_to_dpi(bp.font - 2, dpi).max(scale_to_dpi(9, dpi));
    let countdown_w = measure_text_w(mem_dc, COUNTDOWN_TEMPLATE, font_px);
    let label_w = measure_text_w(mem_dc, "5h", label_font_px)
        .max(measure_text_w(mem_dc, "7d", label_font_px));

    let accent_left = 0;
    let accent_right = accent_left + accent_w;
    let label_left = accent_right + label_pad;
    let label_right = label_left + label_w;
    let bar_left = label_right + label_pad;
    let right_text_right = width_px - pad_x;
    let right_text_left = (right_text_right - countdown_w).max(bar_left + scale_to_dpi(20, dpi));
    let bar_right = (right_text_left - label_pad).max(bar_left + scale_to_dpi(20, dpi));

    let row1_y = pad_y;
    let row2_y = pad_y + bar_h + row_gap;

    BarLayout {
        canvas_w: width_px,
        canvas_h: height_px,
        corner_radius,
        accent_right,
        label_left,
        label_right,
        bar_left,
        bar_right,
        bar_h,
        right_text_left,
        right_text_right,
        row1_y,
        row2_y,
        font_px,
        label_font_px,
    }
}

fn measure_text_w(hdc: HDC, text: &str, font_height_px: i32) -> i32 {
    use windows::Win32::Foundation::SIZE;
    let font_name = wide_str("Segoe UI");
    let mut w: Vec<u16> = text.encode_utf16().collect();
    unsafe {
        let font = CreateFontW(
            -font_height_px,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET.0 as u32,
            OUT_DEFAULT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            CLEARTYPE_QUALITY.0 as u32,
            (FF_SWISS.0 | DEFAULT_PITCH.0) as u32,
            PCWSTR::from_raw(font_name.as_ptr()),
        );
        let old = SelectObject(hdc, font);
        let mut size = SIZE::default();
        let _ = GetTextExtentPoint32W(hdc, &mut w, &mut size);
        SelectObject(hdc, old);
        let _ = DeleteObject(font);
        size.cx
    }
}

/// Pack an `Rgb` for direct write into a 32-bpp `BI_RGB` DIB. The DIB stores
/// bytes B,G,R,X in memory, so a little-endian u32 read is `(b) | (g<<8) | (r<<16)`.
/// Note this is the OPPOSITE byte order from a GDI `COLORREF` (which is
/// `(r) | (g<<8) | (b<<16)`) — don't confuse the two.
fn rgb_to_dib(c: Color) -> u32 {
    (c.b as u32) | ((c.g as u32) << 8) | ((c.r as u32) << 16)
}

struct PaintInputs {
    model: TrayIconKind,
    session_pct: Option<f64>,
    session_text: String,
    weekly_pct: Option<f64>,
    weekly_text: String,
    is_dark: bool,
    pulse_phase: u32,
}

fn render(hwnd: HWND) {
    let (size_logical, dpi, inputs) = {
        let bubbles = lock_bubbles();
        let Some(b) = bubbles.get(&(hwnd.0 as isize)) else {
            return;
        };
        (
            b.size_logical,
            b.dpi,
            PaintInputs {
                model: b.model,
                session_pct: b.session_pct,
                session_text: b.session_text.clone(),
                weekly_pct: b.weekly_pct,
                weekly_text: b.weekly_text.clone(),
                is_dark: b.is_dark,
                pulse_phase: b.pulse_phase,
            },
        )
    };

    unsafe {
        let screen_dc = GetDC(hwnd);
        let mem_dc = CreateCompatibleDC(screen_dc);
        let layout = compute_layout(size_logical, dpi, mem_dc);

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: layout.canvas_w,
                biHeight: -layout.canvas_h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let dib = CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
            .unwrap_or_default();
        if dib.is_invalid() || bits.is_null() {
            let _ = DeleteDC(mem_dc);
            ReleaseDC(hwnd, screen_dc);
            return;
        }
        let old_bmp = SelectObject(mem_dc, dib);

        let pixel_count = (layout.canvas_w * layout.canvas_h) as usize;
        let pixels = std::slice::from_raw_parts_mut(bits as *mut u32, pixel_count);

        // Everything outside the rounded rect stays 0 (fully transparent).
        pixels.fill(0);

        paint_background(pixels, &layout, &inputs);
        paint_accent_stripe(pixels, &layout, inputs.model, inputs.is_dark);
        paint_bars(pixels, &layout, &inputs);
        paint_text_layer(mem_dc, &layout, &inputs);

        // Final alpha pass: alpha=255 inside the rounded rect, 0 outside. This
        // also lifts GDI-drawn text (which leaves alpha=0) into the visible plane.
        apply_alpha_mask(pixels, &layout);

        let mut wr = RECT::default();
        let _ = GetWindowRect(hwnd, &mut wr);
        let pt_dst = POINT {
            x: wr.left,
            y: wr.top,
        };
        let pt_src = POINT { x: 0, y: 0 };
        let sz = SIZE {
            cx: layout.canvas_w,
            cy: layout.canvas_h,
        };
        let blend = BLENDFUNCTION {
            BlendOp: 0,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: 1, // AC_SRC_ALPHA
        };
        let _ = UpdateLayeredWindow(
            hwnd,
            screen_dc,
            Some(&pt_dst),
            Some(&sz),
            mem_dc,
            Some(&pt_src),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );

        SelectObject(mem_dc, old_bmp);
        let _ = DeleteObject(dib);
        let _ = DeleteDC(mem_dc);
        ReleaseDC(hwnd, screen_dc);
    }
}

fn paint_background(pixels: &mut [u32], layout: &BarLayout, inputs: &PaintInputs) {
    let bg = if inputs.is_dark {
        Color::from_hex("#1F1F1F")
    } else {
        Color::from_hex("#F3F3F3")
    };
    let bg_packed = rgb_to_dib(bg);
    let blush_packed = rgb_to_dib(blend(bg, Color::from_hex("#C45020"), 0.15));

    let row1_blush = inputs.session_pct.is_some_and(|p| p >= 95.0);
    let row2_blush = inputs.weekly_pct.is_some_and(|p| p >= 95.0);
    let row1_band = row_band(layout, layout.row1_y);
    let row2_band = row_band(layout, layout.row2_y);

    for y in 0..layout.canvas_h {
        for x in 0..layout.canvas_w {
            if !point_in_rounded_rect(x, y, layout.canvas_w, layout.canvas_h, layout.corner_radius) {
                continue;
            }
            let in_row1 = row1_blush && y >= row1_band.0 && y < row1_band.1;
            let in_row2 = row2_blush && y >= row2_band.0 && y < row2_band.1;
            let pixel = if in_row1 || in_row2 { blush_packed } else { bg_packed };
            pixels[(y * layout.canvas_w + x) as usize] = pixel;
        }
    }
}

/// Top/bottom y-extent for a row's blush band — slightly taller than the bar
/// so the tint frames the row rather than just sitting under the fill.
fn row_band(layout: &BarLayout, row_top: i32) -> (i32, i32) {
    let padding = (layout.bar_h / 4).max(2);
    let top = (row_top - padding).max(0);
    let bot = (row_top + layout.bar_h + padding).min(layout.canvas_h);
    (top, bot)
}

fn paint_accent_stripe(pixels: &mut [u32], layout: &BarLayout, model: TrayIconKind, is_dark: bool) {
    let stripe = rgb_to_dib(accent_color_for(model, is_dark));
    for y in 0..layout.canvas_h {
        for x in 0..layout.accent_right {
            if !point_in_rounded_rect(x, y, layout.canvas_w, layout.canvas_h, layout.corner_radius) {
                continue;
            }
            pixels[(y * layout.canvas_w + x) as usize] = stripe;
        }
    }
}

/// Per-provider identity color. Claude = orange. Codex = white-in-dark /
/// charcoal-in-light — picking a pure white in light mode would vanish into
/// the `#F3F3F3` background, so we mirror to a contrasting neutral.
fn accent_color_for(model: TrayIconKind, is_dark: bool) -> Color {
    match model {
        ProviderId::Claude => Color::from_hex("#D97757"),
        ProviderId::ChatGpt => {
            if is_dark {
                Color::from_hex("#FFFFFF")
            } else {
                Color::from_hex("#2A2A2A")
            }
        }
    }
}

fn paint_bars(pixels: &mut [u32], layout: &BarLayout, inputs: &PaintInputs) {
    let track = if inputs.is_dark {
        Color::from_hex("#3A3A3A")
    } else {
        Color::from_hex("#D6D6D6")
    };
    paint_one_bar(pixels, layout, layout.row1_y, inputs.session_pct, track, inputs);
    paint_one_bar(pixels, layout, layout.row2_y, inputs.weekly_pct, track, inputs);
}

fn paint_one_bar(
    pixels: &mut [u32],
    layout: &BarLayout,
    top: i32,
    pct: Option<f64>,
    track: Color,
    inputs: &PaintInputs,
) {
    let bar_w = layout.bar_right - layout.bar_left;
    if bar_w <= 0 {
        return;
    }
    let track_packed = rgb_to_dib(track);
    for y in top..top + layout.bar_h {
        for x in layout.bar_left..layout.bar_right {
            pixels[(y * layout.canvas_w + x) as usize] = track_packed;
        }
    }
    let Some(p) = pct else {
        return;
    };
    let fill_w = ((p.clamp(0.0, 100.0) / 100.0) * bar_w as f64).round() as i32;
    if fill_w <= 0 {
        return;
    }
    let mut accent_rgb = bar_fill_color(inputs.model, inputs.is_dark, p);
    if p >= 95.0 {
        // Slow brightness triangle: 0.85 → 1.15 over 24 ticks (≈1.9s @ 80ms).
        let t = pulse_triangle(inputs.pulse_phase);
        accent_rgb = brighten(accent_rgb, t);
    }
    let accent_packed = rgb_to_dib(accent_rgb);
    let end_x = (layout.bar_left + fill_w).min(layout.bar_right);
    for y in top..top + layout.bar_h {
        for x in layout.bar_left..end_x {
            pixels[(y * layout.canvas_w + x) as usize] = accent_packed;
        }
    }
}

/// Triangle wave in [0, 1] with period 24 ticks. 0 at phase=0,12; 1 at phase=6,18.
fn pulse_triangle(phase: u32) -> f64 {
    let p = (phase % 24) as i32;
    let dist = (p - 12).abs(); // 0..12
    1.0 - (dist as f64 / 12.0)
}

/// Linearly brighten `c` toward white by `t` in [0, 1].
fn brighten(c: Color, t: f64) -> Color {
    // Map t to a smaller brightness delta — the pulse should be a subtle nudge.
    let t = t.clamp(0.0, 1.0) * 0.30;
    Color::new(
        ((c.r as f64) + (255.0 - c.r as f64) * t).round() as u8,
        ((c.g as f64) + (255.0 - c.g as f64) * t).round() as u8,
        ((c.b as f64) + (255.0 - c.b as f64) * t).round() as u8,
    )
}

fn blend(a: Color, b: Color, t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::new(
        ((a.r as f64) * (1.0 - t) + (b.r as f64) * t).round() as u8,
        ((a.g as f64) * (1.0 - t) + (b.g as f64) * t).round() as u8,
        ((a.b as f64) * (1.0 - t) + (b.b as f64) * t).round() as u8,
    )
}

/// One pass over the GDI text: row labels (muted) + inline percent (inside
/// the bar) + countdown (right column).
fn paint_text_layer(hdc: HDC, layout: &BarLayout, inputs: &PaintInputs) {
    let text_color = if inputs.is_dark {
        Color::from_hex("#EAEAEA")
    } else {
        Color::from_hex("#1F1F1F")
    };
    let muted_color = if inputs.is_dark {
        Color::from_hex("#888888")
    } else {
        Color::from_hex("#6E6E6E")
    };

    let font_name = wide_str("Segoe UI");
    unsafe {
        let main_font = create_font(layout.font_px, &font_name, FW_NORMAL.0 as i32);
        let bold_font = create_font(layout.font_px, &font_name, FW_SEMIBOLD.0 as i32);
        let label_font = create_font(layout.label_font_px, &font_name, FW_NORMAL.0 as i32);
        SetBkMode(hdc, TRANSPARENT);

        // Row labels in the left column.
        SelectObject(hdc, label_font);
        SetTextColor(hdc, COLORREF(muted_color.into_colorref()));
        draw_label(hdc, layout, layout.row1_y, "5h");
        draw_label(hdc, layout, layout.row2_y, "7d");

        // Inline percent: drawn over the bar, contrast picked from the pixel
        // under the text (fill if covered, track otherwise).
        SelectObject(hdc, bold_font);
        draw_inline_percent(hdc, layout, layout.row1_y, inputs.session_pct, inputs.model, inputs.is_dark);
        draw_inline_percent(hdc, layout, layout.row2_y, inputs.weekly_pct, inputs.model, inputs.is_dark);

        // Countdown on the right.
        SelectObject(hdc, main_font);
        SetTextColor(hdc, COLORREF(text_color.into_colorref()));
        draw_countdown(hdc, layout, layout.row1_y, &inputs.session_text);
        draw_countdown(hdc, layout, layout.row2_y, &inputs.weekly_text);

        let _ = DeleteObject(main_font);
        let _ = DeleteObject(bold_font);
        let _ = DeleteObject(label_font);
    }
}

fn create_font(height_px: i32, name_w: &[u16], weight: i32) -> HFONT {
    unsafe {
        CreateFontW(
            -height_px,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET.0 as u32,
            OUT_DEFAULT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            CLEARTYPE_QUALITY.0 as u32,
            (FF_SWISS.0 | DEFAULT_PITCH.0) as u32,
            PCWSTR::from_raw(name_w.as_ptr()),
        )
    }
}

fn draw_label(hdc: HDC, layout: &BarLayout, row_top: i32, text: &str) {
    let mut text_w = wide_str(text);
    let len_no_nul = text_w.len().saturating_sub(1);
    let mut rect = RECT {
        left: layout.label_left,
        top: row_top - 2,
        right: layout.label_right,
        bottom: row_top + layout.bar_h + 2,
    };
    unsafe {
        let _ = DrawTextW(
            hdc,
            &mut text_w[..len_no_nul],
            &mut rect,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOCLIP,
        );
    }
}

fn draw_inline_percent(
    hdc: HDC,
    layout: &BarLayout,
    row_top: i32,
    pct: Option<f64>,
    model: TrayIconKind,
    is_dark: bool,
) {
    let Some(p) = pct else {
        return;
    };
    let text = format!("{:.0}%", p);

    // Measure the percent against the currently-selected font so we can
    // anchor it to the fill's trailing edge rather than the bar's far right.
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    let mut sz = windows::Win32::Foundation::SIZE::default();
    unsafe {
        let _ = GetTextExtentPoint32W(hdc, &mut wide, &mut sz);
    }
    let text_w = sz.cx;

    let bar_w = layout.bar_right - layout.bar_left;
    let fill_w = ((p.clamp(0.0, 100.0) / 100.0) * bar_w as f64).round() as i32;
    let inset = (layout.bar_h / 4).max(2);
    let fill_color = bar_fill_color(model, is_dark, p);
    let track_color = if is_dark {
        Color::from_hex("#3A3A3A")
    } else {
        Color::from_hex("#D6D6D6")
    };

    // Two anchoring modes:
    //  - Fill is wide enough to hold the percent → right-align the text
    //    *inside* the fill at its trailing edge. The text sits on the fill.
    //  - Fill is too narrow → left-align the text just to the right of the
    //    fill, on the track. The text follows the fill's edge.
    // Either way the percent is tethered to where the bar reaches.
    let (text_left, underlying) = if fill_w >= text_w + inset * 2 {
        let right = layout.bar_left + fill_w - inset;
        ((right - text_w).max(layout.bar_left + inset), fill_color)
    } else {
        let left = layout.bar_left + fill_w + inset;
        let clamped = left.min(layout.bar_right - text_w - inset).max(layout.bar_left + inset);
        (clamped, track_color)
    };

    let fg = if use_dark_text_over(underlying) {
        Color::from_hex("#101010")
    } else {
        Color::from_hex("#F5F5F5")
    };

    let mut text_buf = wide_str(&text);
    let len_no_nul = text_buf.len().saturating_sub(1);
    let mut rect = RECT {
        left: text_left,
        top: row_top - 2,
        right: (text_left + text_w).min(layout.bar_right),
        bottom: row_top + layout.bar_h + 2,
    };
    unsafe {
        SetTextColor(hdc, COLORREF(fg.into_colorref()));
        let _ = DrawTextW(
            hdc,
            &mut text_buf[..len_no_nul],
            &mut rect,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOCLIP,
        );
    }
}

fn draw_countdown(hdc: HDC, layout: &BarLayout, row_top: i32, text: &str) {
    if text.is_empty() {
        return;
    }
    // Left-align so the countdown sits right next to the bar with only the
    // `label_pad` gap. Right-aligning to the bubble's far edge left a visible
    // float between bar end and number.
    let mut text_w = wide_str(text);
    let len_no_nul = text_w.len().saturating_sub(1);
    let mut rect = RECT {
        left: layout.right_text_left,
        top: row_top - 2,
        right: layout.right_text_right,
        bottom: row_top + layout.bar_h + 2,
    };
    unsafe {
        let _ = DrawTextW(
            hdc,
            &mut text_w[..len_no_nul],
            &mut rect,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOCLIP | DT_END_ELLIPSIS,
        );
    }
}

fn apply_alpha_mask(pixels: &mut [u32], layout: &BarLayout) {
    for y in 0..layout.canvas_h {
        for x in 0..layout.canvas_w {
            let idx = (y * layout.canvas_w + x) as usize;
            if point_in_rounded_rect(x, y, layout.canvas_w, layout.canvas_h, layout.corner_radius) {
                pixels[idx] |= 0xFF00_0000;
            } else {
                pixels[idx] = 0;
            }
        }
    }
}

/// Discrete 4-band fill color. The "safe" band uses the provider's identity
/// color so Codex bars stay white-on-dark while Claude bars stay orange; the
/// warning bands are the same alarm palette regardless of provider so an
/// approaching-limit always looks the same to the eye.
///
/// - <60%   → provider accent (Claude `#D97757` / Codex theme-derived)
/// - 60–80% → amber           (#E0A040)
/// - 80–95% → red             (#C45020)
/// - ≥95%   → deep red        (#A01818) — paired with pulse animation
pub fn bar_fill_color(model: TrayIconKind, is_dark: bool, percent: f64) -> Color {
    if percent < 60.0 {
        accent_color_for(model, is_dark)
    } else if percent < 80.0 {
        Color::from_hex("#E0A040")
    } else if percent < 95.0 {
        Color::from_hex("#C45020")
    } else {
        Color::from_hex("#A01818")
    }
}

/// Relative luminance of an sRGB color in 0..255 space. Cheap approximation
/// of the Rec. 709 coefficients — good enough for "should the text on this
/// pixel be white or black?" decisions.
fn luminance(c: Color) -> u32 {
    (c.r as u32 * 299 + c.g as u32 * 587 + c.b as u32 * 114) / 1000
}

/// Returns true when `c` is light enough that black text is more readable
/// than white text on top of it.
fn use_dark_text_over(c: Color) -> bool {
    luminance(c) >= 150
}

// ---------- Helpers ----------

fn primary_dpi() -> u32 {
    unsafe { GetDpiForSystem().max(96) }
}

fn scale_to_dpi(logical: i32, dpi: u32) -> i32 {
    ((logical as i64) * (dpi as i64) / 96) as i32
}

fn default_position(width_px: i32, height_px: i32, model: TrayIconKind) -> (i32, i32) {
    // Bottom-right of primary work area, with a 24-pixel gap from the edges.
    // Stagger the Codex bubble above the Claude one if both are enabled.
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let wa = if GetMonitorInfoW(monitor, &mut info).as_bool() {
            info.rcWork
        } else {
            RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            }
        };
        let gap = 24;
        let stagger = match model {
            ProviderId::Claude => 0,
            ProviderId::ChatGpt => height_px + gap,
        };
        let x = wa.right - width_px - gap;
        let y = wa.bottom - height_px - gap - stagger;
        (x, y)
    }
}
