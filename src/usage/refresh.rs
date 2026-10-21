// Token refresh orchestrator.
//
// Each provider's credential source advertises a `RefreshHint` describing
// which CLI to spawn. We invoke that CLI and watch the credential file's
// signature; if it changes within the timeout we declare success.

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::creds::{CredentialSource, RefreshHint};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Directories searched after `PATH` on Unix. A desktop session often starts
/// GUI apps with a shorter `PATH` than the login shell, which misses the
/// usual per-user install locations of the Claude and Codex CLIs.
#[cfg(unix)]
const EXTRA_HOME_DIRS: &[&str] = &[".local/bin", ".claude/local"];
#[cfg(unix)]
const EXTRA_SYSTEM_DIRS: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Refreshed,
    StillExpired,
    CliMissing,
    Timeout,
}

pub struct Orchestrator {
    timeout: Duration,
    poll_interval: Duration,
}

impl Orchestrator {
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            poll_interval: Duration::from_millis(500),
        }
    }

    pub fn refresh(&self, source: &dyn CredentialSource) -> Outcome {
        let initial_sig = source.signature();
        let hint = source.refresh_hint();
        if !spawn_cli(&hint) {
            return Outcome::CliMissing;
        }
        let start = Instant::now();
        while start.elapsed() < self.timeout {
            std::thread::sleep(self.poll_interval);
            if source.signature() != initial_sig {
                return Outcome::Refreshed;
            }
        }
        if source.signature() != initial_sig {
            Outcome::Refreshed
        } else {
            Outcome::Timeout
        }
    }
}

fn spawn_cli(hint: &RefreshHint) -> bool {
    let (candidates, args) = cli_for(hint);
    spawn_first(candidates, args)
}

#[cfg(windows)]
fn cli_for(hint: &RefreshHint) -> (&'static [&'static str], &'static [&'static str]) {
    match hint {
        RefreshHint::LocalClaudeCli => (&["claude.cmd", "claude.exe", "claude"], &["-p", "."]),
        RefreshHint::LocalCodexCli => (
            &["codex.cmd", "codex.ps1", "codex.exe", "codex"],
            &["exec", "."],
        ),
    }
}

#[cfg(unix)]
fn cli_for(hint: &RefreshHint) -> (&'static [&'static str], &'static [&'static str]) {
    match hint {
        RefreshHint::LocalClaudeCli => (&["claude"], &["-p", "."]),
        RefreshHint::LocalCodexCli => (&["codex"], &["exec", "."]),
    }
}

/// Spawn the first candidate that starts. The child is detached: a reaper
/// thread waits on it so no zombie is left behind.
fn spawn_first(candidates: &[&str], args: &[&str]) -> bool {
    for name in candidates {
        let Some(mut cmd) = command_for(name, args) else {
            continue;
        };
        cmd.env_remove("CLAUDECODE")
            .env_remove("CLAUDE_CODE_ENTRYPOINT")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        if let Ok(child) = cmd.spawn() {
            reap_detached(child);
            return true;
        }
    }
    false
}

fn reap_detached(mut child: Child) {
    std::thread::spawn(move || {
        if let Err(err) = child.wait() {
            log::warn!("refresh CLI could not be reaped: {err}");
        }
    });
}

#[cfg(windows)]
fn command_for(name: &str, args: &[&str]) -> Option<Command> {
    let lower = name.to_ascii_lowercase();
    let mut cmd = if lower.ends_with(".ps1") {
        let mut c = Command::new("powershell.exe");
        c.arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(name);
        c
    } else if lower.ends_with(".cmd") || lower.ends_with(".bat") {
        let mut c = Command::new("cmd.exe");
        c.arg("/c").arg(name);
        c
    } else {
        Command::new(name)
    };
    cmd.args(args);
    Some(cmd)
}

#[cfg(unix)]
fn command_for(name: &str, args: &[&str]) -> Option<Command> {
    let path_var = std::env::var_os("PATH");
    let home = dirs::home_dir();
    let dirs = search_dirs(path_var.as_deref(), home.as_deref());
    command_in(name, args, &dirs)
}

/// Resolve `name` in `dirs` and run it with `PATH` set to the same list.
/// npm-installed CLIs are `#!/usr/bin/env node` scripts, so the child has to
/// see the extra install directories too: a GUI session's short `PATH`
/// would let the script start and then exit at once with "node not found".
#[cfg(unix)]
fn command_in(name: &str, args: &[&str], dirs: &[std::path::PathBuf]) -> Option<Command> {
    let program = find_executable(name, dirs)?;
    let path = match std::env::join_paths(dirs) {
        Ok(path) => path,
        Err(err) => {
            log::warn!("refresh CLI search path cannot be joined: {err}");
            return None;
        }
    };
    let mut cmd = Command::new(program);
    cmd.args(args).env("PATH", path);
    Some(cmd)
}

