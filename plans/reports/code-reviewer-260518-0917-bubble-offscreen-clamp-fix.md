# Code Review: Bubble Off-Screen Clamp Fix

**Scope:** Proposed bug-fix for v0.1.7 "widget enabled but not shown" — saved positions on disconnected monitor.
**Files:** `src/bubble.rs` (create, clamp_into_work_area, set_user_visible, default_position), `src/app.rs` (spawn_bubble, toggle_widget_visibility, reset_positions).

## Overall Assessment

**Fix is correct and minimal. Ship it with two small refinements.** Root-cause matches code (verified: `bubble.rs:143-160` passes saved `position` straight into `CreateWindowExW`; `clamp_into_work_area` at `:770` only wired into `WM_SETTINGCHANGE` at `:486`). Approach is the right shape: clamp post-create, pre-show.

## Critical Issues

None.

## High Priority

1. **Call order — clamp must run BEFORE `render(hwnd)` at `bubble.rs:220`, not just before `ShowWindow` at `:222`.** `render` calls `GetWindowRect` (`:1108`) for the `UpdateLayeredWindow` destination point. If clamp runs after `render`, the first frame paints at the off-screen coords; second paint only happens on next update_data tick. Move `clamp_into_work_area(hwnd)` to between line `:218` (state insert) and `:220` (render).

## Medium Priority

2. **`MonitorFromWindow` on a not-yet-shown off-screen window — verified safe.** Win32 sets the window rect immediately at `CreateWindowExW` return (visibility is irrelevant to `GetWindowRect`). With `MONITOR_DEFAULTTONEAREST` and a window whose entire rect lies on a disconnected monitor, the OS computes intersection with each *currently attached* monitor's rect; none intersect → falls back to nearest by Euclidean distance → returns the primary on a single-monitor setup. Saved `[2407,1282]` on a 1920-wide primary → nearest = primary → clamp pulls to `(1920-w, …)`. Correct.

3. **Multi-monitor edge case is preserved.** If the saved position is on a still-connected secondary, `MonitorFromWindow` returns that secondary monitor and clamps within its work area — no unwanted pull to primary. Good.

4. **Partial off-screen.** `clamp_into_work_area` only adjusts when fully outside (clamps each axis independently to `[wa.left, wa.right-w]`). A window whose top-left is on-screen but bottom-right spills off → it pulls the whole window inside. Behaviour is fine; matches `snap_to_edge` (`:644-645`).

5. **DPI mismatch (saved from 4K → 1080p primary):** the saved coords are physical pixels but the new bubble's `width_px/height_px` are recomputed against the *current* primary DPI (`:140-142`). Clamp uses the new size against the new monitor's work area — correct. No DPI bug.

6. **`default_position` case:** no-op (already inside work area). Safe.

## Low Priority

7. **Log levels are appropriate.** `info!` in `create` (fires once per bubble creation), `set_user_visible` (fires only on user-toggle — verified at `app.rs:1152` only called from `toggle_widget_visibility`), and `toggle_widget_visibility` (one event per click). None on the render hot path. Approved.

8. **Alternative call site (clamp in `app::spawn_bubble`):** Less attractive. `spawn_bubble` doesn't own the HWND lifecycle and would need a fresh `GetWindowRect` round-trip. Keeping the clamp inside `bubble::create` keeps the bubble module the sole owner of window geometry and means future call sites (e.g. tests, a hypothetical re-create-on-DPI-change) also benefit for free. The "bubble module stays position-agnostic" argument is weak — it already calls `default_position`, `snap_to_edge`, and `clamp_into_work_area`. Position-aware is the status quo.

## Side Effects

- No callers of `bubble::create` assert the returned HWND is at the exact requested coords. `app::spawn_bubble` (`:277`) ignores position post-create; `reset_positions` (`:1156`) destroys + recreates. Safe.
- `position(hwnd)` (`:322`) reads live `GetWindowRect`, so any subsequent `on_bubble_moved` save reflects the clamped coords — this self-heals the persisted bad value on first drag.

## Positive Observations

- Clamp helper already exists and is correct (`:770-809`).
- Fix is one line + three log statements; minimal blast radius.
- Persisted-corruption auto-heal via first interaction is a nice property.

## Recommended Actions

1. **MUST:** Place `clamp_into_work_area(hwnd)` between `lock_bubbles().insert(...)` (`:218`) and `render(hwnd)` (`:220`) — not after `render`.
2. **SHOULD:** Add an `info!` in `clamp_into_work_area` that fires only when `nx != r.left || ny != r.top` (i.e. the actual reposition path). Free diagnostic for future "bubble moved itself" reports.
3. **CONSIDER:** Persist the clamped position immediately after `create` so `settings.json` is self-healed on next launch, not only after a drag. Trade-off: writes settings on every startup; current behaviour writes only on user action. Probably YAGNI — drift gets repaired on first interaction.

## Unresolved Questions

- Should we also persist the corrected position eagerly (action 3)? Default to no per YAGNI; flag for user.
- Does Windows ever defer `CreateWindowExW` window-rect commit until `ShowWindow`? Per MSDN and verified by existing `snap_to_edge` using the same pattern in `WM_EXITSIZEMOVE`, no — rect is committed synchronously.

**Status:** DONE_WITH_CONCERNS
**Summary:** Fix is correct and small. One ordering bug: clamp must precede `render`, not just `ShowWindow`, otherwise the first paint targets the off-screen coords.
**Concerns:** Action 1 (clamp before render) is a real correctness issue — the proposal as written ("after CreateWindowExW succeeds and before ShowWindow") technically permits ordering after `render`, which would defeat the fix until the next data update.
