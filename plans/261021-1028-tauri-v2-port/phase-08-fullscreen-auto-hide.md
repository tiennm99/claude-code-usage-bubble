---
phase: 8
title: "Fullscreen auto-hide"
status: pending
priority: P2
effort: "2h"
dependencies: [6]
---

# Phase 08: Fullscreen Auto-Hide

## Context

Each bubble runs a 1500 ms timer (`src/bubble.rs:970-1205`;
`evaluate_foreground_fullscreen` at `src/bubble.rs:1008`). When a foreground
window covers its monitor's full rect within 2 px, all bubbles hide, unless
that window's class is in the ignore list, it is cloaked, or it is minimized.

## Requirements

- `platform::foreground_is_fullscreen() -> (bool, &'static str reason)`:
  - **Windows:** the existing evaluation, copied verbatim, with one change: the
    `ClaudeCodeUsageBubble` class check becomes "the foreground window belongs
    to our own process" (`GetWindowThreadProcessId` compared with
    `std::process::id()`). The `Progman`, `WorkerW` and `Shell_TrayWnd` class
    checks stay. Uses `rect_covers_monitor` and the style bits from `core::geom`.
  - **Linux:** with an X11 connection (`x11rb`, connected once and cached),
    read root `_NET_ACTIVE_WINDOW`, then that window's `_NET_WM_STATE`. The
    result is true if the state contains `_NET_WM_STATE_FULLSCREEN` and the
    window is not ours (compare `_NET_WM_PID`). Without X11, return false.
  - **macOS:** stub that returns false, with a doc comment pointing to
    `CGWindowListCopyWindowInfo` for later.
- One 1500 ms thread checks the state and applies `core::visibility::next`
  (Phase 06 table) to every bubble. Fullscreen sets `hidden_by_fullscreen` and
  hides the bubbles. When it clears, the bubbles show without activation unless
  `user_hidden` is set. A user show while fullscreen is active shows at once
  (parity with `src/bubble.rs:407-427`). The thread snapshots state and drops the
  lock before any window call (D14).
- Log each decision with its reason under `--diagnose`.

## Files

- Create: `src/fullscreen.rs` (thread and state transitions)
- Modify: `src/platform/windows.rs`, `src/platform/linux.rs`, `src/platform/macos.rs`, `src/lib.rs`

## Steps

1. Move the Windows code and keep the two existing tests (in `core::geom`).
2. Add the x11rb reader.
3. Add unit tests for `core::visibility::next` covering every row of the Phase 06 table, including a user show during fullscreen and fullscreen off while user-hidden.

## Validation

- Host: `cargo test fullscreen:: visibility::` is green.
- Linux desktop (X11, manual): a fullscreen browser (F11) hides the bubbles within 1.5 s, and they come back when it exits.
- Windows (CI artifact, manual): a fullscreen game or video hides them, a maximized window does not, the desktop does not, and opening the bubble menu or the panel does not.

## Risks

| Risk | L x I | Mitigation |
|------|-------|------------|
| The panel being foreground is misread as fullscreen | L x M | The own-PID check. |
| The X11 connection fails under Xwayland | L x L | Treat it as not fullscreen. |

## Rollback

Revert the commits. Bubbles then stay visible over fullscreen apps.
