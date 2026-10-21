//! Windows-only helpers. This is the only module that talks to Win32 APIs.

use std::io;
use std::path::Path;
use std::time::Duration;

use webview2_com::Microsoft::Web::WebView2::Win32::GetAvailableCoreWebView2BrowserVersionString;
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, SetForegroundWindow, MB_ICONERROR, MB_OK, MB_SETFOREGROUND,
};

use crate::core::proxy::{self, SystemProxy};

const INTERNET_SETTINGS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";
const PERSONALIZE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const AUMID_KEY_PREFIX: &str = r"Software\Classes\AppUserModelId\";

/// The static proxy from `HKCU\…\Internet Settings`, or `None` when
/// `ProxyEnable` is not 1 or no usable `ProxyServer` entry exists.
/// PAC/WPAD (`AutoConfigURL`, auto-detect) is deliberately not evaluated.
pub fn system_proxy() -> Option<SystemProxy> {
    let key = windows_registry::CURRENT_USER
        .open(INTERNET_SETTINGS_KEY)
        .ok()?;
    let enable = key.get_u32("ProxyEnable").ok();
    let server = key.get_string("ProxyServer").ok();
    let proxy_override = key.get_string("ProxyOverride").ok();
    proxy::from_registry_values(enable, server.as_deref(), proxy_override.as_deref())
}

/// Whether the apps theme is dark. `SystemUsesLightTheme` is 1 for light;
/// a missing value means dark, as on a fresh Windows 11 install.
pub fn is_dark() -> bool {
    let light = windows_registry::CURRENT_USER
        .open(PERSONALIZE_KEY)
        .and_then(|key| key.get_u32("SystemUsesLightTheme"));
    !matches!(light, Ok(1))
}

/// Wait until process `pid` has exited, at most `timeout`. Returns true when
/// it is gone (or never existed), false on timeout or a failed wait.
pub fn wait_pid(pid: u32, timeout: Duration) -> bool {
    // SAFETY: plain FFI call; the returned handle is closed below.
    let Ok(handle) = (unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }) else {
        // No such process: it already exited.
        return true;
    };
    let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: `handle` is a valid process handle opened with SYNCHRONIZE.
    let result = unsafe { WaitForSingleObject(handle, ms) };
    // SAFETY: `handle` came from OpenProcess and is closed exactly once.
    if let Err(err) = unsafe { CloseHandle(handle) } {
        log::debug!("CloseHandle for pid {pid} failed: {err}");
    }
    result == WAIT_OBJECT_0
}

/// Whether the Microsoft Edge WebView2 runtime is installed.
pub fn webview2_available() -> bool {
    let mut version = PWSTR::null();
    // SAFETY: a null folder asks for the installed Evergreen runtime; the
    // out pointer is valid for the call.
    let result =
        unsafe { GetAvailableCoreWebView2BrowserVersionString(PCWSTR::null(), &mut version) };
    // Frees the CoTaskMem string; a null pointer yields "".
    let version = webview2_com::take_pwstr(version);
    match result {
        Ok(()) if !version.is_empty() => {
            log::info!("WebView2 runtime {version}");
            true
        }
        Ok(()) => false,
        Err(err) => {
            log::warn!("WebView2 runtime lookup failed: {err}");
            false
        }
    }
}

/// Show a blocking error message box with no owner window.
pub fn message_box_error(title: &str, text: &str) {
    // SAFETY: both strings outlive the call; no owner window is passed.
    let _ = unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(text),
            &HSTRING::from(title),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
        )
    };
}

/// Register `aumid` so toasts from this portable exe are shown although no
/// Start-menu shortcut carries the id: write
/// `HKCU\Software\Classes\AppUserModelId\<aumid>` with `DisplayName` and
/// `IconUri` (`icon_png` written to `icon_path`), then make it the process
/// id. Each step is attempted; the first error is returned after all ran.
pub fn register_aumid(
    aumid: &str,
    display_name: &str,
    icon_png: &[u8],
    icon_path: &Path,
) -> io::Result<()> {
    let icon = write_if_changed(icon_path, icon_png);
    if let Err(err) = &icon {
        log::warn!("AUMID icon {} not written: {err}", icon_path.display());
    }

    let key = windows_registry::CURRENT_USER
        .create(format!("{AUMID_KEY_PREFIX}{aumid}"))
        .and_then(|key| {
            key.set_string("DisplayName", display_name)?;
            if icon.is_ok() {
                key.set_string("IconUri", icon_path.to_string_lossy().as_ref())?;
            }
            Ok(())
        })
        .map_err(io::Error::from);

    // SAFETY: the HSTRING outlives the call.
    let process = unsafe { SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(aumid)) }
        .map_err(io::Error::from);

    key.and(process).and(icon)
}

/// Write `bytes` to `path` unless it already holds exactly them.
fn write_if_changed(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if std::fs::read(path).is_ok_and(|current| current == bytes) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, bytes)
}

/// The current user's start-at-login entry, `HKCU\…\Run\<name>`. Its data
/// is the quoted exe path, as every earlier release wrote it, so a path with
/// spaces cannot resolve to a planted prefix. HKLM is never written, so an
/// elevated run cannot leave an entry that a normal run cannot remove.
/// `is_enabled` honours a Task Manager disable in `StartupApproved\Run`.
pub fn autostart_entry(name: &str) -> io::Result<auto_launch::AutoLaunch> {
    let exe = std::env::current_exe()?;
    Ok(auto_launch::AutoLaunch::new(
        name,
        &quoted_path(&exe),
        auto_launch::WindowsEnableMode::CurrentUser,
        &[] as &[&str],
    ))
}

/// `path` wrapped in double quotes, the form a `Run` value needs.
fn quoted_path(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

/// Bring `window` to the foreground. A popup menu needs its owner in the
/// foreground, or it does not dismiss on a click elsewhere.
pub fn set_foreground(window: &tauri::WebviewWindow) {
    match window.hwnd() {
        // SAFETY: the handle belongs to a live window of this process.
        Ok(hwnd) => {
            if !unsafe { SetForegroundWindow(hwnd) }.as_bool() {
                log::debug!("SetForegroundWindow({}) refused", window.label());
            }
        }
        Err(err) => log::warn!("no HWND for {}: {err}", window.label()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_value_path_is_quoted() {
        assert_eq!(
            quoted_path(Path::new(r"D:\My Tools\claude-code-usage-bubble.exe")),
            r#""D:\My Tools\claude-code-usage-bubble.exe""#
        );
    }
}
