---
phase: 3
title: "Update install: rename + move + native spawn"
status: complete
priority: P1
effort: "2h"
dependencies: [1]
---

# Phase 3: Update install: rename + move + native spawn

## Overview

Replace `src/update/install.rs::begin`'s `cmd.exe /c "timeout & move & start ..."` handoff with native steps: `MoveFileExW` to rename the running exe sideways, `MoveFileExW` to move the staged exe into place, then `spawn_detached` of the new binary. Removes the only remaining cmd.exe invocation in the update flow.

## Requirements

**Functional**
- Update install produces zero console flash.
- New binary version starts after auto-update without user interaction.
- SHA-256 verification continues to gate the swap (no swap on checksum mismatch).
- On any failure step, the original exe must remain runnable (no half-state).

**Non-functional**
- `install.rs` net change ≈ -30 lines (cmd-quoting code is gone, replaced by short Win32 calls).
- New code paths use the windows crate; no new dependencies.

## Architecture

```
install::begin(http, release):
  1. current = current_exe()
  2. ensure_writable(current.parent())        (unchanged)
  3. staging = stage_path()
  4. reject_unsafe_path(current)              (kept for defense-in-depth; harmless now)
  5. reject_unsafe_path(staging)
  6. create_dir_all(staging.parent())
  7. download(http, asset_url, staging, sha256)   (unchanged)
  8. backup = current.with_file_name(format!("{}.old.{}", filename, pid))
  9. MoveFileExW(current, backup, 0)          ← NEW: rename running exe sideways
  10. MoveFileExW(staging, current, MOVEFILE_REPLACE_EXISTING)   ← NEW
  11. our_pid = GetCurrentProcessId()
  12. handoff::spawn_detached(current,
        ["--wait-pid", our_pid, "--updated-to", version_str])    ← NEW
  13. return Ok(())
```

Caller (`app.rs` Apply action) is responsible for `PostQuitMessage` after `begin` returns Ok — same as today.

### Rollback semantics

| Step that failed | State | Recovery |
|---|---|---|
| 7 (download) | Original exe untouched | Existing behavior: error surfaced, user retries |
| 9 (rename current → backup) | Original exe untouched | Surface `Error::NotWritable`; do not proceed |
| 10 (move staging → current) | Original exe is at backup path, current path empty | Best-effort revert: rename backup back to current; surface error |
| 10 + revert (both fail) | Original at backup path; current path empty; user has no runnable binary at the install location | **Per Validation Session 1 decision:** show a Windows `MessageBoxW` (MB_OK \| MB_ICONERROR) telling the user where the backup is, then exit. Message: "Update failed. Your original binary is saved as `{backup_path}`. Please rename it back to `{exe_name}` manually." |
| 12 (spawn child) | New exe at correct path, but app didn't restart | Log + tray balloon "Update applied; restart manually". Rare — `CreateProcessW` on a fresh fully-written exe almost never fails |

<!-- Updated: Validation Session 1 - Rollback escalation MessageBox added -->

### Rollback escalation helper

Add a private `surface_rollback_failure(backup_path: &Path, target_name: &str)` helper that calls `MessageBoxW` with `MB_OK | MB_ICONERROR` and the localized message. Adds a new `LocaleStrings` field `update_rollback_failed_body` parameterized with `{backup_path}` and `{exe_name}` (Rust `format!` substitution at call site). The plain MessageBox uses the Win32 dialog, so no console can flash.

## Related Code Files

- **Modify**: `src/update/install.rs::begin`
- **Modify**: `src/update/install.rs::spawn_handoff` → REMOVED entirely
- **Modify**: `src/update/install.rs` imports (drop `os::windows::process::CommandExt`, `process::{Command, Stdio}`; add `MoveFileExW`, `MOVEFILE_REPLACE_EXISTING`, `GetCurrentProcessId`, `MessageBoxW`, `MB_OK`, `MB_ICONERROR`)
- **Modify**: `src/update/mod.rs::Error` — add `Error::SwapFailed(String)` variant if MoveFileExW failures don't fit existing variants cleanly
- **Modify**: `src/i18n/mod.rs::LocaleStrings` — add `update_rollback_failed_body: String` (also belongs to Phase 4 i18n group, but Phase 3 is the consumer)

## Implementation Steps

1. **Read current `install.rs::begin` + `spawn_handoff`** to confirm exact bounds (lines 19-36 + 99-121).

2. **Decide path-safety policy**: keep `reject_unsafe_path` (the `%`-check) as defense-in-depth even though no cmd.exe runs. Update the function-level comment to reflect new reality (kept for paranoia, not strict need).

