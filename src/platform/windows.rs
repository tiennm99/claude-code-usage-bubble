//! Windows-only helpers. This is the only module that talks to Win32 APIs.

use crate::core::proxy::{self, SystemProxy};

const INTERNET_SETTINGS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

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
