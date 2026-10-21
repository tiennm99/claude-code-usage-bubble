//! Process handoff: relaunching the app and cleaning up after earlier
//! updates.
//!
//! A relaunched instance gets `--wait-pid <our pid>` and waits for this
//! process to exit before it takes the single-instance lock.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::core::settings::APP_DIR_NAME;

/// Spawn a detached copy of the app with `--wait-pid <pid>` followed by
/// `extra_args`. The caller exits only when this returns `Ok`, so a failed
/// spawn never leaves the user without a running app.
pub fn relaunch(extra_args: &[OsString]) -> io::Result<()> {
    let exe = relaunch_target()?;
    let mut command = Command::new(&exe);
    command
        .arg("--wait-pid")
        .arg(std::process::id().to_string())
        .args(extra_args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detach(&mut command);
    let child = command.spawn()?;
    log::info!(
        "relaunched {} as pid {} (parent pid {})",
        exe.display(),
        child.id(),
        std::process::id()
    );
    Ok(())
}

/// The file to start: the AppImage itself when running from one (the
/// executable path points into its temporary mount), else this executable.
fn relaunch_target() -> io::Result<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        if let Some(appimage) = std::env::var_os("APPIMAGE").filter(|v| !v.is_empty()) {
            return Ok(PathBuf::from(appimage));
        }
    }
    std::env::current_exe()
}

#[cfg(windows)]
fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    // A new process group, so a Ctrl+C aimed at the old instance's
    // terminal does not also stop the new one.
    command.process_group(0);
}

/// `data_local_dir()/ClaudeCodeUsageBubble/updates`, where updates are
/// staged and `update-error.log` lives.
pub fn updates_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join(APP_DIR_NAME).join("updates"))
}

/// Delete what earlier updates left behind: `<exe file name>.old.*` next to
/// the executable and, in `updates_dir`, the staged `update.exe` and
/// `updater-helper-*.exe` copies. `update-error.log` is kept. Every failure
/// is logged and skipped.
pub fn cleanup_stale_files(exe: &Path, updates_dir: Option<&Path>) {
    if let (Some(dir), Some(name)) = (exe.parent(), exe.file_name()) {
        let prefix = format!("{}.old.", name.to_string_lossy());
        remove_matching(dir, |file| file.starts_with(&prefix));
    }
    if let Some(dir) = updates_dir {
        remove_matching(dir, |file| {
            file == "update.exe" || (file.starts_with("updater-helper-") && file.ends_with(".exe"))
        });
    }
}

fn remove_matching(dir: &Path, matches: impl Fn(&str) -> bool) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            if err.kind() != io::ErrorKind::NotFound {
                log::debug!("cleanup: read_dir {} failed: {err}", dir.display());
            }
            return;
        }
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !matches(&name.to_string_lossy()) {
            continue;
        }
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => log::info!("cleanup: removed {}", path.display()),
            Err(err) => log::debug!("cleanup: remove {} failed: {err}", path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// A fresh, empty directory under the system temp dir.
    fn scratch_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("ccub-handoff-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    fn touch(path: &Path) {
        std::fs::write(path, b"x").expect("write file");
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("read dir")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn cleanup_removes_only_leftovers_from_updates() {
        let root = scratch_dir("cleanup");
        let install = root.join("install");
        let updates = root.join("updates");
        std::fs::create_dir_all(&install).expect("install dir");
        std::fs::create_dir_all(&updates).expect("updates dir");

        let exe = install.join("claude-code-usage-bubble.exe");
        touch(&exe);
        touch(&install.join("claude-code-usage-bubble.exe.old.1234"));
        touch(&install.join("claude-code-usage-bubble.exe.old.99"));
        touch(&install.join("other.exe.old.1"));
        touch(&install.join("settings.json"));
        // A directory with a matching name is left alone.
        std::fs::create_dir_all(install.join("claude-code-usage-bubble.exe.old.dir")).expect("dir");

        touch(&updates.join("update.exe"));
        touch(&updates.join("updater-helper-4242.exe"));
        touch(&updates.join("updater-helper-notes.txt"));
        touch(&updates.join("update-error.log"));

        cleanup_stale_files(&exe, Some(&updates));

        assert_eq!(
            names(&install),
            [
                "claude-code-usage-bubble.exe",
                "claude-code-usage-bubble.exe.old.dir",
                "other.exe.old.1",
                "settings.json",
            ]
        );
        assert_eq!(
            names(&updates),
            ["update-error.log", "updater-helper-notes.txt"]
        );
        std::fs::remove_dir_all(&root).expect("remove scratch dir");
    }

    #[test]
    fn cleanup_tolerates_missing_directories() {
        let root = scratch_dir("missing");
        let exe = root.join("absent").join("app");
        cleanup_stale_files(&exe, Some(&root.join("no-updates")));
        cleanup_stale_files(Path::new("app"), None);
        std::fs::remove_dir_all(&root).expect("remove scratch dir");
    }

    #[test]
    fn updates_dir_sits_under_the_app_data_dir() {
        if let Some(dir) = updates_dir() {
            assert!(dir.ends_with(Path::new(APP_DIR_NAME).join("updates")));
        }
    }
}