3. **Add `swap_and_spawn` private helper** (replaces `spawn_handoff`):
   ```rust
   fn swap_and_spawn(
       source: &Path,
       target: &Path,
       version: &super::release::Version,
   ) -> Result<(), super::Error> {
       let backup = backup_path(target);
       move_file(target, &backup, 0)?;
       if let Err(e) = move_file(source, target, MOVEFILE_REPLACE_EXISTING) {
           // Best-effort revert
           let _ = move_file(&backup, target, MOVEFILE_REPLACE_EXISTING);
           return Err(e);
       }
       let pid = unsafe { GetCurrentProcessId() };
       let args = vec![
           OsString::from("--wait-pid"),
           OsString::from(pid.to_string()),
           OsString::from("--updated-to"),
           OsString::from(format!("{}.{}.{}", version.major, version.minor, version.patch)),
       ];
       super::handoff::spawn_detached(target, &args)
           .map_err(super::Error::Io)
   }

   fn move_file(src: &Path, dst: &Path, flags: MOVE_FILE_FLAGS) -> Result<(), super::Error> {
       let src_w = to_utf16_nul(src);
       let dst_w = to_utf16_nul(dst);
       let r = unsafe {
           MoveFileExW(
               PCWSTR::from_raw(src_w.as_ptr()),
               PCWSTR::from_raw(dst_w.as_ptr()),
               flags,
           )
       };
       r.ok().map_err(|e| super::Error::SwapFailed(e.to_string()))
   }

   fn backup_path(target: &Path) -> PathBuf {
       let pid = unsafe { GetCurrentProcessId() };
       let mut p = target.to_owned();
       let fname = target.file_name().map(|s| s.to_string_lossy().into_owned())
           .unwrap_or_else(|| "exe".to_string());
       p.set_file_name(format!("{fname}.old.{pid}"));
       p
   }
   ```
   Use the existing `os::to_utf16_nul` helper for wide-char conversion (already used in `app.rs::run`).

4. **Rewrite `begin`** to call `swap_and_spawn(&staging, &current, &release.version)` instead of `spawn_handoff(&staging, &current)`. Pass the version from the `Release` struct already in hand.

5. **Add `Error::SwapFailed(String)` variant** to `src/update/mod.rs` if no existing variant fits the move-failure semantics. The `#[error(...)]` message should be `"file swap failed: {0}"`.

6. **Remove `spawn_handoff` function** entirely. Remove now-unused imports (`std::os::windows::process::CommandExt`, `Command`, `Stdio`, `CREATE_NO_WINDOW`, `DETACHED_PROCESS` constants).

7. **Compile check**: `cargo build --release`. Address any feature-flag gaps (likely need `Win32_Storage_FileSystem` added to Cargo `windows` features for `MoveFileExW`).

8. **Test rollback path manually**: write a temp .exe to staging that is read-only or has wrong permissions to force step 10 to fail; verify backup is restored and original is still runnable.

## Success Criteria

- [ ] `cargo build --release` clean.
- [ ] Manual auto-update from a test-tagged v0.1.99 produces ZERO visible console window across 5 consecutive runs.
- [ ] SHA-256 mismatch still rejects the swap (verify by tampering with a downloaded asset before swap).
- [ ] On forced step-10 failure (simulated): backup is restored, original binary still launches.
- [ ] `spawn_handoff` function no longer exists in the codebase (`grep -r spawn_handoff src/` returns empty).
- [ ] No `cmd.exe` string remains in `src/update/install.rs`.

## Risk Assessment

| Risk | Mitigation |
|---|---|
| `MoveFileExW` fails because AV holds a handle on the running exe | Surface `Error::SwapFailed`; user retries. Rare on Defender (which scans on read, not perpetually) |
| Two updates triggered in quick succession leave two `.old.<pid>` files | Phase 4 cleanup at startup handles this — glob removes ALL `.old.*` siblings |
| User's install dir is on a network share where renaming-while-open is forbidden | Existing `ensure_writable` probe catches read-only / no-write cases. Network FS oddities → surface as `NotWritable` |
| `MoveFileExW` with `REPLACE_EXISTING` on a non-NTFS volume | Works on FAT32 (per MS docs); the renaming-while-running concern is NTFS-specific but step 9 always renames an empty (just-emptied) path in step 10 |
| Release-build inlines + LTO breaks symbol-level rollback assumption | Functional rollback path is exercised by phase 5's manual test under release profile |
