---
phase: 2
title: "Restart path: replace restart_app with native CreateProcessW"
status: complete
priority: P1
effort: "1h"
dependencies: [1]
---

# Phase 2: Restart path: replace restart_app with native CreateProcessW

## Overview

Replace `src/app.rs::restart_app`'s `cmd.exe /c "timeout & start ..."` handoff with a direct `spawn_detached(current_exe, ["--wait-pid", our_pid])` call. The new instance handles the parent-exit wait itself using phase 1's helper; no timer needed.

## Requirements

**Functional**
- Restart triggered from the tray menu produces zero console flash.
- New instance acquires `Global\ClaudeCodeUsageBubble` mutex successfully every time.
- Settings still flushed to disk before exit (preserve current `snap + save` defensive write).
- Path-with-`%` defense becomes unnecessary (no cmd.exe); the check is removed.

**Non-functional**
- `restart_app` function shrinks from ~40 lines to ~25 lines.

## Architecture

```
restart_app():
  1. settings::save(snap)                          (unchanged)
  2. exe = current_exe()                           (unchanged)
  3. our_pid = GetCurrentProcessId()
  4. handoff::spawn_detached(exe, [--wait-pid, our_pid])
  5. on success: PostQuitMessage(0)
  6. on failure: log error, do NOT quit
```

Mutex release is implicit on process exit — no explicit `ReleaseMutex` needed because the `_mutex` handle in `app::run` is dropped when `run()` returns after `PostQuitMessage`. `Drop` closes the handle, which releases the mutex.

## Related Code Files

- **Modify**: `src/app.rs::restart_app` (lines ~1378-1432)
- **Remove**: the `RESTART_CREATE_NO_WINDOW` / `RESTART_DETACHED_PROCESS` constants (now in handoff.rs)
- **Remove**: the `%`-rejection defense (no cmd.exe to exploit)
- **Remove**: the `replace('"', "")` quote-stripping (handoff.rs handles quoting)

## Implementation Steps

1. **Read current `restart_app`** at `src/app.rs:1378-1432` to confirm exact bounds.

2. **Rewrite `restart_app`** to:
   ```rust
   fn restart_app() {
       // Defensive settings flush (unchanged)
       let snap = lock_state().as_ref().map(|s| s.settings.clone());
       if let Some(s) = snap {
           settings::save(&s);
       }

       let exe = match std::env::current_exe() {
           Ok(p) => p,
           Err(e) => {
               log::error!("restart: current_exe failed: {e}");
               return;
           }
       };

       let pid = unsafe { GetCurrentProcessId() };
       let args = vec![
           OsString::from("--wait-pid"),
           OsString::from(pid.to_string()),
       ];

       match update::handoff::spawn_detached(&exe, &args) {
           Ok(()) => {
               log::info!("restart: spawned detached child, posting quit");
               unsafe { PostQuitMessage(0) };
           }
           Err(e) => log::error!("restart: spawn failed: {e}"),
       }
   }
   ```

3. **Drop the two `RESTART_*` const declarations** above the function — they were specific to the old cmd.exe path. Phase 1's helper has its own.

4. **Drop the `%`-rejection block** in the new `restart_app`. The brainstorm doc keeps the equivalent check in `install.rs` for defense-in-depth; here it's pure dead weight without cmd.exe.

5. **Imports**: add `use std::ffi::OsString;` and `use windows::Win32::System::Threading::GetCurrentProcessId;` if not already in scope.

6. **Compile check**: `cargo build --release`.

7. **Manual smoke test**: run the binary, click Restart in the tray menu, verify:
   - No console window flashes
   - New instance appears within ~1s
   - Old instance log shows "spawned detached child, posting quit"
   - New instance log shows mutex acquired (via Phase 1's retry path)

## Success Criteria

- [ ] `cargo build --release` clean.
- [ ] Manual restart from menu produces ZERO visible console window across 20 consecutive triggers.
- [ ] New instance window appears within 1500ms of menu click.
- [ ] Settings file (`settings.json`) shows updated mtime after restart, confirming defensive save still runs.
- [ ] `restart_app` function is ≤25 lines.

## Risk Assessment

| Risk | Mitigation |
|---|---|
| Mutex handle not yet dropped when child tries to acquire | Phase 1's `--wait-pid` + 5s `WaitForSingleObject` + 3s mutex retry covers race comfortably (8s total budget vs <1s actual parent exit) |
| `PostQuitMessage` doesn't immediately exit; window-message pump may process more events | Phase 1 mutex retry tolerates up to 3s of overlap |
| `current_exe()` returns a path that the child can't load (rare: deleted exe, fileshare disconnect) | Existing behavior preserved: log error, do not quit. User can manually retry |
