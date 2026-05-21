# Silent in-app update + restart (no cmd.exe)

**Date:** 2026-05-21
**Author:** brainstormer
**Status:** approved (ready for `/ck:plan`)

## Problem

User sees an occasional flash terminal window. Two paths today spawn `cmd.exe /c "timeout ... & start ..."` for update install and app restart, with `CREATE_NO_WINDOW | DETACHED_PROCESS`. Combination of those flags + the inner `start ""` invocation can still emit a brief console flash on some Windows configs (Defender hooks, conhost init, AV inspection). Goal: zero-flash, fully silent auto-update + restart, with in-app notification.

## Scope

**In scope**
- `src/update/install.rs::begin` — kill cmd.exe handoff
- `src/app.rs::restart_app` — kill cmd.exe handoff
- New CLI flag `--wait-pid <pid>` on the main binary (cooperates with itself across update)
- Cleanup of stale `bubble.exe.old.*` siblings at startup
- Tray balloon "Updated to vX.Y.Z" on first run after auto-update

**Out of scope (split as follow-up)**
- `src/usage/refresh.rs` CLI spawns (`claude.cmd`, `codex.cmd`, `powershell.exe`, `wsl.exe`) — already use `CREATE_NO_WINDOW`; address separately if flash persists after this change
- `src/creds/wsl_bridge.rs` `wsl.exe` calls — same reasoning

## Approaches evaluated

| Approach | Decision | Rationale |
|---|---|---|
| A: Native rename + direct `CreateProcessW` + `--wait-pid` | **CHOSEN** | Zero cmd.exe ⇒ zero flash possible; single-binary preserved; matches existing Win32 style; recoverable on interrupt |
| B: Helper-exe pattern (`bubble-updater.exe`) | rejected | Reverses deliberate "no helper exe" decision (`src/update/install.rs:3-5`); release-pipeline change; helper bootstrap problem |
| C: NTFS POSIX atomic replace (`FileRenameInfoEx`) | rejected | Obscure API; harder failure modes with AV / image-protection; not worth the elegance trade-off |

## Final design — Approach A

### Update install flow

```
1. fetch_latest()                              (unchanged — pure HTTP via WinHTTP)
2. download(release_url, staging_path)         (unchanged — sha256 verify)
3. rename current.exe -> current.exe.old.<pid> (MoveFileExW, allowed while running)
4. move staging.exe   -> current.exe          (MoveFileExW REPLACE_EXISTING)
5. settings::save(snap)                        (defensive flush, unchanged)
6. release singleton mutex                     (explicit ReleaseMutex + CloseHandle)
7. CreateProcessW(current.exe, "--wait-pid <our_pid> --updated-to vX.Y.Z",
                  CREATE_NO_WINDOW | DETACHED_PROCESS)
8. PostQuitMessage(0)
```

### Restart flow (settings-change path)

```
1. settings::save(snap)
2. CreateProcessW(current.exe, "--wait-pid <our_pid>",
                  CREATE_NO_WINDOW | DETACHED_PROCESS)
3. release singleton mutex
4. PostQuitMessage(0)
```

### New instance startup additions

```rust
// Before acquiring Global\ClaudeCodeUsageBubble mutex:
if let Some(parent_pid) = parse_wait_pid_arg() {
    let h = OpenProcess(SYNCHRONIZE, FALSE, parent_pid)?;
    WaitForSingleObject(h, 5000);              // 5s cap; proceed regardless
    CloseHandle(h);
}

// After main window is up:
cleanup_old_exes(current_dir, "bubble.exe.old.*");

if let Some(version) = parse_updated_to_arg() {
    tray::show_balloon(t!("update.toast.updated_to", v = version));
}
```

### Why `--wait-pid` instead of cmd's `timeout /t 2`

- `timeout` is a cmd.exe builtin; using it requires cmd.exe.
- `WaitForSingleObject` on the parent process handle is the canonical Win32 idiom: zero delay if parent already exited, exact timing when it actually exits, no console involvement.
- Bonus: removes the magic "2 seconds is enough" guess.

### Path safety

The current `reject_unsafe_path` (`%` rejection) becomes unnecessary — no cmd.exe to expand `%var%`. Keep the function for defense-in-depth; revisit in code review.

## Files touched

| File | Change |
|---|---|
| `src/update/install.rs` | Replace `spawn_handoff` with `swap_and_spawn` using `MoveFileExW` + `CreateProcessW`; drop `cmd` arg-quoting code |
| `src/update/mod.rs` | Add `Error::SwapFailed` variant if needed |
| `src/app.rs::restart_app` | Replace cmd.exe spawn with `CreateProcessW` + mutex release ordering |
| `src/app.rs` | Add CLI flag parsing for `--wait-pid` and `--updated-to`; cleanup pass for `.old.*` siblings; balloon tray call on successful update boot |
| `src/main.rs` | Wire `--wait-pid` into the early-startup mutex-acquisition path (BEFORE `update::run_cli` check) |
| `src/tray/mod.rs` (or `tray/badge.rs`) | Confirm `Shell_NotifyIconW` with `NIF_INFO` balloon is supported by current tray code; add helper if missing |
| `src/i18n/*` | New string keys: `update.toast.updated_to` |

## Risks + mitigations

| Risk | Mitigation |
|---|---|
| `MoveFileExW` rename fails (NTFS permission denied, AV scanner holding handle) | Surface `Error::NotWritable` to user; do NOT proceed to step 4 (still recoverable — original exe untouched at this point) |
| New instance crashes before clearing old exe ⇒ `.old.*` accumulates | Cleanup glob `bubble.exe.old.*` on every startup is idempotent and cheap |
| Mutex race: new instance acquires before old releases | `--wait-pid` + `WaitForSingleObject(5000ms)` covers it; if it times out the new instance retries `CreateMutexW` in a 200ms loop for ~3s before giving up |
| User on FAT32 / non-NTFS volume | `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` still works on FAT32; renaming-while-running is the NTFS-specific concern but the .exe is rarely on FAT32. Document the edge case |
| `--wait-pid` arg parsed in legacy build that doesn't recognize it | Old builds will ignore unknown args (cargo CLI parser behavior — verify). If they crash, the user can manually launch. Acceptable: this is a one-way migration; once the new flag is in a release, future updates are smooth |

## Success criteria

1. Manual update from v0.1.9 → test-tagged v0.1.99 produces ZERO visible console window across 20 consecutive runs on Win11 + Win10
2. Restart triggered from menu (e.g. language change) produces ZERO visible console
3. Auto-update at scheduled interval (Hourly) produces a tray balloon "Updated to v0.1.99" on next launch
4. `.old.*` files do not accumulate after 5 update cycles
5. App still launches cleanly when no parent PID was passed (i.e. fresh user start)
6. SHA-256 verification path unchanged and still rejects tampered binaries

## Out-of-scope follow-ups

1. **Audit `usage::refresh::spawn_local` / `spawn_wsl`**: the `wsl.exe` invocation in particular has known console-flash quirks even with `CREATE_NO_WINDOW`. If the user still sees occasional flashes after this change ships, that is the next investigation target.
2. **`creds::wsl_bridge::wsl_run`**: same family of `wsl.exe` invocations.

## Unresolved questions

- None blocking implementation. (Open follow-up: whether to also pipe `--wait-pid` into the legacy `--apply-update` compatibility branch in `update::run_cli`, in case a very old build is doing the spawning.)
