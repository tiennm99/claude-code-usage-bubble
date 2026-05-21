---
phase: 1
title: "Foundation: CLI flags + native spawn helper + mutex retry"
status: complete
priority: P1
effort: "3h"
dependencies: []
---

# Phase 1: Foundation: CLI flags + native spawn helper + mutex retry

## Overview

Build the primitives that phases 2-4 reuse: a CLI argument parser for `--wait-pid <pid>` and `--updated-to <version>`, a `spawn_detached_self` helper that calls `CreateProcessW` directly (no cmd.exe), and mutex-acquisition retry logic that activates only when `--wait-pid` was passed.

## Requirements

**Functional**
- Parse `--wait-pid <u32>` and `--updated-to <version-string>` from `std::env::args` without breaking existing flags (`--diagnose`, `--apply-update`).
- Expose `spawn_detached(exe: &Path, args: &[OsString]) -> io::Result<()>` that uses `CreateProcessW` with `CREATE_NO_WINDOW | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`.
- Expose `wait_for_parent_exit(pid: u32, timeout_ms: u32)` that uses `OpenProcess(SYNCHRONIZE)` + `WaitForSingleObject`. Returns silently on timeout (best-effort).
- Mutex acquisition in `app::run` retries `CreateMutexW` for ~3 seconds (200ms backoff) ONLY when `--wait-pid` was present; preserves today's immediate-fail behavior for normal startup.

**Non-functional**
- No new external dependencies. All Win32 calls via the existing `windows = "0.58"` crate features.
- Helper module ≤ ~120 lines total.

## Architecture

```
src/
├── update/
│   └── handoff.rs   ← NEW: spawn_detached, wait_for_parent_exit, cleanup_stale_old_exes
├── main.rs          ← parse --wait-pid early, call wait_for_parent_exit BEFORE app::run
└── app.rs           ← run() reads a static "wait_pid_was_passed" flag, retries mutex if set
```

Rationale for `src/update/handoff.rs`: keeps low-level Win32 process/file ops alongside the update module that uses them most. `app.rs` and `main.rs` import it for restart + post-update bootstrap.

## Related Code Files

- **Create**: `src/update/handoff.rs`
- **Modify**: `src/main.rs` (early arg parse + wait + flag handoff to app)
- **Modify**: `src/update/mod.rs` (declare `pub mod handoff`)
- **Modify**: `src/app.rs` (mutex retry loop, gated on flag from main)
- **Modify**: `Cargo.toml` if a new `windows` feature is needed (likely `Win32_System_Threading` already covers `OpenProcess`/`WaitForSingleObject`/`CreateProcessW`)

## Implementation Steps

1. **Audit `windows` crate features.** Confirm `Win32_System_Threading` is in `Cargo.toml` (it is — line 31). Verify `CreateProcessW`, `STARTUPINFOW`, `PROCESS_INFORMATION` are accessible. Add `Win32_Storage_FileSystem` if not already present (needed for phase 3's `MoveFileExW`).

2. **Create `src/update/handoff.rs`** with three pub fns:
   - `pub fn spawn_detached(exe: &Path, args: &[OsString]) -> io::Result<()>`
     - Build a wide-char command line: quoted exe path + space-joined args, NUL-terminated.
     - `STARTUPINFOW` zero-initialized, `cb` set.
     - `CreateProcessW(NULL, cmdline_wide.as_mut_ptr(), NULL, NULL, FALSE, CREATE_NO_WINDOW | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP, NULL, NULL, &si, &pi)`.
     - Close `pi.hProcess` and `pi.hThread` immediately (fire-and-forget).
   - `pub fn wait_for_parent_exit(pid: u32, timeout_ms: u32)`
     - `OpenProcess(SYNCHRONIZE, FALSE, pid)`. If it fails (parent already gone), return immediately.
     - `WaitForSingleObject(h, timeout_ms)`. Ignore return value.
     - `CloseHandle(h)`.
   - `pub fn cleanup_stale_old_exes(current_exe: &Path)` (used by phase 4; stub here, fill in phase 4)
     - Stub: returns `Ok(())`.

3. **Modify `src/update/mod.rs`** to add `pub mod handoff;`.

4. **Modify `src/main.rs`** — insert BEFORE `app::run()`:
   ```rust
   let wait_pid = args.iter()
       .position(|a| a == "--wait-pid")
       .and_then(|i| args.get(i + 1))
       .and_then(|s| s.parse::<u32>().ok());
   if let Some(pid) = wait_pid {
       update::handoff::wait_for_parent_exit(pid, 5_000);
   }
   ```
   Note: keep this AFTER `update::run_cli` so the legacy `--apply-update` path still short-circuits cleanly.

5. **Modify `src/app.rs::run`** — gate mutex retry on whether `--wait-pid` appeared. Cheapest implementation: re-parse `std::env::args` once at the top of `run()`. Then around line 156:
   ```rust
   let retry_mutex = std::env::args().any(|a| a == "--wait-pid");
   let _mutex = acquire_singleton_mutex(retry_mutex)?;  // new helper
   ```
   New helper `acquire_singleton_mutex(retry: bool)`:
   - If `!retry`: today's behavior (fail immediately on `ERROR_ALREADY_EXISTS`).
   - If `retry`: loop CreateMutexW → on `ALREADY_EXISTS`, `Sleep(200)` and retry. Budget 15 iterations = ~3 seconds. Log every retry. After budget exhausted, return error and exit cleanly.

6. **Compile check.** `cargo build --release`. Fix any feature gaps. No behavior should change yet — `--wait-pid` arg is parsed but no caller passes it yet.

## Success Criteria

- [ ] `cargo build --release` succeeds with zero warnings beyond the existing `dead_code` allow.
- [ ] Running the binary normally (no flags) behaves identically to today (mutex check is immediate, no retry).
- [ ] Running the binary with `--wait-pid <pid-of-running-instance>` against a live instance: the new process waits ≤5s for old one to exit, then acquires the mutex within ~200ms of its release. Verifiable by killing the original after 2s and watching the new one continue.
- [ ] `update::handoff::spawn_detached` smoke test: from a small one-off snippet in `main` (gated behind a never-used flag) verify CreateProcessW returns success and pid increments. Remove before commit.

## Risk Assessment

| Risk | Mitigation |
|---|---|
| Wide-char cmdline construction has off-by-one / missing NUL | Hand-test with a path containing spaces; assert `OsStringExt::encode_wide` produces expected bytes |
| `CREATE_NO_WINDOW | DETACHED_PROCESS` combination misbehaves on GUI subsystem binaries | Documented Windows behavior: for a GUI subsystem child, both flags are effectively no-ops (no console requested), but combining them is harmless. Tested by phase 5 |
| Mutex retry loop hangs forever if budget logic wrong | Hard cap = 15 iterations × 200ms = 3.0s. After that, exit. Add log line per retry so a stuck loop is visible in `--diagnose` |
| `OpenProcess(SYNCHRONIZE, ...)` returns access-denied for cross-session | Fallback: `WaitForSingleObject` simply isn't called; mutex retry compensates |
