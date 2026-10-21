//! Linux-only helpers.

use std::sync::OnceLock;

/// The AppIndicator libraries the tray backend loads, in its own order.
const APPINDICATOR_LIBRARIES: [&str; 2] =
    ["libayatana-appindicator3.so.1", "libappindicator3.so.1"];

/// Whether an AppIndicator library can be loaded. The tray backend panics
/// when neither loads, and the release profile aborts on panic, so the tray
/// is built only when this is true. Probed once per process.
pub fn appindicator_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        for name in APPINDICATOR_LIBRARIES {
            // SAFETY: these are the libraries the tray backend itself loads;
            // their initialisers run either way once a tray is built.
            match unsafe { libloading::Library::new(name) } {
                Ok(_) => {
                    log::info!("tray: {name} found");
                    return true;
                }
                Err(err) => log::debug!("tray: {name} not loadable: {err}"),
            }
        }
        log::warn!("tray: no AppIndicator library found; running without a tray");
        false
    })
}
