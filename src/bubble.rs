// Floating circular bubble window.
//
// Top-level window with WS_POPUP + WS_EX_LAYERED + WS_EX_TOPMOST + WS_EX_NOACTIVATE.
// Shape is achieved via per-pixel alpha (alpha=0 outside the circle) and confirmed
// via WM_NCHITTEST returning HTCAPTION inside the circle, HTTRANSPARENT outside.
// The OS handles drag automatically because HTCAPTION inside the circle puts the
// click into the system move loop.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Mutex, MutexGuard, OnceLock};

use windows::core::PCWSTR;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Shell::ExtractIconExW;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::diagnose;
use crate::native_interop::{wide_str, Color, TIMER_FULLSCREEN_CHECK};
use crate::tray_icon::TrayIconKind;

// ---------- Public types & API ----------

pub const MIN_BUBBLE_SIZE: i32 = 32;
pub const MAX_BUBBLE_SIZE: i32 = 128;
pub const DEFAULT_BUBBLE_SIZE: i32 = 56;
const RESIZE_STEP: i32 = 8;
const SNAP_ZONE_LOGICAL: i32 = 12;
const CLASS_NAME: &str = "ClaudeCodeUsageBubble";
const FULLSCREEN_POLL_MS: u32 = 1500;

pub struct BubbleConfig {
    pub model: TrayIconKind,
    pub size_logical: i32,
    pub position: Option<(i32, i32)>,
    pub percent: Option<f64>,
    pub is_dark: bool,
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
            diagnose::log("bubble RegisterClassExW returned 0");
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
        let size_px = scale_to_dpi(initial_size_logical, dpi);
        let (x, y) =
            config
                .position
                .unwrap_or_else(|| default_position(size_px, config.model));
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            PCWSTR::from_raw(class_w.as_ptr()),
            PCWSTR::from_raw(title_w.as_ptr()),
            WS_POPUP,
            x,
            y,
            size_px,
            size_px,
            HWND::default(),
            HMENU::default(),
            hinstance,
            None,
        )
        .unwrap_or_default()
    };

    if hwnd == HWND::default() {
        diagnose::log("bubble CreateWindowExW failed");
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
            percent: config.percent,
            is_dark: config.is_dark,
            drag_start_pos: None,
            hidden_by_fullscreen: false,
            user_hidden: false,
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
        let _ = DestroyWindow(hwnd);
    }
}

