//! Linux-only helpers.

use std::sync::OnceLock;

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
