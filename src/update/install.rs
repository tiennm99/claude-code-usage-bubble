// Download a release asset and swap it in via native Win32 calls.
//
// After writing the new .exe to a staging path and verifying its
// SHA-256, we `MoveFileExW` the running exe sideways (so Windows
// releases the file lock on our own image), then `MoveFileExW` the
// staged exe into place, then spawn the new binary detached via
// `handoff::spawn_detached`. No shell, no console allocation.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    MoveFileExW, MOVEFILE_COPY_ALLOWED, MOVEFILE_REPLACE_EXISTING, MOVE_FILE_FLAGS,
};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONERROR, MB_OK,
};

use crate::net::Client;
use crate::os::to_utf16_nul;

pub fn begin(http: &Client, release: &super::Release) -> Result<(), super::Error> {
    let current = std::env::current_exe()?;
    ensure_writable(&current)?;
    let staging = stage_path()?;
    // Defense in depth: `MoveFileExW` itself is immune to `%`-expansion
    // (no shell parses our paths), but the existing rejection guards
    // future code paths that might invoke external tools, so keep it.
    reject_unsafe_path(&current)?;
    reject_unsafe_path(&staging)?;
    if let Some(parent) = staging.parent() {
        std::fs::create_dir_all(parent)?;
    }
    download(http, &release.asset_url, &staging, release.asset_sha256.as_ref())?;
    swap_and_spawn(&staging, &current, &release.version)?;
    Ok(())
}

/// CLI entry-point compatibility for `--apply-update <target> <source> <pid>`.
/// The native handoff already does the swap-and-restart; if this binary
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
    to: &Path,
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

fn reject_unsafe_path(p: &Path) -> Result<(), super::Error> {
    let s = p.to_string_lossy();
    if s.contains('%') {
        return Err(super::Error::UnsafePath(format!(
            "path contains '%': {s}"
        )));
    }
    Ok(())
}

fn swap_and_spawn(
    source: &Path,
    target: &Path,
    version: &super::release::Version,
) -> Result<(), super::Error> {
    let backup = backup_path(target);
    // Step 1: rename running exe sideways. Windows allows renaming a
    // file even while its image is mapped into memory; this releases
    // the lock on the original `target` path. Same directory by
    // construction, so plain MoveFileExW with no flags is sufficient.
    move_file(target, &backup, MOVE_FILE_FLAGS(0))?;

    // Step 2: move staged exe into place. Staging lives under
    // %LOCALAPPDATA%, target lives wherever the user installed —
    // COPY_ALLOWED lets MoveFileExW fall back to copy+delete when
    // the two paths cross volumes (portable installs on D:/E:/etc.).
    let step2_flags = MOVEFILE_REPLACE_EXISTING | MOVEFILE_COPY_ALLOWED;
    if let Err(swap_err) = move_file(source, target, step2_flags) {
        // Best-effort revert. Same volume, no COPY_ALLOWED needed.
        if let Err(revert_err) = move_file(&backup, target, MOVEFILE_REPLACE_EXISTING) {
            log::error!("rollback also failed: {revert_err}; surfacing modal");
            let target_name = target
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "claude-code-usage-bubble.exe".to_string());
            surface_rollback_failure(&backup, &target_name);
        }
        return Err(swap_err);
    }

    // Step 3: spawn the new exe detached with --wait-pid + --updated-to.
    let pid = unsafe { GetCurrentProcessId() };
    let version_str = format!("{}.{}.{}", version.major, version.minor, version.patch);
    let args = vec![
        OsString::from("--wait-pid"),
        OsString::from(pid.to_string()),
        OsString::from("--updated-to"),
        OsString::from(version_str),
    ];
    if let Err(spawn_err) = super::handoff::spawn_detached(target, &args) {
        // New binary is on disk but won't auto-launch. Roll back so
        // the user's next "Restart" stays on the known-good version.
        log::error!("spawn_detached failed after swap: {spawn_err}; attempting revert");
        if let Err(revert_err) = move_file(&backup, target, MOVEFILE_REPLACE_EXISTING) {
            log::error!("post-spawn revert failed: {revert_err}");
        }
        return Err(super::Error::Io(spawn_err));
    }
    Ok(())
}

fn move_file(src: &Path, dst: &Path, flags: MOVE_FILE_FLAGS) -> Result<(), super::Error> {
    let src_w = to_utf16_nul(&src.to_string_lossy());
    let dst_w = to_utf16_nul(&dst.to_string_lossy());
    let result = unsafe {
        MoveFileExW(
            PCWSTR::from_raw(src_w.as_ptr()),
            PCWSTR::from_raw(dst_w.as_ptr()),
            flags,
        )
    };
    result.map_err(|e| {
        super::Error::SwapFailed(format!(
            "MoveFileExW({} -> {}): {e}",
            src.display(),
            dst.display()
        ))
    })
}

fn backup_path(target: &Path) -> PathBuf {
    let pid = unsafe { GetCurrentProcessId() };
    let fname = target
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "exe".to_string());
    let mut p = target.to_owned();
    p.set_file_name(format!("{fname}.old.{pid}"));
    p
}

fn surface_rollback_failure(backup: &Path, target_name: &str) {
    // Pull the localized body from i18n; the caller passes the
    // user-meaningful filename so we can format it in-place.
    let strings = crate::i18n::I18n::load(None).strings().clone();
    let body = format!(
        "{}{}\n\n{}",
        strings.update_rollback_failed_body,
        backup.display(),
        target_name
    );
    let title_w = to_utf16_nul(&strings.update_failed);
    let body_w = to_utf16_nul(&body);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR::from_raw(body_w.as_ptr()),
            PCWSTR::from_raw(title_w.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
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

fn ensure_writable(target: &Path) -> Result<(), super::Error> {
    let parent = target.parent().ok_or_else(|| {
        super::Error::NotWritable("could not resolve install directory".to_string())
    })?;
    let probe = parent.join(".__bubble_update_probe");
    std::fs::write(&probe, b"").map_err(|e| super::Error::NotWritable(e.to_string()))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}
