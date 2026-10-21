---
phase: 4
title: "App runtime: state, scheduler, CLI, single instance"
status: pending
priority: P1
effort: "5h"
dependencies: [3]
---

# Phase 04: App Runtime

## Context

`src/app.rs`, about 1500 lines, couples the Win32 message loop, timers and state.
Replace it with Tauri setup, managed state, one scheduler thread (D14) and the
command and event bridge. All of it lives in the lib (D1); `run()` is the entry.

## Requirements

- Pre-Builder order inside `run()`:
  1. `--diagnose` sets up diag.
  2. `cfg(windows)`: WebView2 guard. If `GetAvailableCoreWebView2BrowserVersionString` fails or returns empty, `platform::windows::message_box_error` shows "Claude Code Usage Bubble needs the Microsoft Edge WebView2 Runtime" plus the Evergreen download URL, then the process exits 0.
  3. `--wait-pid <pid>` waits until that PID is gone, polling every 100 ms, capped at 15 s (D13). `OpenProcess`/`WaitForSingleObject` on Windows, `libc::kill(pid, 0)` on Unix. Log the wait and its outcome under `--diagnose`.
  4. Apply the Linux `GDK_BACKEND=x11` rule (D12).
  5. Register `tauri-plugin-single-instance` first, with a no-op callback.
  6. Register autostart and notification in Phase 05; register the updater in Phase 09. No other plugin is registered here, so no plugin needs config that does not exist yet.
- `--apply-update` is **not** handled: no released version runs the downloaded binary that way (plan.md, Backwards compatibility). An unknown flag is ignored.
- `setup()`:
  - Load settings, drop off-screen positions against `available_monitors()`, and read the theme.
  - Build `AppState` in a `Mutex`, plus a `ui_bridge` that holds the `AppHandle`. Provide `with_state(|s| …) -> T` returning owned data; it is the only way to read state before calling Tauri (D14).
  - Run the startup cleanup: delete `<exe filename>.old.*` siblings and, under `data_local_dir()/ClaudeCodeUsageBubble/updates/`, `update.exe` and `updater-helper-*.exe` (moved from `src/update/handoff.rs:83-108` and `src/update/install.rs:323-343` into `src/handoff.rs`, with a unit test on a temp dir). `update-error.log` is kept.
  - Start the scheduler thread and trigger the first poll.
- `RunEvent::ExitRequested { code: None, api }` calls `api.prevent_exit()` (D18).
- The scheduler thread receives messages over an `mpsc` channel with `recv_timeout`:
  - `PollNow`, and `SetPollInterval`, which re-arms from now without an immediate poll.
  - `RecomputeCountdown`.
  - **Update check deadline:** at startup and on `SetUpdateInterval`, use `core::state`'s due/remaining computed from `last_update_check_unix`: check now if due, otherwise sleep the remainder (`src/app.rs:1383-1405`, `1459-1476`). Only `UpdateCheckDone` re-arms for the full interval (intentional fix #6). `SetUpdateInterval(None)` cancels the deadline.
  - `Shutdown`.
  - Keep the `AtomicBool` poll gate (`src/app.rs:442-511`).
- The poll worker runs registry poll, then `apply_results`, then credential refresh: `try_refresh` every auth-failed provider in order, 8 s each; raise the session-expired alert only for the first provider whose outcome is not `Refreshed`; no immediate re-poll (`src/app.rs:571-590`). Then `propagate()`.
- `propagate()` snapshots under the lock, drops it, then builds the payloads (`core::payload`) and sends `bubble:update` with `emit_to` to each bubble label. It calls `tray::sync` and `menu::refresh` (Phase 05 hooks) and `panel::refresh_if_visible` (Phase 07), all outside the lock. Then it re-arms the countdown.
- Commands:
  - `get_bubble_state(provider)` and `get_strings()`.
  - `log_js(level, msg)`.
  - `bubble_ready(label)`, which calls the window module's "show without activation" (Phase 06).
- Handoff and exit:
  - `handoff::relaunch(extra_args) -> io::Result<()>` spawns `current_exe` (or `$APPIMAGE`) detached with `--wait-pid <pid>` plus the extra args. On Windows it uses `creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)`.
  - Restart: save settings, `relaunch(&[])`; on `Ok` call `app.exit(0)`, on `Err` log it and keep running (`src/app.rs:1478-1512`).
  - Exit is `app.exit(0)`.
  - Bubbles: `CloseRequested` calls `prevent_close`, so Alt+F4 does nothing (D18).
- `--updated-to <v>` stores a pending "update applied" alert, which Phase 05 shows once the tray exists. It works with or without `--wait-pid` (v0.4.0 sends it alone).

## Files

- Create: `src/runtime.rs` (setup, `AppState`, `with_state`, propagate), `src/scheduler.rs`, `src/commands.rs`, `src/handoff.rs` (relaunch, startup cleanup), `src/cli.rs`
- Modify: `src/lib.rs`, `src/platform/windows.rs` (`wait_pid`, `webview2_available`, `message_box_error`), `src/platform/mod.rs` (Unix `wait_pid`)
- Delete: `src/app.rs`, `src/update/handoff.rs` (its logic moves to `src/handoff.rs`)

## Steps

1. Write `cli.rs` as a pure argv parser, with tests for `--diagnose`, `--wait-pid`, `--updated-to` alone and combined, and an ignored unknown flag.
2. Build `AppState`, the scheduler and propagate. Use a fake emitter trait so the scheduler's deadline logic is unit-testable without a window, including "Weekly to Daily with the last check 2 days ago checks at once".
3. Wire the commands. Leave bubble creation to Phase 06; the test window from Phase 01 renders the payload as raw JSON for now.

## Validation

- Host: `cargo test cli:: scheduler:: handoff::` is green.
- Linux desktop (X11, manual; this host has no display): `npm run tauri dev -- -- --diagnose` logs a successful poll (or `NoCredentials`), and the test window shows the payload JSON. A second launch exits at once.
- Linux desktop: the Restart command relaunches the app, and the log shows the child waited for the old PID.
- Linux desktop: closing the test window does not end the process; Exit does.
- Windows (CI artifact): starts, polls, and Restart relaunches.

## Risks

| Risk | L x I | Mitigation |
|------|-------|------------|
| Single instance kills the relaunched child while the parent is still exiting | M x M | The wait-until-gone loop (15 s cap) runs before the plugin registers. |
| Deadlock between a worker holding the state lock and a main-thread handler | M x H | D14 rule, enforced by `with_state`; code review checks no Tauri call sits inside a lock scope. |
| Window operations called off the main thread | M x M | Every window call from a worker goes through `run_on_main_thread`. |
| The Linux tray or D-Bus is missing on a headless host | M x L | The app must not exit when tray creation fails (handled in Phase 05). |

## Rollback

Revert the commits. Nothing outside `src/` changes.
