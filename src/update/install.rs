//! Applying a downloaded, signature-checked update to the portable exe, and
//! the paths under `updates_dir` that updates use.
//!
//! The running process cannot reliably replace its own mapped image on
//! Windows; `self_replace` handles the rename and copy sequence for that.
//! The caller then relaunches the installed path with `--wait-pid` and exits.

use std::fmt::Display;
use std::io;
use std::path::{Path, PathBuf};

use super::Error;
use crate::handoff;

/// File name the download is staged under before the swap. Startup cleanup
/// deletes a leftover one.
const STAGED_EXE: &str = "update.exe";

/// File name of the last apply failure's log.
const ERROR_LOG: &str = "update-error.log";

/// `updates_dir()/update.exe`.
pub fn stage_path() -> Result<PathBuf, Error> {
    updates_file(STAGED_EXE)
}

/// `updates_dir()/update-error.log`.
pub fn error_log_path() -> Result<PathBuf, Error> {
    updates_file(ERROR_LOG)
}

fn updates_file(name: &str) -> Result<PathBuf, Error> {
    handoff::updates_dir()
        .map(|dir| dir.join(name))
        .ok_or_else(|| Error::NotWritable("no local data directory available".to_string()))
}

/// Replace the running executable with `bytes`.
pub fn replace_portable(bytes: &[u8]) -> Result<(), Error> {
    let current = std::env::current_exe()?;
    let staging = stage_path()?;
    stage_and_swap(&current, &staging, bytes, |staged| {
        self_replace::self_replace(staged)
    })
}

/// Write `bytes` to `staging`, then `swap` it in for `target`. The staged
/// file is removed afterwards, whether the swap worked or not.
fn stage_and_swap(
    target: &Path,
    staging: &Path,
    bytes: &[u8],
    swap: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<(), Error> {
    ensure_writable(target)?;
    // No shell parses these paths today; the rejection keeps any future
    // hand-off through an external tool safe from `%`-expansion.
    reject_unsafe_path(target)?;
    reject_unsafe_path(staging)?;
    if let Some(parent) = staging.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(staging, bytes)?;
    let swapped = swap(staging).map_err(|e| {
        Error::SwapFailed(format!(
            "self_replace({} -> {}): {e}",
            staging.display(),
            target.display()
        ))
    });
    if let Err(err) = std::fs::remove_file(staging) {
        log::debug!("staged update {} not removed: {err}", staging.display());
    }
    swapped
}

/// Fail early when the install directory cannot take a new file, which
/// `self_replace` needs.
fn ensure_writable(target: &Path) -> Result<(), Error> {
    let parent = target
        .parent()
        .ok_or_else(|| Error::NotWritable("could not resolve install directory".to_string()))?;
    let probe = parent.join(".__bubble_update_probe");
    std::fs::write(&probe, b"").map_err(|e| Error::NotWritable(e.to_string()))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

fn reject_unsafe_path(p: &Path) -> Result<(), Error> {
    let s = p.to_string_lossy();
    if s.contains('%') {
        return Err(Error::UnsafePath(format!("path contains '%': {s}")));
    }
    Ok(())
}

/// Record an apply failure in `update-error.log`, replacing the previous
/// entry. Failures to write it are only logged.
pub fn write_update_error(error: &impl Display) {
    let path = match error_log_path() {
        Ok(path) => path,
        Err(err) => {
            log::warn!("update error log skipped: {err}");
            return;
        }
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let unix_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    if let Err(err) = std::fs::write(&path, error_log_body(unix_time, error)) {
        log::warn!("update error log {} not written: {err}", path.display());
    }
}

/// The log format every release has written.
fn error_log_body(unix_time: u64, error: &impl Display) -> String {
    format!("unix_time={unix_time}\nerror={error}\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("ccub-install-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[test]
    fn staging_and_log_paths_sit_in_the_updates_dir() {
        let (Ok(stage), Ok(log)) = (stage_path(), error_log_path()) else {
            // No local data dir on this host; the error path is covered by
            // the `NotWritable` mapping.
            return;
        };
        let dir = handoff::updates_dir().expect("updates dir");
        assert_eq!(stage, dir.join("update.exe"));
        assert_eq!(log, dir.join("update-error.log"));
    }

    #[test]
    fn swap_gets_the_staged_bytes_and_the_stage_is_removed() {
        let root = scratch_dir("swap");
        let target = root.join("app.exe");
        std::fs::write(&target, b"old").expect("target");
        let staging = root.join("updates").join("update.exe");

        stage_and_swap(&target, &staging, b"new", |staged| {
            std::fs::copy(staged, &target).map(|_| ())
        })
        .expect("swap");

        assert_eq!(std::fs::read(&target).expect("read"), b"new");
        assert!(!staging.exists());
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn failed_swap_keeps_the_target_and_removes_the_stage() {
        let root = scratch_dir("fail");
        let target = root.join("app.exe");
        std::fs::write(&target, b"old").expect("target");
        let staging = root.join("update.exe");

        let err = stage_and_swap(&target, &staging, b"new", |_| {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "locked"))
        })
        .expect_err("swap fails");

        assert!(matches!(err, Error::SwapFailed(ref m) if m.contains("locked")));
        assert_eq!(std::fs::read(&target).expect("read"), b"old");
        assert!(!staging.exists());
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn percent_paths_are_rejected_before_anything_is_written() {
        let root = scratch_dir("unsafe");
        let target = root.join("app.exe");
        let staging = root.join("%TEMP%").join("update.exe");

        let err = stage_and_swap(&target, &staging, b"new", |_| Ok(())).expect_err("rejected");

        assert!(matches!(err, Error::UnsafePath(_)));
        assert!(!staging.exists());
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn missing_install_dir_is_not_writable() {
        let root = scratch_dir("missing");
        let target = root.join("absent").join("app.exe");
        let err = stage_and_swap(&target, &root.join("update.exe"), b"new", |_| Ok(()))
            .expect_err("not writable");
        assert!(matches!(err, Error::NotWritable(_)));
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn error_log_keeps_its_format() {
        assert_eq!(
            error_log_body(1_700_000_000, &"network: timed out"),
            "unix_time=1700000000\nerror=network: timed out\n"
        );
    }
}