/// `PATH` entries in order, then the extra install locations, without
/// duplicates. Empty and relative entries are skipped so the working
/// directory is never searched.
#[cfg(unix)]
fn search_dirs(
    path_var: Option<&std::ffi::OsStr>,
    home: Option<&std::path::Path>,
) -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;

    let mut out: Vec<PathBuf> = Vec::new();
    let from_path = path_var.map(|p| std::env::split_paths(p).collect::<Vec<_>>());
    let extras_home = home
        .into_iter()
        .flat_map(|h| EXTRA_HOME_DIRS.iter().map(move |d| h.join(d)));
    let extras_system = EXTRA_SYSTEM_DIRS.iter().map(PathBuf::from);
    for dir in from_path
        .unwrap_or_default()
        .into_iter()
        .chain(extras_home)
        .chain(extras_system)
    {
        if dir.is_absolute() && !out.contains(&dir) {
            out.push(dir);
        }
    }
    out
}

/// First `dir/name` that is a regular file with an execute bit set.
#[cfg(unix)]
fn find_executable(name: &str, dirs: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    dirs.iter().map(|d| d.join(name)).find(|candidate| {
        std::fs::metadata(candidate)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    })
}

#[cfg(all(test, unix))]
mod tests {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ccub-refresh-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn write_file(path: &Path, mode: u32) {
        std::fs::write(path, "#!/bin/sh\n").expect("write");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
    }

    #[test]
    fn search_dirs_puts_path_first_then_extras_without_duplicates() {
        let path = OsString::from("/usr/bin::relative/bin:/usr/local/bin:/usr/bin");
        let dirs = search_dirs(Some(&path), Some(Path::new("/home/u")));
        let expected: Vec<PathBuf> = [
            "/usr/bin",
            "/usr/local/bin",
            "/home/u/.local/bin",
            "/home/u/.claude/local",
            "/opt/homebrew/bin",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        assert_eq!(dirs, expected);
    }

    #[test]
    fn search_dirs_without_path_or_home_keeps_system_extras() {
        let dirs = search_dirs(None, None);
        assert_eq!(
            dirs,
            vec![
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/usr/local/bin")
            ]
        );
    }

    #[test]
    fn find_executable_skips_missing_and_non_executable_files() {
        let first = temp_dir("first");
        let second = temp_dir("second");
        write_file(&first.join("claude"), 0o644);
        std::fs::create_dir_all(first.join("codex")).expect("dir named like the CLI");
        write_file(&second.join("claude"), 0o755);
        let dirs = vec![first.clone(), second.clone()];
        assert_eq!(
            find_executable("claude", &dirs),
            Some(second.join("claude"))
        );
        assert_eq!(find_executable("codex", &dirs), None);
        assert_eq!(find_executable("absent", &dirs), None);
        let _ = std::fs::remove_dir_all(first);
        let _ = std::fs::remove_dir_all(second);
    }

    #[test]
    fn spawned_child_is_reaped() {
        let dir = temp_dir("spawn");
        let script = dir.join("fake-cli");
        let marker = dir.join("ran");
        std::fs::write(
            &script,
            format!("#!/bin/sh\necho \"$@\" > '{}'\n", marker.display()),
        )
        .expect("write script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let name = script.to_str().expect("utf8 path");
        assert!(spawn_first(
            &["definitely-not-a-cli-name", name],
            &["-p", "."]
        ));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !marker.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let written = std::fs::read_to_string(&marker).unwrap_or_default();
        assert_eq!(written.trim(), "-p .");
        assert!(!spawn_first(&["definitely-not-a-cli-name"], &[]));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn child_path_resolves_env_interpreter_from_extra_dirs() {
        let tools = temp_dir("tools");
        let bin = temp_dir("bin");
        let marker = bin.join("ran");
        let helper = "ccub-fake-interpreter";
        std::fs::write(
            tools.join(helper),
            format!("#!/bin/sh\necho interpreted > '{}'\n", marker.display()),
        )
        .expect("write helper");
        std::fs::set_permissions(tools.join(helper), std::fs::Permissions::from_mode(0o755))
            .expect("chmod helper");
        std::fs::write(bin.join("fake-cli"), format!("#!/usr/bin/env {helper}\n"))
            .expect("write script");
        std::fs::set_permissions(bin.join("fake-cli"), std::fs::Permissions::from_mode(0o755))
            .expect("chmod script");
        let dirs = vec![
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
            bin.clone(),
            tools.clone(),
        ];
        let mut cmd = command_in("fake-cli", &[], &dirs).expect("script is found");
        let status = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("script runs");
        assert!(status.success(), "child exited with {status}");
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap_or_default().trim(),
            "interpreted"
        );
        assert!(command_in("absent-cli", &[], &dirs).is_none());
        let _ = std::fs::remove_dir_all(tools);
        let _ = std::fs::remove_dir_all(bin);
    }
}
