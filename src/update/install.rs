// Download a release asset and hand off via inline `cmd /c`.
//
// We avoid the helper-exe pattern entirely: after writing the new .exe
// to a staging path, we spawn cmd.exe with a one-liner that waits 2 s,
// moves the new binary over the running one (Windows releases the file
// lock when our process exits), and relaunches it.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

use crate::net::Client;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DETACHED_PROCESS: u32 = 0x0000_0008;

pub fn begin(http: &Client, release: &super::Release) -> Result<(), super::Error> {
    let current = std::env::current_exe()?;
    ensure_writable(&current)?;
    let staging = stage_path()?;
    // Refuse to proceed if either path contains `%`. Inside double quotes
    // cmd.exe still expands `%var%` references, so a path containing `%`
    // would let cmd substitute environment variables into the swap step.
    // Such paths are vanishingly rare on real Windows installs; failing
    // fast is safer than rolling a bespoke cmd-escape layer.
    reject_unsafe_path(&current)?;
    reject_unsafe_path(&staging)?;
    if let Some(parent) = staging.parent() {
        std::fs::create_dir_all(parent)?;
    }
    download(http, &release.asset_url, &staging, release.asset_sha256.as_ref())?;
    spawn_handoff(&staging, &current)?;
    Ok(())
}

/// CLI entry-point compatibility for `--apply-update <target> <source> <pid>`.
/// The inline-cmd handoff already does the swap-and-restart; if this binary
/// is invoked with the legacy flag (e.g. from an older release's helper)
/// just exit cleanly so the upgrade still completes.
pub fn run_cli(args: &[String]) -> Option<i32> {
    if args.len() >= 2 && args[1] == "--apply-update" {
        Some(0)
    } else {
        None
    }
}

fn download(
    http: &Client,
    url: &str,
    to: &std::path::Path,
    expected_sha256: Option<&[u8; 32]>,
) -> Result<(), super::Error> {
    let resp = http
        .get(url)
        .header("User-Agent", super::release::user_agent())
        .send()?;
    if !(200..300).contains(&resp.status()) {
        return Err(super::Error::Network(crate::net::Error::Status(resp.status())));
    }
    let body = resp.body();
    if let Some(expected) = expected_sha256 {
        let mut hasher = Sha256::new();
        hasher.update(body);
        let actual = hasher.finalize();
        if actual.as_slice() != expected {
            return Err(super::Error::ChecksumMismatch {
                expected: hex_encode(expected),
                actual: hex_encode(&actual),
            });
        }
    }
    std::fs::write(to, body)?;
    Ok(())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn reject_unsafe_path(p: &std::path::Path) -> Result<(), super::Error> {
    let s = p.to_string_lossy();
    if s.contains('%') {
        return Err(super::Error::UnsafePath(format!(
            "path contains '%' which cmd.exe expands as a variable: {s}"
        )));
    }
    Ok(())
}

fn spawn_handoff(source: &std::path::Path, target: &std::path::Path) -> Result<(), super::Error> {
    let src_str = source.to_string_lossy().replace('"', "");
    let tgt_str = target.to_string_lossy().replace('"', "");
    // 2-second wait gives the current process time to exit and release the
    // file lock before `move` overwrites it.
    let cmd = format!(
        r#"timeout /t 2 /nobreak >nul & move /y "{src_str}" "{tgt_str}" & start "" "{tgt_str}""#
    );
    // raw_arg bypasses Rust's std auto-escaping which would turn the inner
    // `"` characters into `\"`. cmd.exe does not recognise `\"`, so the
    // escaped form makes `start` see the path as `\\` and emit a
    // "Windows cannot find '\\'" dialog. Feeding the command line raw
    // preserves the quotes cmd.exe actually expects.
    Command::new("cmd.exe")
        .raw_arg("/c")
        .raw_arg(format!("\"{cmd}\""))
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

fn stage_path() -> Result<PathBuf, super::Error> {
    let base = dirs::data_local_dir().ok_or_else(|| {
        super::Error::NotWritable("no local data directory available".to_string())
    })?;
    Ok(base
        .join("ClaudeCodeUsageBubble")
        .join("updates")
        .join("update.exe"))
}

fn ensure_writable(target: &std::path::Path) -> Result<(), super::Error> {
    let parent = target.parent().ok_or_else(|| {
        super::Error::NotWritable("could not resolve install directory".to_string())
    })?;
    let probe = parent.join(".__bubble_update_probe");
    std::fs::write(&probe, b"").map_err(|e| super::Error::NotWritable(e.to_string()))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}
