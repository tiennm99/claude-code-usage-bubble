# Plan: Menu Restart Button

**Slug:** menu-restart-button
**Created:** 2026-05-18 09:45
**Branch:** main
**Status:** Implemented (awaiting commit)

## Goal

Add a "Restart" entry to the tray right-click context menu, positioned directly above "Exit". Clicking it relaunches the running binary in-place without prompting for confirmation.

## Why

User-requested. Current flow to apply a config/locale tweak that doesn't hot-reload (or to recover after a hang) is Exit → relaunch from Start menu. A one-click restart is symmetric with Exit and avoids hunting for the binary again.

## Phases

| # | Title | Status |
|---|-------|--------|
| 01 | Implement Restart action | Done — [phase-01-implement-restart-action.md](phase-01-implement-restart-action.md) |

## Key Decisions

- **Placement:** main menu, between `Show widget` separator and `Exit`. NOT inside Settings submenu — keeps top-level discoverability.
- **No confirmation dialog.** Settings auto-save on every change (settings.rs:138-152); restart is non-destructive.
- **Mechanism:** detached `cmd.exe /c timeout /t 1 >nul & start "" "<exe>"` handoff, then `PostQuitMessage(0)`. Same pattern as `update::install::begin` minus the swap step. The 1-second wait lets the current process release `Global\ClaudeCodeUsageBubble` mutex before the new instance's `CreateMutexW` runs.
- **Reject paths containing `%`** — cmd.exe expands `%var%`, same defense the update module already uses (install.rs:90-94).

## Dependencies

- None. Pure Rust + existing `windows` crate features.

## Out of Scope

- Restart after settings change auto-trigger (would be a separate feature).
- Restart-with-args (e.g., toggle `--diagnose`).
- Cross-platform — Windows-only by design.

## Unresolved Questions

None.
