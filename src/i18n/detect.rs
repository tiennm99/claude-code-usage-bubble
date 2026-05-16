// Discover the user's preferred Windows UI language.
//
// We try three sources in priority order and return the first non-empty
// result. Callers normalise the returned BCP-47-ish code against the
// list of locales we actually ship.

use windows::core::PWSTR;
use windows::Win32::Globalization::{
    GetUserDefaultLocaleName, GetUserDefaultUILanguage, GetUserPreferredUILanguages,
    LCIDToLocaleName, LOCALE_ALLOW_NEUTRAL_NAMES, MAX_LOCALE_NAME, MUI_LANGUAGE_NAME,
};

/// First non-empty locale code from the user's preferences. May be
/// `Some("en-US")` style; callers do prefix normalisation.
pub fn detect_system_locale() -> Option<String> {
    preferred_ui()
        .into_iter()
        .next()
        .or_else(default_ui_language)
        .or_else(default_locale_name)
}

fn preferred_ui() -> Vec<String> {
    unsafe {
        let mut count: u32 = 0;
        let mut buf_len: u32 = 0;
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, PWSTR::null(), &mut buf_len)
            .is_err()
            || buf_len == 0
        {
            return Vec::new();
        }
        let mut buffer = vec![0u16; buf_len as usize];
        if GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            PWSTR(buffer.as_mut_ptr()),
            &mut buf_len,
        )
        .is_err()
        {
            return Vec::new();
        }
        buffer
            .split(|u| *u == 0)
            .filter(|s| !s.is_empty())
            .map(String::from_utf16_lossy)
            .collect()
    }
}

fn default_ui_language() -> Option<String> {
    unsafe {
        let lcid = GetUserDefaultUILanguage();
        let mut buf = [0u16; MAX_LOCALE_NAME as usize];
        let len = LCIDToLocaleName(lcid as u32, Some(&mut buf), LOCALE_ALLOW_NEUTRAL_NAMES);
        if len <= 1 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..(len as usize - 1)]))
    }
}

fn default_locale_name() -> Option<String> {
    unsafe {
        let mut buf = [0u16; MAX_LOCALE_NAME as usize];
        let len = GetUserDefaultLocaleName(&mut buf);
        if len <= 1 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..(len as usize - 1)]))
    }
}
