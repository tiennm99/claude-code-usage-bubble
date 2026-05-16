// Windows light/dark theme detection.
//
// Windows stores the current theme under
// `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`
// with a `SystemUsesLightTheme` DWORD: 1 means light, 0 means dark.

use super::registry;

const THEME_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const LIGHT_VALUE: &str = "SystemUsesLightTheme";

/// `true` if the system is in dark mode. Defaults to dark when the registry
/// value is missing (matches Windows 11 first-boot behaviour).
pub fn is_dark() -> bool {
    !matches!(registry::read_u32(THEME_KEY, LIGHT_VALUE), Some(1))
}