pub fn update_percentage(hwnd: HWND, percent: Option<f64>) {
    {
        let mut bubbles = lock_bubbles();
        let Some(b) = bubbles.get_mut(&(hwnd.0 as isize)) else {
            return;
        };
        b.percent = percent;
    }
    render(hwnd);
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
    percent: Option<f64>,
    is_dark: bool,
    drag_start_pos: Option<(i32, i32)>,
    hidden_by_fullscreen: bool,
    user_hidden: bool,
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
            if wparam.0 == TIMER_FULLSCREEN_CHECK {
                check_fullscreen(hwnd);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            crate::app::on_menu_command(wparam.0 as u32, hwnd);
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
    let cx = (r.left + r.right) / 2;
    let cy = (r.top + r.bottom) / 2;
    let radius = ((r.right - r.left) / 2).max(1);
    let dx = pt.x - cx;
    let dy = pt.y - cy;
    if dx * dx + dy * dy <= radius * radius {
        LRESULT(HTCAPTION as isize)
    } else {
        LRESULT(HTTRANSPARENT as isize)
    }
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
    let size_px = scale_to_dpi(new_logical, dpi);
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut r);
        // Resize centered on existing center.
        let cx = (r.left + r.right) / 2;
        let cy = (r.top + r.bottom) / 2;
        let new_x = cx - size_px / 2;
        let new_y = cy - size_px / 2;
        let _ = SetWindowPos(
            hwnd,
            HWND::default(),
            new_x,
            new_y,
            size_px,
            size_px,
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
    let snap_zone = scale_to_dpi(SNAP_ZONE_LOGICAL, dpi);
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

    if (nx - wa.left).abs() < snap_zone {
        nx = wa.left;
    } else if (wa.right - (nx + w)).abs() < snap_zone {
        nx = wa.right - w;
    }
    if (ny - wa.top).abs() < snap_zone {
        ny = wa.top;
    } else if (wa.bottom - (ny + h)).abs() < snap_zone {
        ny = wa.bottom - h;
    }

    // Clamp into the work area in any case (so the bubble can't be lost off-screen).
    nx = nx.clamp(wa.left, wa.right - w);
    ny = ny.clamp(wa.top, wa.bottom - h);

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

fn render(hwnd: HWND) {
    let (size_logical, dpi, percent, is_dark) = {
        let bubbles = lock_bubbles();
        let Some(b) = bubbles.get(&(hwnd.0 as isize)) else {
            return;
        };
        (b.size_logical, b.dpi, b.percent, b.is_dark)
    };
    let size_px = scale_to_dpi(size_logical, dpi);

    unsafe {
        let screen_dc = GetDC(hwnd);
        let mem_dc = CreateCompatibleDC(screen_dc);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size_px,
                biHeight: -size_px,
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

        let pixel_count = (size_px * size_px) as usize;
        let pixels = std::slice::from_raw_parts_mut(bits as *mut u32, pixel_count);

        paint_background(pixels, size_px, is_dark);
        paint_ring(pixels, size_px, percent.unwrap_or(0.0), is_dark);
        paint_text(mem_dc, size_px, percent, is_dark, dpi);

        // Final alpha pass: set alpha=255 inside circle, 0 outside.
        let cx = (size_px - 1) as f64 / 2.0;
        let cy = cx;
        let radius = (size_px / 2) as f64 - 1.0;
        let r_sq = radius * radius;
        for y in 0..size_px {
            for x in 0..size_px {
                let dx = x as f64 - cx;
                let dy = y as f64 - cy;
                let idx = (y * size_px + x) as usize;
                if dx * dx + dy * dy <= r_sq {
                    pixels[idx] |= 0xFF000000;
                } else {
                    pixels[idx] = 0;
                }
            }
        }

        let mut wr = RECT::default();
        let _ = GetWindowRect(hwnd, &mut wr);
        let pt_dst = POINT {
            x: wr.left,
            y: wr.top,
        };
        let pt_src = POINT { x: 0, y: 0 };
        let sz = SIZE {
            cx: size_px,
            cy: size_px,
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

fn paint_background(pixels: &mut [u32], size_px: i32, is_dark: bool) {
    let bg = if is_dark {
        Color::from_hex("#1F1F1F")
    } else {
        Color::from_hex("#F3F3F3")
    };
    let bg_bgr = bg.to_colorref();
    let cx = (size_px - 1) as f64 / 2.0;
    let cy = cx;
    let radius = (size_px / 2) as f64 - 1.0;
    let r_sq = radius * radius;
    for y in 0..size_px {
        for x in 0..size_px {
            let dx = x as f64 - cx;
            let dy = y as f64 - cy;
            let idx = (y * size_px + x) as usize;
            if dx * dx + dy * dy <= r_sq {
                pixels[idx] = bg_bgr;
            } else {
                pixels[idx] = 0;
            }
        }
    }
}

fn paint_ring(pixels: &mut [u32], size_px: i32, percent: f64, is_dark: bool) {
    let ring = ring_color_for_percent(percent).to_colorref();
    let track = if is_dark {
        Color::from_hex("#3A3A3A").to_colorref()
    } else {
        Color::from_hex("#D6D6D6").to_colorref()
    };
    let cx = (size_px - 1) as f64 / 2.0;
    let cy = cx;
    let outer = (size_px / 2) as f64 - 1.0;
    let thickness = ((size_px as f64) * 0.12).max(3.0);
    let inner = outer - thickness;
    let inner_sq = inner * inner;
    let outer_sq = outer * outer;
    let sweep = (percent.clamp(0.0, 100.0) / 100.0) * 2.0 * std::f64::consts::PI;
    for y in 0..size_px {
        for x in 0..size_px {
            let dx = x as f64 - cx;
            let dy = y as f64 - cy;
            let d_sq = dx * dx + dy * dy;
            if d_sq <= outer_sq && d_sq >= inner_sq {
                // Angle from 12 o'clock, clockwise.
                let mut a = dx.atan2(-dy);
                if a < 0.0 {
                    a += 2.0 * std::f64::consts::PI;
                }
                let idx = (y * size_px + x) as usize;
                pixels[idx] = if a <= sweep { ring } else { track };
            }
        }
    }
}

fn paint_text(hdc: HDC, size_px: i32, percent: Option<f64>, is_dark: bool, _dpi: u32) {
    let text = match percent {
        Some(p) => format!("{:.0}%", p),
        None => "—".to_string(),
    };
    let mut text_w = wide_str(&text);
    let text_color = if is_dark {
        Color::from_hex("#F5F5F5")
    } else {
        Color::from_hex("#1F1F1F")
    };
    let font_height = -(size_px / 4).max(8);
    let font_name = wide_str("Segoe UI");
    unsafe {
        let font = CreateFontW(
            font_height,
            0,
            0,
            0,
            FW_SEMIBOLD.0 as i32,
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
        let old_font = SelectObject(hdc, font);
        SetTextColor(hdc, COLORREF(text_color.to_colorref()));
        SetBkMode(hdc, TRANSPARENT);
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: size_px,
            bottom: size_px,
        };
        // Trim the trailing NUL — DrawTextW reads slice length as count.
        let len_no_nul = text_w.len().saturating_sub(1);
        let _ = DrawTextW(
            hdc,
            &mut text_w[..len_no_nul],
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOCLIP,
        );
        SelectObject(hdc, old_font);
        let _ = DeleteObject(font);
    }
}

pub fn ring_color_for_percent(percent: f64) -> Color {
    if percent <= 50.0 {
        return Color::from_hex("#D97757");
    }
    let stops: [(f64, Color); 5] = [
        (50.0, Color::from_hex("#D97757")),
        (70.0, Color::from_hex("#D08540")),
        (85.0, Color::from_hex("#CC8C20")),
        (95.0, Color::from_hex("#C45020")),
        (100.0, Color::from_hex("#B82020")),
    ];
    for pair in stops.windows(2) {
        let (sp, sc) = pair[0];
        let (ep, ec) = pair[1];
        if percent <= ep {
            let span = (ep - sp).max(f64::EPSILON);
            let t = ((percent - sp) / span).clamp(0.0, 1.0);
            return Color::new(
                lerp_u8(sc.r, ec.r, t),
                lerp_u8(sc.g, ec.g, t),
                lerp_u8(sc.b, ec.b, t),
            );
        }
    }
    Color::from_hex("#B82020")
}

fn lerp_u8(a: u8, b: u8, t: f64) -> u8 {
    (a as f64 + (b as f64 - a as f64) * t).round() as u8
}

// ---------- Helpers ----------

fn primary_dpi() -> u32 {
    unsafe { GetDpiForSystem().max(96) }
}

fn scale_to_dpi(logical: i32, dpi: u32) -> i32 {
    ((logical as i64) * (dpi as i64) / 96) as i32
}

fn default_position(size_px: i32, model: TrayIconKind) -> (i32, i32) {
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
            TrayIconKind::Claude => 0,
            TrayIconKind::Codex => size_px + gap,
        };
        let x = wa.right - size_px - gap;
        let y = wa.bottom - size_px - gap - stagger;
        (x, y)
    }
}
