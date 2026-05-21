---
title: "Silent in-app update + restart (no cmd.exe)"
description: "Replace cmd.exe handoff in update install + app restart paths with native Win32 (MoveFileExW + CreateProcessW) so no terminal window can ever flash. Add tray-balloon notification after auto-updates."
status: in_progress
priority: P2
branch: "main"
tags: ["update", "restart", "win32", "ux"]
blockedBy: []
blocks: []
created: "2026-05-21T07:43:16.332Z"
createdBy: "ck:plan"
source: skill
---

# Silent in-app update + restart (no cmd.exe)

## Overview

Two paths today spawn `cmd.exe /c "timeout ... & start ..."` for update install (`src/update/install.rs::begin`) and app restart (`src/app.rs::restart_app`). Combination of `CREATE_NO_WINDOW | DETACHED_PROCESS` + inner `start ""` can still flash a console window on some Windows configs. This plan replaces both with native `MoveFileExW` + `CreateProcessW` (the main exe is `windows_subsystem = "windows"`, so direct spawn never allocates a console). Also wires a tray balloon "Updated to vX.Y.Z" on first launch after an auto-update.

Brainstorm context: [`plans/reports/brainstormer-260521-1530-silent-update-no-cmd.md`](../reports/brainstormer-260521-1530-silent-update-no-cmd.md).

Supersedes the cmd.exe mechanism decision in [`260518-0945-menu-restart-button`](../260518-0945-menu-restart-button/plan.md) (that plan picked cmd.exe deliberately, modeled on `update::install`; this plan replaces both call sites with the native path).

## Phases

| Phase | Name | Status |
|-------|------|--------|
| 1 | [Foundation: CLI flags + native spawn helper + mutex retry](./phase-01-foundation-cli-flags-native-spawn-helper-mutex-retry.md) | Complete |
| 2 | [Restart path: replace restart_app with native CreateProcessW](./phase-02-restart-path-replace-restart-app-with-native-createprocessw.md) | Complete |
| 3 | [Update install: rename + move + native spawn](./phase-03-update-install-rename-move-native-spawn.md) | Complete |
| 4 | [Cleanup + tray notification](./phase-04-cleanup-tray-notification.md) | Complete |
| 5 | [Manual end-to-end verification](./phase-05-manual-end-to-end-verification.md) | Pending (user-driven) |

## Key contracts (must hold across plan)

| Contract | Source today | Invariant |
|---|---|---|
| Singleton mutex name | `app.rs` `APP_MUTEX_NAME` = `Global\ClaudeCodeUsageBubble` | New instance must wait for parent to release before acquiring |
| Main binary subsystem | `main.rs:1` `#![windows_subsystem = "windows"]` | Direct spawn allocates no console |
| Asset filename | `release.rs:7` `claude-code-usage-bubble.exe` | Unchanged |
| SHA-256 verification | `install.rs:64-73` | Unchanged — still verified before swap |
| Settings save on shutdown | `app.rs::restart_app` snap+save | Preserved in new restart helper |

## Dependencies

No cross-plan dependencies. Related (superseded mechanism): `260518-0945-menu-restart-button`.

## Out of Scope

- `src/usage/refresh.rs` CLI spawns (`claude.cmd`, `codex.cmd`, `powershell.exe`, `wsl.exe`) — already use `CREATE_NO_WINDOW`; tracked for follow-up.
- `src/creds/wsl_bridge.rs` `wsl.exe` calls — same.
- Code signing / SmartScreen suppression — separate roadmap item.
- Cross-platform restart/update — Windows-only by design.

## Unresolved Questions

None.

## Validation Log

### Session 1 — 2026-05-21

#### Verification Results
- **Tier**: Full (5 phases)
- **Claims checked**: 8
- **Verified**: 6 | **Failed**: 2 | **Unverified**: 0
- Verified: `app.rs:155` mutex creation; `app.rs:1378-1432` restart_app bounds; `install.rs:42-48` `--apply-update` exit-clean handler; `install.rs:99-121` `spawn_handoff`; `os::to_utf16_nul` exists and is in use; `windows = 0.58` features in Cargo.toml include `Win32_System_Threading` but NOT `Win32_Storage_FileSystem` (phase 3 must add it).
- Failed: (V1) phase 4 said `tray::notify` has "one current caller" — actually 2 (`app.rs:831`, `app.rs:859`); (V2) phase 4 said i18n uses key-based template lookup with `{v}` placeholder — actually uses struct-field-based `LocaleStrings` with TOML, no template engine.

#### Decisions

1. **i18n approach: add 3 fields to `LocaleStrings` + translate 8 locale TOML files; use Rust `format!` at call site for version substitution.**
   Reason: existing pattern is struct-field-based via `include_str!` of `src/i18n/locales/{en,nl,es,fr,de,ja,ko,zh-TW}.toml`. No template substitution at the loader level.
   → Propagated to `phase-04-cleanup-tray-notification.md`: Requirements, Related Code Files, Implementation Steps, Success Criteria, Risk Assessment all updated.

2. **Rollback failure escalation: Windows `MessageBoxW` (MB_OK | MB_ICONERROR) when MoveFileExW step 10 AND best-effort revert both fail.**
   Reason: silent failure here would orphan the user — they'd have no exe at install path. A clear modal tells them where the backup is (`bubble.exe.old.<pid>`).
   → Propagated to `phase-03-update-install-rename-move-native-spawn.md`: rollback table extended; imports list updated; new helper `surface_rollback_failure` added; new `LocaleStrings` field `update_rollback_failed_body` added to phase 4's i18n work.

3. **Stuck-parent fallback: accept 8s total budget (5s `WaitForSingleObject` + 3s mutex retry), then exit cleanly. No `TerminateProcess`.**
   Reason: forcing process termination would defeat the `settings::save` defensive flush guarantee. The 8s ceiling is generous for normal Windows scheduling; if a real hang persists, user can manually kill old process via Task Manager — acceptable failure mode.
   → No phase file change needed. Phase 1 budget numbers (5s + 3s) already match.

4. **Auto-update timing: apply when check fires, no idle-window deferral.**
   Reason: matches user's brainstorm-phase choice ("Fully silent auto-update"). Idle detection adds complexity (state tracking, defer-budget, defer-never-applies edge case) for marginal UX gain — bubble flicker during ~1s restart is acceptable.
   → No phase file change needed.

#### Whole-Plan Consistency Sweep
- Files reread: `plan.md`, `phase-01-…md`, `phase-02-…md`, `phase-03-…md`, `phase-04-…md`, `phase-05-…md`
- Decision deltas checked: 4
- Reconciled stale references: 2
  - Phase 4 i18n section rewritten from key-based template to struct-field + Rust `format!`
  - Phase 4 `tray::notify` caller count corrected from "1" to "2" with explicit file:line citations
- Cross-phase touchpoints verified: Phase 3 adds `update_rollback_failed_body` to `LocaleStrings`, Phase 4 owns the full i18n change (3 fields + 8 TOMLs) — both phases reference the same struct, consistent
- Unresolved contradictions: 0
