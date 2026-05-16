---
phase: 2
title: "End-to-end update test"
status: pending
priority: P1
effort: "2h"
dependencies: [1]
---

# Phase 2: End-to-end update test

## Overview

Cut a real test release pair and prove the in-app updater finds it,
downloads it, swaps the running .exe, and relaunches into the new
version. The updater code already exists; this phase is about
flushing out integration bugs between CI output and the updater's
expectations (asset name casing, redirect behavior, file lock release
timing, version-comparison edge cases).

## Requirements

### Functional
- Cut `v0.1.0` from current `main` → CI uploads asset.
- Bump `Cargo.toml` to `0.1.1`, commit, push, tag `v0.1.1` → CI uploads asset.
- On a Windows test machine, run the v0.1.0 binary downloaded from the v0.1.0 release.
- From the right-click menu, "Check for updates" must transition `Idle → Checking → Available`.
- Clicking "Update available" must transition `Available → Applying` and exit the process.
- ~2 seconds later, v0.1.1 must be running (verify via right-click menu showing "Up to date" after re-check, or via file properties on the .exe).
- The `version_action` apply branch (`src/app.rs:1037-1066`) must succeed: `update::install::begin` returns `Ok(())` and the process posts `WM_QUIT` (`PostQuitMessage(0)` at `src/app.rs:1056`).

### Non-functional
- Test machine has no admin privileges → confirms `ensure_writable` (`src/update/install.rs:81-89`) works for `%LOCALAPPDATA%` install.
- Run from an install path that contains a space (e.g. `C:\Users\test user\bin\`) to validate `spawn_handoff` quoting (`src/update/install.rs:53-69`).

## Architecture

### Test matrix

| Scenario | Where exe lives | Expected |
|---|---|---|
| Vanilla user-local install | `%LOCALAPPDATA%\ClaudeCodeUsageBubble\` | Succeeds |
| Path with spaces | `C:\Users\test user\bin\` | Succeeds (cmd /c quoting) |
| Read-only install dir (e.g. `C:\Program Files\…`) | `C:\Program Files\Bubble\` | `Failed` status surfaces; no crash |
| Offline | n/a | `Failed` status, no crash, retries on next 24h timer |
| Already on latest | n/a | `UpToDate` status, no download |

### Observability

- Run with `claude-code-usage-bubble.exe --diagnose` to capture the
  log at `%TEMP%\claude-code-usage-bubble.log`. Look for `update apply
  failed:` lines (`src/app.rs:1058`).
- After update, the new process is started by `cmd.exe` (detached);
  Task Manager parent column will show no parent — that's expected.

## Related Code Files

- Reference only (no edits expected): `src/update/release.rs`, `src/update/install.rs`, `src/app.rs` (lines 1025-1135), `Cargo.toml`

## Implementation Steps

1. **Cut v0.1.0:**
   ```bash
   git -C D:/tiennm99/claude-code-usage-bubble tag -a v0.1.0 -m "v0.1.0"
   git -C D:/tiennm99/claude-code-usage-bubble push origin v0.1.0
   ```
   Wait for the Phase-1 workflow to produce `v0.1.0` release with `claude-code-usage-bubble.exe`. Download the asset locally — this is the "old" binary.

2. **Smoke-test the v0.1.0 download** on a Windows machine: run it, confirm the bubble appears, right-click → "Check for updates" returns "Up to date" (no v0.1.1 yet).

3. **Cut v0.1.1:**
   - Bump `Cargo.toml` `version = "0.1.0"` → `"0.1.1"`.
   - `cargo build --release` locally to refresh `Cargo.lock`.
   - Commit: `chore: bump version to 0.1.1`.
   - Tag: `git tag -a v0.1.1 -m "v0.1.1"`.
   - Push both: `git push origin main && git push origin v0.1.1`.

4. **Run the v0.1.0 binary** (still installed from step 1) and right-click → "Check for updates". Status should transition to "Update available". Click it. The process exits, ~2 s pass, the new v0.1.1 binary should launch automatically.

5. **Verify v0.1.1 is running:** right-click → "Check for updates" should now return "Up to date". Cross-check `claude-code-usage-bubble.exe --diagnose` log for the version line, or check file properties in Explorer.

6. **Cleanup if it goes wrong:**
   - Stuck "Applying" status with no swap → kill the detached `cmd.exe` in Task Manager, manually copy `%LOCALAPPDATA%\ClaudeCodeUsageBubble\updates\update.exe` over the running exe location.
   - `cmd /c` quoting broke → fix in `src/update/install.rs:58-60` and retag as `v0.1.2`.

7. **Run negative scenarios** (table above): path-with-spaces, read-only install dir, offline. Each must fail-soft without crashing.

## Todo List

- [ ] v0.1.0 release cut and asset downloaded
- [ ] v0.1.0 binary verified runnable on Windows
- [ ] Cargo.toml bumped to 0.1.1, committed, tagged, pushed
- [ ] v0.1.1 release produced by CI
- [ ] v0.1.0 binary self-updates to v0.1.1 successfully
- [ ] Post-update, "Check for updates" returns "Up to date"
- [ ] Negative scenario: install in path with space succeeds
- [ ] Negative scenario: read-only install dir surfaces "Failed" status, no crash
- [ ] Negative scenario: offline → "Failed", retry timer rearmed

## Success Criteria

- [ ] A v0.1.0 download → click update → v0.1.1 running with no manual file copying.
- [ ] No SmartScreen kill (it will warn on first run; that's expected and documented).
- [ ] `%TEMP%\claude-code-usage-bubble.log` contains no `update apply failed` lines after the successful run.
- [ ] All negative scenarios fail without crashing the bubble.

## Risk Assessment

| Risk | Likelihood | Mitigation |
|---|---|---|
| File lock not released in 2 s window | Low-Medium | The 2 s `timeout` in `spawn_handoff` is conservative; if it ever races, bump to 3 s |
| GitHub CDN redirect not followed by WinHTTP | Very Low | WinHTTP follows redirects by default (no `WINHTTP_OPTION_DISABLE_FEATURE` set); will surface in step 4 if broken |
| Antivirus quarantines the freshly-written staging exe | Medium | Document the workaround (allowlist the install dir); future signing fixes this |
| Test pollutes real release feed | Low | If you must test with throwaway tags, use `workflow_dispatch` (creates draft) instead of pushing the tag |

## Security Considerations

- The downloaded asset is fetched over HTTPS from a `*.githubusercontent.com` CDN; WinHTTP validates certs against the system root store.
- No checksum verification yet — accepted risk (HTTPS + cert pinning is the floor). Future enhancement: ship `SHA256SUMS.txt` and verify in `install::download`.
- The `cmd /c` command string is composed only from `current_exe()` and `stage_path()`; neither is user-controlled. No shell injection vector.
