//! Linux-only helpers.

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::sync::OnceLock;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt, KeyButMask, Window};
use x11rb::rust_connection::RustConnection;

use super::FullscreenCheck;
use crate::core::desktop_entry;

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

x11rb::atom_manager! {
    /// The EWMH atoms the fullscreen check reads.
    FullscreenAtoms: FullscreenAtomsCookie {
        _NET_ACTIVE_WINDOW,
        _NET_WM_STATE,
        _NET_WM_STATE_FULLSCREEN,
        _NET_WM_STATE_HIDDEN,
        _NET_WM_PID,
    }
}

/// The fullscreen atoms on the shared connection, interned on first use;
/// `None` without X11 or when interning failed (then it is not retried).
fn fullscreen_atoms() -> Option<(&'static X11, FullscreenAtoms)> {
    static ATOMS: OnceLock<Option<FullscreenAtoms>> = OnceLock::new();
    let x11 = x11()?;
    let atoms = ATOMS.get_or_init(|| {
        let atoms = FullscreenAtoms::new(&x11.conn)
            .map_err(|err| err.to_string())
            .and_then(|cookie| cookie.reply().map_err(|err| err.to_string()));
        match atoms {
            Ok(atoms) => Some(atoms),
            Err(err) => {
                log::warn!("EWMH atoms could not be interned: {err}");
                None
            }
        }
    });
    atoms.map(|atoms| (x11, atoms))
}

/// The 32-bit values of `property` on `window`, at most `max` of them.
/// Empty when the property is missing or has another type or format.
fn property32(
    x11: &X11,
    window: Window,
    property: Atom,
    kind: AtomEnum,
    max: u32,
) -> Result<Vec<u32>, String> {
    let reply = x11
        .conn
        .get_property(false, window, property, kind, 0, max)
        .map_err(|err| err.to_string())?
        .reply()
        .map_err(|err| err.to_string())?;
    Ok(reply.value32().map(Iterator::collect).unwrap_or_default())
}

/// Whether the EWMH active window is fullscreen and belongs to another
/// process. False without X11, without an active window or when a query
/// fails (a window can vanish between the reads).
pub fn foreground_is_fullscreen() -> FullscreenCheck {
    let Some((x11, atoms)) = fullscreen_atoms() else {
        return FullscreenCheck::new(false, "no X11 connection");
    };
    let active = match property32(x11, x11.root, atoms._NET_ACTIVE_WINDOW, AtomEnum::WINDOW, 1) {
        Ok(values) => values.first().copied().unwrap_or(0),
        Err(err) => {
            log::debug!("_NET_ACTIVE_WINDOW unreadable: {err}");
            return FullscreenCheck::new(false, "active window unreadable");
        }
    };
    if active == 0 {
        return FullscreenCheck::new(false, "no active window");
    }
    let state = property32(x11, active, atoms._NET_WM_STATE, AtomEnum::ATOM, 64);
    let pid = property32(x11, active, atoms._NET_WM_PID, AtomEnum::CARDINAL, 1);
    let (state, pid) = match (state, pid) {
        (Ok(state), Ok(pid)) => (state, pid.first().copied()),
        (Err(err), _) | (_, Err(err)) => {
            log::debug!("state of window 0x{active:X} unreadable: {err}");
            return FullscreenCheck::new(false, "active window unreadable");
        }
    };
    let (fullscreen, reason) = evaluate_active_window(&state, &atoms, pid, std::process::id());
    FullscreenCheck {
        fullscreen,
        reason,
        detail: format!(
            "window=0x{active:X} pid={}",
            pid.map_or_else(|| "<none>".to_string(), |p| p.to_string())
        ),
    }
}

/// The decision for the active window from its `_NET_WM_STATE` atoms and
/// `_NET_WM_PID`: fullscreen, not minimized, and not one of our windows.
fn evaluate_active_window(
    state: &[Atom],
    atoms: &FullscreenAtoms,
    window_pid: Option<u32>,
    own_pid: u32,
) -> (bool, &'static str) {
    if window_pid == Some(own_pid) {
        return (false, "foreground window is ours");
    }
    if state.contains(&atoms._NET_WM_STATE_HIDDEN) {
        return (false, "foreground minimized");
    }
    if !state.contains(&atoms._NET_WM_STATE_FULLSCREEN) {
        return (false, "foreground not fullscreen");
    }
    (true, "foreground fullscreen")
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

/// The start-at-login entry, `~/.config/autostart/<name>.desktop`: the file
/// the autostart plugin wrote in earlier releases, so an entry made by one
/// of them still reads as enabled and is removed by a disable.
fn autostart_file(name: &str) -> io::Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home directory"))?;
    Ok(home
        .join(".config")
        .join("autostart")
        .join(format!("{name}.desktop")))
}

/// Whether the start-at-login entry `name` exists.
pub fn autostart_enabled(name: &str) -> io::Result<bool> {
    Ok(autostart_file(name)?.exists())
}

/// Write or remove the start-at-login entry `name`. Enabling starts
/// `appimage` when the process runs from an AppImage (its mounted
/// executable moves on every run), otherwise the current executable. The
/// path is quoted in `Exec`, so a path with spaces or shell characters
/// starts the right file.
pub fn set_autostart(name: &str, appimage: Option<OsString>, enable: bool) -> io::Result<()> {
    let file = autostart_file(name)?;
    if !enable {
        return match std::fs::remove_file(&file) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        };
    }
    let exe = match appimage {
        Some(path) => PathBuf::from(path),
        None => std::env::current_exe()?,
    };
    let exe = exe.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("executable path is not UTF-8: {}", exe.display()),
        )
    })?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&file, desktop_entry::autostart_entry(name, exe))
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

    const ATOMS: FullscreenAtoms = FullscreenAtoms {
        _NET_ACTIVE_WINDOW: 1,
        _NET_WM_STATE: 2,
        _NET_WM_STATE_FULLSCREEN: 3,
        _NET_WM_STATE_HIDDEN: 4,
        _NET_WM_PID: 5,
    };
    const ABOVE: Atom = 99;

    #[test]
    fn fullscreen_needs_the_state_atom_on_a_window_of_another_process() {
        let fs = ATOMS._NET_WM_STATE_FULLSCREEN;
        assert_eq!(
            evaluate_active_window(&[ABOVE, fs], &ATOMS, Some(42), 7),
            (true, "foreground fullscreen")
        );
        // A window without `_NET_WM_PID` cannot be ours.
        assert!(evaluate_active_window(&[fs], &ATOMS, None, 7).0);
        assert_eq!(
            evaluate_active_window(&[ABOVE], &ATOMS, Some(42), 7),
            (false, "foreground not fullscreen")
        );
        assert!(!evaluate_active_window(&[], &ATOMS, Some(42), 7).0);
    }

    #[test]
    fn our_own_and_minimized_windows_never_count_as_fullscreen() {
        let fs = ATOMS._NET_WM_STATE_FULLSCREEN;
        assert_eq!(
            evaluate_active_window(&[fs], &ATOMS, Some(7), 7),
            (false, "foreground window is ours")
        );
        assert_eq!(
            evaluate_active_window(&[fs, ATOMS._NET_WM_STATE_HIDDEN], &ATOMS, Some(42), 7),
            (false, "foreground minimized")
        );
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
