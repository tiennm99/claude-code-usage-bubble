---
phase: 8
title: "Fullscreen auto-hide"
status: in-progress
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
    `UsageMonitor` class check becomes "the foreground window belongs
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

## Todo

- [x] `platform::foreground_is_fullscreen()` dispatcher in `src/platform/mod.rs`; native Wayland (no X11 positioning) reports false
- [x] Windows: the v0.5.0 evaluation moved into `src/platform/windows.rs`, with the bubble-class check replaced by the own-PID check (`GetWindowThreadProcessId`); `Progman`, `WorkerW` and `Shell_TrayWnd` still ignored; `rect_covers_monitor` and the style bits come from `core::geom`, whose two tests stay there
- [x] Linux: x11rb reader on the shared cached connection (`_NET_ACTIVE_WINDOW`, `_NET_WM_STATE`, `_NET_WM_PID`), pure `evaluate_active_window` unit-tested
- [x] macOS stub returning false, pointing at `CGWindowListCopyWindowInfo`
- [x] `src/fullscreen.rs`: 1500 ms thread, snapshots the visibility state, then `Runtime::apply_visibility` outside the lock; each hide/show decision logged with reason and detail under `--diagnose`
- [x] `core::visibility::next` tests cover every row of the Phase 06 table (user show during fullscreen, fullscreen off while user-hidden); `fullscreen::` tests cover check sequences through it
- [x] Host: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (197 passed, 1 ignored), `cargo test -- fullscreen:: visibility::` (9 passed), `npm run lint`, `npm test` (37 passed), `npm run tauri build -- --no-bundle`; CI green on `windows-latest`, `ubuntu-22.04` and `macos-latest`
- [ ] Linux desktop (X11) and Windows manual checks below

## Implementation notes

- `foreground_is_fullscreen()` returns a `FullscreenCheck { fullscreen, reason, detail }` instead of a `(bool, &'static str)` pair, so the diagnostic log keeps the window handle, class, bounds and monitor that v0.5.0 logged.
- Linux also treats a `_NET_WM_STATE_HIDDEN` (minimized) active window as not fullscreen, the X11 counterpart of the Windows minimized check. A window without `_NET_WM_PID` is not ours. A failed query (the window vanished between reads) counts as not fullscreen.
- Decisions are logged only when they change the bubbles, as v0.5.0 did; logging every 1.5 s tick would flood the log.
- A user show during fullscreen keeps `hidden_by_fullscreen`, so the next tick does not hide the bubbles again; leaving fullscreen then clears the flag.
- The thread starts before the bubbles are created; a bubble created while fullscreen is active stays hidden through `Visibility::shows`.
- The listed `cargo test fullscreen:: visibility::` is not valid cargo syntax (one filter before `--`); the equivalent is `cargo test -- fullscreen:: visibility::`.

## Validation

- Host: `cargo test -- fullscreen:: visibility::` is green.
- Linux desktop (X11, manual): a fullscreen browser (F11) hides the bubbles within 1.5 s, and they come back when it exits.
- Windows (CI artifact, manual): a fullscreen game or video hides them, a maximized window does not, the desktop does not, and opening the bubble menu or the panel does not.

## Risks

| Risk | L x I | Mitigation |
|------|-------|------------|
| The panel being foreground is misread as fullscreen | L x M | The own-PID check. |
| The X11 connection fails under Xwayland | L x L | Treat it as not fullscreen. |

## Rollback

Revert the commits. Bubbles then stay visible over fullscreen apps.
