// Per-window DPI helpers.
//
// The DPI of a window can differ from `GetDpiForSystem` on multi-monitor
// setups where the app is per-monitor DPI aware. Always prefer
// `GetDpiForWindow` for HWNDs that participate in the message loop.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};

/// The default DPI Win32 reports for 100% scaling.
pub const BASE_DPI: u32 = 96;

/// DPI for the supplied window. Falls back to system DPI if the call fails.
pub fn for_window(hwnd: HWND) -> u32 {
    let raw = unsafe { GetDpiForWindow(hwnd) };
    if raw == 0 {
        for_system()
    } else {
        raw.max(BASE_DPI)
    }
}

/// Global system DPI. Cheap; safe to call from any thread.
pub fn for_system() -> u32 {
    unsafe { GetDpiForSystem() }.max(BASE_DPI)
}

/// Scale a logical (96-DPI) pixel measurement to the given DPI.
pub fn scale(logical_px: i32, dpi: u32) -> i32 {
    ((logical_px as i64) * (dpi as i64) / (BASE_DPI as i64)) as i32
}
