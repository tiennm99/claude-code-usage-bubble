// Diagnostic logging facade backed by `log` + `simplelog`.
//
// `init(true)` redirects every `log::info!`/`log::warn!`/`log::error!` call
// across the crate to a file in `%TEMP%`. With `init(false)` (the default,
// i.e. no `--diagnose` flag) logging is a no-op.

use std::fs::File;
use std::path::PathBuf;

use simplelog::{Config, LevelFilter, WriteLogger};

const LOG_FILE_NAME: &str = "claude-code-usage-bubble.log";

/// Initialise file-based logging. Idempotent — second call is a no-op.
///
/// Returns the resolved log-file path on success, or `Ok(None)` when
/// `enabled` is false. `Err` is only returned if the file could not be
/// opened (e.g. read-only `%TEMP%`); callers may ignore the error.
pub fn init(enabled: bool) -> std::io::Result<Option<PathBuf>> {
    if !enabled {
        return Ok(None);
    }
    let path = std::env::temp_dir().join(LOG_FILE_NAME);
    let file = File::create(&path)?;
    // simplelog will refuse a second init; convert that into a soft no-op.
    let _ = WriteLogger::init(LevelFilter::Debug, Config::default(), file);
    log::info!("diagnostic logging enabled at {}", path.display());
    Ok(Some(path))
}
