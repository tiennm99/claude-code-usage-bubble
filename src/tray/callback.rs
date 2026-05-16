// Dispatch the `WM_APP_TRAY` notification message.
//
// Shell_NotifyIconW packs the event in the LOWORD of lparam (the
// underlying mouse-event code) and the icon ID in HIWORD. We translate
// to a `TrayAction` and let the app handle it.

use windows::Win32::Foundation::LPARAM;

use super::TrayAction;

const WM_LBUTTONUP: u32 = 0x0202;
const WM_RBUTTONUP: u32 = 0x0205;

pub fn handle(lparam: LPARAM) -> TrayAction {
    let raw = lparam.0 as u32;
    let event = raw & 0xFFFF;
    match event {
        WM_LBUTTONUP => TrayAction::ToggleWidget,
        WM_RBUTTONUP => TrayAction::ShowContextMenu,
        _ => TrayAction::None,
    }
}
