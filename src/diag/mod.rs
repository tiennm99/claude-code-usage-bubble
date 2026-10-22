// Diagnostic logging facade backed by `log` + `simplelog`.
//
// `init(true)` redirects every `log::info!`/`log::warn!`/`log::error!` call
// across the crate to the file `log_path()` names: `%TEMP%` on Windows, and
// on Unix a private `usage-monitor` folder (mode 0700) in the user's state
// directory (`$XDG_STATE_HOME`, by default `~/.local/state`), with the file
// itself readable by the owner only (mode 0600). With `init(false)` (the
// default, i.e. no `--diagnose` flag) logging is a no-op.

use std::fs::File;
use std::path::PathBuf;

use simplelog::{CombinedLogger, ConfigBuilder, LevelFilter, WriteLogger};

const LOG_FILE_NAME: &str = "usage-monitor.log";

/// The folder that holds the log on Unix, inside the state directory.
#[cfg(unix)]
const LOG_DIR_NAME: &str = "usage-monitor";

/// The updater plugin logs the proxy URL, credentials included, at debug
/// level, so its messages are kept to info and above.
const UPDATER_TARGET: &str = "tauri_plugin_updater";

/// Where `--diagnose` writes its log. On Linux this is
/// `$XDG_STATE_HOME/usage-monitor/usage-monitor.log`, by default
/// `~/.local/state/usage-monitor/usage-monitor.log`; on Windows
/// `%TEMP%\usage-monitor.log`.
pub fn log_path() -> PathBuf {
    #[cfg(unix)]
    {
        unix_log_dir(dirs::state_dir(), dirs::cache_dir(), std::env::temp_dir()).join(LOG_FILE_NAME)
    }
    #[cfg(not(unix))]
    {
        std::env::temp_dir().join(LOG_FILE_NAME)
    }
}

/// The Unix log folder: under the state directory, else the cache
/// directory, else the temp directory.
#[cfg(unix)]
fn unix_log_dir(state: Option<PathBuf>, cache: Option<PathBuf>, temp: PathBuf) -> PathBuf {
    state.or(cache).unwrap_or(temp).join(LOG_DIR_NAME)
}

/// Create (or truncate) the log at `path`. On Unix its folder is created
/// with mode 0700 and must be a directory owned by this user, so a folder
/// someone else made in a shared temp directory is refused; the file is
/// created with mode 0600, and an existing file is reset to 0600.
fn open_log(path: &std::path::Path) -> std::io::Result<File> {
    #[cfg(unix)]
    {
        use std::fs::{DirBuilder, OpenOptions, Permissions};
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};

        if let Some(dir) = path.parent() {
            DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
            let meta = std::fs::symlink_metadata(dir)?;
            // SAFETY: geteuid has no preconditions and cannot fail.
            let uid = unsafe { libc::geteuid() };
            if !meta.is_dir() || meta.uid() != uid {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("{} is not a folder owned by this user", dir.display()),
                ));
            }
        }
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.set_permissions(Permissions::from_mode(0o600))?;
        Ok(file)
    }
    #[cfg(not(unix))]
    {
        File::create(path)
    }
}

/// Initialise file-based logging. Idempotent — second call is a no-op.
///
/// Returns the resolved log-file path on success, or `Ok(None)` when
/// `enabled` is false. `Err` is only returned if the file could not be
/// opened (e.g. a read-only log folder); callers may ignore the error.
pub fn init(enabled: bool) -> std::io::Result<Option<PathBuf>> {
    if !enabled {
        return Ok(None);
    }
    let path = log_path();
    let file = open_log(&path)?;
    let updater_file = file.try_clone()?;
    let app = ConfigBuilder::new()
        .add_filter_ignore_str(UPDATER_TARGET)
        .build();
    let updater = ConfigBuilder::new()
        .add_filter_allow_str(UPDATER_TARGET)
        .build();
    // simplelog will refuse a second init; convert that into a soft no-op.
    let _ = CombinedLogger::init(vec![
        WriteLogger::new(LevelFilter::Debug, app, file),
        WriteLogger::new(LevelFilter::Info, updater, updater_file),
    ]);
    log::info!("diagnostic logging enabled at {}", path.display());
    Ok(Some(path))
}

#[cfg(all(test, unix))]
mod tests {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use super::*;

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn log_folder_prefers_state_then_cache_then_temp() {
        let state = Some(PathBuf::from("/home/me/.local/state"));
        let cache = Some(PathBuf::from("/home/me/.cache"));
        let temp = PathBuf::from("/tmp");
        assert_eq!(
            unix_log_dir(state, cache.clone(), temp.clone()),
            PathBuf::from("/home/me/.local/state/usage-monitor")
        );
        assert_eq!(
            unix_log_dir(None, cache, temp.clone()),
            PathBuf::from("/home/me/.cache/usage-monitor")
        );
        assert_eq!(
            unix_log_dir(None, None, temp),
            PathBuf::from("/tmp/usage-monitor")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_log_lives_in_the_state_directory() {
        let expected = dirs::state_dir()
            .unwrap()
            .join("usage-monitor")
            .join("usage-monitor.log");
        assert_eq!(log_path(), expected);
    }

    #[test]
    fn log_is_private_and_truncated() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("usage-monitor-diag-{}-{nanos}", std::process::id()));
        let dir = root.join(LOG_DIR_NAME);
        let path = dir.join(LOG_FILE_NAME);

        let mut file = open_log(&path).unwrap();
        file.write_all(b"first run").unwrap();
        drop(file);
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&path), 0o600);

        // A file left readable by an older release is narrowed and emptied.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        drop(open_log(&path).unwrap());
        assert_eq!(mode(&path), 0o600);
        assert_eq!(std::fs::read(&path).unwrap(), b"");

        std::fs::remove_dir_all(&root).unwrap();
    }
}
