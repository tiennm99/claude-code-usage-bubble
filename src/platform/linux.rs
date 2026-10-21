//! Linux-only helpers.

use std::ffi::OsString;
use std::sync::OnceLock;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, KeyButMask, Window};
use x11rb::rust_connection::RustConnection;

/// The AppIndicator libraries the tray backend loads, in its own order. The
/// unversioned names come from the backend's `backcompat` feature (on by
/// default) and are sometimes the only ones present in an AppImage.
const APPINDICATOR_LIBRARIES: [&str; 4] = [
    "libayatana-appindicator3.so.1",
    "libappindicator3.so.1",
    "libayatana-appindicator3.so",
    "libappindicator3.so",
];

/// Whether an AppIndicator library can be loaded. The tray backend panics
/// when neither loads, and the release profile aborts on panic, so the tray
/// is built only when this is true. Probed once per process.
pub fn appindicator_available() -> bool {
    // The loaded library is kept for the life of the process rather than
    // unloaded right before the tray backend loads it again.
    static LIBRARY: OnceLock<Option<libloading::Library>> = OnceLock::new();
    LIBRARY
        .get_or_init(|| {
            for name in APPINDICATOR_LIBRARIES {
                // SAFETY: these are the libraries the tray backend itself
                // loads; their initialisers run either way once a tray is built.
                match unsafe { libloading::Library::new(name) } {
                    Ok(lib) => {
                        log::info!("tray: {name} found");
                        return Some(lib);
                    }
                    Err(err) => log::debug!("tray: {name} not loadable: {err}"),
                }
            }
            log::warn!("tray: no AppIndicator library found; running without a tray");
            None
        })
        .is_some()
}

/// Whether GTK runs on X11, where windows can be placed: `GDK_BACKEND`
/// names x11 first, or it is unset outside a Wayland session. The runtime
/// sets `GDK_BACKEND=x11` under Wayland when Xwayland is available.
pub fn positioning_supported(
    wayland_display: Option<OsString>,
    gdk_backend: Option<OsString>,
) -> bool {
    let set = |v: &Option<OsString>| v.as_ref().is_some_and(|v| !v.is_empty());
    if set(&gdk_backend) {
        let backend = gdk_backend.unwrap_or_default();
        return backend
            .to_string_lossy()
            .split(',')
            .next()
            .is_some_and(|first| first.trim() == "x11");
    }
    !set(&wayland_display)
}

/// One X connection for the process and its default root window.
struct X11 {
    conn: RustConnection,
    root: Window,
}

/// The shared X connection, opened on first use; `None` without an X
/// server (then it is not retried).
fn x11() -> Option<&'static X11> {
    static X11_CONNECTION: OnceLock<Option<X11>> = OnceLock::new();
    X11_CONNECTION
        .get_or_init(|| match x11rb::connect(None) {
            Ok((conn, screen)) => {
                let root = conn.setup().roots.get(screen)?.root;
                Some(X11 { conn, root })
            }
            Err(err) => {
                log::info!("no X11 connection: {err}");
                None
            }
        })
        .as_ref()
}

/// Whether mouse Button1 is held, from an X `QueryPointer`. False without
/// X11 or when the query fails.
pub fn button1_down() -> bool {
    let Some(x11) = x11() else {
        return false;
    };
    let reply = x11
        .conn
        .query_pointer(x11.root)
        .map_err(|err| err.to_string())
        .and_then(|cookie| cookie.reply().map_err(|err| err.to_string()));
    match reply {
        Ok(reply) => reply.mask.contains(KeyButMask::BUTTON1),
        Err(err) => {
            log::debug!("X QueryPointer failed: {err}");
            false
        }
    }
}

/// How long the portal settings read may block.
const PORTAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Maps the portal's `org.freedesktop.appearance color-scheme` value to a
/// dark flag: 1 prefers dark, 2 prefers light. "No preference" (0), any
/// other value and an unreachable portal give `None`, so the caller keeps
/// its current theme. tao's `Window::theme()` maps all of those to light.
pub fn color_scheme_is_dark(scheme: Option<u32>) -> Option<bool> {
    match scheme {
        Some(1) => Some(true),
        Some(2) => Some(false),
        _ => None,
    }
}

/// Whether the desktop portal states a dark or light preference; `None`
/// when it states none or cannot be reached. Blocks for up to
/// `PORTAL_TIMEOUT`.
pub fn portal_prefers_dark() -> Option<bool> {
    color_scheme_is_dark(read_color_scheme())
}

fn read_color_scheme() -> Option<u32> {
    use dbus::arg::Variant;
    use dbus::blocking::Connection;

    let result = Connection::new_session().and_then(|conn| {
        let proxy = conn.with_proxy(
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            PORTAL_TIMEOUT,
        );
        let (value,): (Variant<Variant<u32>>,) = proxy.method_call(
            "org.freedesktop.portal.Settings",
            "Read",
            ("org.freedesktop.appearance", "color-scheme"),
        )?;
        Ok(value.0 .0)
    });
    match result {
        Ok(scheme) => Some(scheme),
        Err(err) => {
            log::debug!("portal color-scheme unavailable: {err}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(v: &str) -> Option<OsString> {
        Some(OsString::from(v))
    }

    #[test]
    fn positioning_follows_the_gdk_backend() {
        // Plain X11 session.
        assert!(positioning_supported(None, None));
        assert!(positioning_supported(os(""), None));
        // Wayland with the x11 override (Xwayland).
        assert!(positioning_supported(os("wayland-0"), os("x11")));
        assert!(positioning_supported(os("wayland-0"), os("x11,wayland")));
        // Native Wayland.
        assert!(!positioning_supported(os("wayland-0"), None));
        assert!(!positioning_supported(os("wayland-0"), os("wayland")));
        assert!(!positioning_supported(os("wayland-0"), os("wayland,x11")));
        assert!(!positioning_supported(None, os("broadway")));
    }

    #[test]
    fn only_a_stated_portal_preference_sets_the_theme() {
        assert_eq!(color_scheme_is_dark(Some(1)), Some(true));
        assert_eq!(color_scheme_is_dark(Some(2)), Some(false));
        // No preference, unknown values and an unreachable portal keep the
        // current theme (dark by default).
        assert_eq!(color_scheme_is_dark(Some(0)), None);
        assert_eq!(color_scheme_is_dark(Some(7)), None);
        assert_eq!(color_scheme_is_dark(None), None);
    }
}
