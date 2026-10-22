//! Live theme switching on Linux.
//!
//! tao reports a desktop theme change with a placeholder window id that
//! tauri drops, so no window ever sees `ThemeChanged`. Instead one
//! background thread listens on the session bus for the desktop portal's
//! `SettingChanged` signal and applies a stated `color-scheme` preference.

use std::sync::Arc;
use std::time::Duration;

use dbus::arg::{ArgType, RefArg, Variant};
use dbus::blocking::Connection;
use dbus::message::MatchRule;
use tauri::{AppHandle, Manager};

use crate::platform::linux::color_scheme_is_dark;
use crate::runtime::Runtime;

const SETTINGS_INTERFACE: &str = "org.freedesktop.portal.Settings";
const APPEARANCE_NAMESPACE: &str = "org.freedesktop.appearance";
const COLOR_SCHEME_KEY: &str = "color-scheme";

/// How long one wait for bus traffic blocks before it is started again.
const PROCESS_TIMEOUT: Duration = Duration::from_secs(60);

/// Start the listener thread. It runs for the life of the process; without
/// a session bus it logs once and ends, leaving the startup theme in place.
pub fn spawn(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("theme-listener".into())
        .spawn(move || listen(&app));
    if let Err(err) = spawned {
        log::error!("theme listener thread could not start: {err}");
    }
}

fn listen(app: &AppHandle) {
    let conn = match Connection::new_session() {
        Ok(conn) => conn,
        Err(err) => {
            log::warn!("no session bus; the theme will not follow the desktop: {err}");
            return;
        }
    };
    let rule = MatchRule::new_signal(SETTINGS_INTERFACE, "SettingChanged");
    let handle = app.clone();
    let added = conn.add_match(
        rule,
        move |(namespace, key, value): (String, String, Variant<Box<dyn RefArg>>),
              _: &Connection,
              _: &dbus::Message| {
            let scheme = color_scheme_of(&namespace, &key, &value);
            if let Some(is_dark) = color_scheme_is_dark(scheme) {
                apply(&handle, is_dark);
            }
            true
        },
    );
    if let Err(err) = added {
        log::warn!(
            "portal settings signal not watched; the theme will not follow the desktop: {err}"
        );
        return;
    }
    loop {
        if let Err(err) = conn.process(PROCESS_TIMEOUT) {
            log::warn!("theme listener stopped: {err}");
            return;
        }
    }
}

/// Record the new theme on the main thread, like every other UI change.
fn apply(app: &AppHandle, is_dark: bool) {
    let handle = app.clone();
    let queued = app.run_on_main_thread(move || {
        if let Some(runtime) = handle.try_state::<Arc<Runtime>>() {
            runtime.set_dark(is_dark);
        }
    });
    if let Err(err) = queued {
        log::warn!("theme change could not reach the main thread: {err}");
    }
}

/// The `color-scheme` value carried by one `SettingChanged` signal, or
/// `None` for any other setting or a value that is not a `u32`. Variants
/// are unwrapped however deeply the portal nests them.
fn color_scheme_of(namespace: &str, key: &str, value: &dyn RefArg) -> Option<u32> {
    if namespace != APPEARANCE_NAMESPACE || key != COLOR_SCHEME_KEY {
        return None;
    }
    unwrap_u32(value)
}

fn unwrap_u32(value: &dyn RefArg) -> Option<u32> {
    match value.arg_type() {
        ArgType::UInt32 => value.as_u64().and_then(|v| u32::try_from(v).ok()),
        ArgType::Variant => value.as_iter()?.next().and_then(unwrap_u32),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed(value: impl RefArg + 'static) -> Variant<Box<dyn RefArg>> {
        Variant(Box::new(value))
    }

    #[test]
    fn color_scheme_is_read_from_its_own_setting_only() {
        let dark = boxed(1u32);
        assert_eq!(
            color_scheme_of(APPEARANCE_NAMESPACE, COLOR_SCHEME_KEY, &dark),
            Some(1)
        );
        assert_eq!(
            color_scheme_of(APPEARANCE_NAMESPACE, "accent-color", &dark),
            None
        );
        assert_eq!(
            color_scheme_of("org.gnome.desktop.interface", COLOR_SCHEME_KEY, &dark),
            None
        );
    }

    #[test]
    fn nested_variants_unwrap_to_the_scheme() {
        let nested = boxed(Variant(2u32));
        assert_eq!(
            color_scheme_of(APPEARANCE_NAMESPACE, COLOR_SCHEME_KEY, &nested),
            Some(2)
        );
        let bare: Box<dyn RefArg> = Box::new(0u32);
        assert_eq!(
            color_scheme_of(APPEARANCE_NAMESPACE, COLOR_SCHEME_KEY, &bare),
            Some(0)
        );
    }

    #[test]
    fn a_value_of_another_type_is_ignored() {
        let text = boxed("prefer-dark".to_string());
        assert_eq!(
            color_scheme_of(APPEARANCE_NAMESPACE, COLOR_SCHEME_KEY, &text),
            None
        );
        let signed = boxed(1i32);
        assert_eq!(
            color_scheme_of(APPEARANCE_NAMESPACE, COLOR_SCHEME_KEY, &signed),
            None
        );
    }
}
