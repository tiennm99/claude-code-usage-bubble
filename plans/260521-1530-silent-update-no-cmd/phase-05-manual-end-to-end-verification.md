---
phase: 5
title: "Manual end-to-end verification"
status: pending
priority: P2
effort: "1h"
dependencies: [1, 2, 3, 4]
---

# Phase 5: Manual end-to-end verification

## Overview

Functional sign-off across all changed code paths. No unit tests added (the changed surface is Win32-heavy and effectively integration territory); instead, a documented manual checklist that the maintainer runs once before committing.

## Requirements

**Functional**
- All success criteria from phases 1-4 verified on a real Windows machine (Win11 preferred, Win10 as secondary if available).
- Build artifact runs without errors when launched plainly (no flags).
- No regression in existing update / restart / refresh paths.

**Non-functional**
- Verification log saved as a section in `phase-05` after run, with date + outcome per item.

## Test Matrix

### Group A: Restart path (phase 2)

| # | Step | Expected |
|---|---|---|
| A1 | Launch binary, open right-click menu, click "Restart" | No console flash; new instance appears within 1.5s |
| A2 | Repeat A1 twenty times back-to-back | Zero flashes observed; settings.json mtime updates each time |
| A3 | Launch with `--diagnose`, click Restart, inspect `%TEMP%\claude-code-usage-bubble.log` | Shows "restart: spawned detached child, posting quit" and new instance shows mutex acquired (within 200ms of the wait completing) |

### Group B: Update install path (phase 3)

Setup: tag a test release `v0.1.99` via the existing GitHub Actions workflow (per `plans/260516-1730-github-release-auto-update`). Bump local `Cargo.toml` back to `v0.1.0` before running. Build the local v0.1.0 with this plan's changes.

| # | Step | Expected |
|---|---|---|
| B1 | Launch v0.1.0 build, set update channel to "Hourly", manually trigger "Check for updates" | "Update available" shown; click "Apply" |
| B2 | During B1 apply | No console flash; new v0.1.99 instance appears |
| B3 | After B1/B2 | Tray balloon "Update applied — Updated to v0.1.99" appears (blue info icon) |
| B4 | Inspect install dir after B1/B2 | NO `.old.*` files remain (cleanup removed them) |
| B5 | Repeat B1 four more times (after re-tagging v0.1.100 etc.) | Zero flashes across 5 update cycles |
| B6 | Tamper test: edit downloaded asset on disk before swap (would require pausing between download and swap — gate via `--diagnose` log timestamps) | SHA-256 mismatch raises `Error::ChecksumMismatch`; swap is NOT performed; original binary still runs |
| B7 | Rollback test: make staging path read-only or simulate step-10 failure | Backup restored; original binary still runs after a manual restart |

### Group C: Cleanup + notification (phase 4)

| # | Step | Expected |
|---|---|---|
| C1 | Manually drop `claude-code-usage-bubble.exe.old.9999` next to the running binary; launch app | Stale file is removed within 1s of launch |
| C2 | Launch app with `--updated-to 9.9.9` arg from a terminal | Tray balloon shows title + "Updated to v9.9.9" in current UI language |
| C3 | Repeat C2 with each supported locale | Each locale shows correct translation |
| C4 | Launch normally (no `--updated-to`) | No balloon appears |

### Group D: Regression smoke

| # | Step | Expected |
|---|---|---|
| D1 | Launch binary fresh (cold start, no flags) | Bubble appears in <2s; usage refresh fires once; tray icon registers |
| D2 | Open settings (right-click → Language → switch), confirm restart happens | Restart works without flash (this is the same `restart_app` path) |
| D3 | Disable auto-update ("Disabled"), wait 30s, re-enable Hourly | No state corruption; check timer resets |
| D4 | Run with `--diagnose --apply-update <some-path> <pid>` (legacy compat) | Returns exit code 0 cleanly (per `update::install::run_cli`) — unchanged |

## Implementation Steps

1. Build `cargo build --release`.
2. Copy `target/release/claude-code-usage-bubble.exe` to `%LOCALAPPDATA%\ClaudeCodeUsageBubble\` (fresh test directory).
3. Run through Test Matrix A → B → C → D in order.
4. For each test row, record outcome (PASS/FAIL + notes) in the Verification Log section below.
5. If any FAIL: open an issue describing the failure, do NOT mark phase complete.
6. If all PASS: mark phase complete, commit changes following project commit conventions (no `chore:` or `docs:` per CLAUDE.md).

## Success Criteria

- [ ] All Group A tests PASS.
- [ ] All Group B tests PASS (B6 + B7 may be skipped if test scaffolding too costly; document as such).
- [ ] All Group C tests PASS.
- [ ] All Group D tests PASS.
- [ ] Verification Log filled in with date + outcomes.
- [ ] No console flash observed across the entire test session.

## Risk Assessment

| Risk | Mitigation |
|---|---|
| Manual testing skips Group B because tagging real releases is annoying | Alternative: build two local copies (v0.1.0 + v0.1.99), set up a local HTTP server with a fake GitHub-Releases-shaped JSON, point the app at it via a debug flag. Out of scope for this plan but noted |
| "Flash" is subjective at 60Hz | Record screen with OBS at 60fps for one test run, scrub frame-by-frame to confirm zero console window appearance |
| Win10-only flash regression (only test on Win11) | Document Win10 testing as "best effort"; primary target is Win11. Note Win10 result in Verification Log |

## Verification Log

<!-- Fill in after running the test matrix -->

### Session 1 — TBD
- Group A: TBD
- Group B: TBD
- Group C: TBD
- Group D: TBD
- Flash observed: TBD
- Notes: TBD
