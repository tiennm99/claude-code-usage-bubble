---
phase: 2
title: "Extract portable core with tests"
status: completed
priority: P1
effort: "6h"
dependencies: [1]
---

# Phase 02: Extract Portable Core With Tests

## Context

Most of the logic is already pure but sits inside Win32 files. Extract it into
`src/core/`. The code is plain Rust with `Rect`/`Point` structs instead of
`RECT`/`HWND`, so it can be unit-tested on Linux. There are only 6 tests today
(`settings.rs` 2, `bubble.rs` 2 (`mod fullscreen_tests` at `src/bubble.rs:1208`),
`i18n` 1, `update/install.rs` 1).

## Requirements

Move or port each item below, keeping its exact behaviour and constants.

- `core/geom.rs`: `Rect{l,t,r,b}`, `Point`, and `scale(px, dpi)` (`src/os/dpi.rs:29-31`).
  - Bubble sizing: `aspect_at_width` (`src/bubble.rs:68`) and `bubble_height_logical`, with `MIN 140 / MAX 360 / DEFAULT 200 / STEP 20` (`src/bubble.rs:50-53`).
  - `default_position(work_area, w, h, provider)`, with the raw 24 px gap and the `h+24` stagger (`src/bubble.rs:2012-2041`).
  - `clamp_into_work_area`, including the Codex bottom-right `h+24` lift (`src/bubble.rs:891-948`).
  - The snap functions over a `SnapInput{rect, work_area, dpi, taskbar: Option<(Edge, Rect)>, peers: Vec<Rect>}`: corner, then edge, then taskbar, then peer, then the final clamp (`src/bubble.rs:709-889`, `950-968`).
  - `point_in_rounded_rect` (`src/bubble.rs:637`), even though D4 does not use it for hit-testing. Phase 06 uses it to decide whether a drag started inside the pill.
  - `place_near(bubble, panel_size, bounds)` (`src/panel.rs:515-542`), and `position_on_any_monitor(x, y, monitors)` with a 140 px probe, replacing `src/settings.rs:114`.
  - `rect_covers_monitor`, and `window_style_bits_allow_fullscreen` as plain u32 bit logic.
- `core/settings.rs`: move `src/settings.rs`. Split `load()` into `parse_and_normalise(&str)` and `drop_offscreen_positions(&mut, &[Rect])`; Phase 04 calls the second one after `available_monitors()`. Keep the path (`APP_DIR_NAME` `src/settings.rs:15`), the serde attributes, the atomic save and the toggle invariant.
- `core/i18n.rs`: move `src/i18n/mod.rs`. Add `start_at_login`, "Start at login", and select it with `cfg(windows)`. Keep `format_window`, `format_countdown` and `time_until_display_change`.
- `core/color.rs` and `core/usage_color.rs`: `Rgb` without `into_colorref`; `accent_color_for`, `bar_fill_color`, `brighten` and `pulse_triangle`; `Rgb::to_hex()`.
- `core/state.rs`: `apply_results` with the 80/95 crossings, the `!` and `…` texts, `last_poll_ok` and the auth-failure list (`src/app.rs:513-569`), plus the 30-minute alert cooldown (`src/app.rs:56`, `841-898`). It returns a list of alerts instead of calling the tray. Also `tray_tooltip` (`src/app.rs:777-790`), `UpdateStatus` and `version_action_label` (`src/app.rs:1176-1192`), and the update-check due/remaining math from `last_update_check_unix` (`src/app.rs:1383-1405`): due when never checked or `now - last >= interval`, otherwise the remainder.
- `core/payload.rs`: pure builders for `BubblePayload` and `PanelPayload` (plan.md, Data flow), from a provider snapshot plus `is_dark`, `canvas_w`, `canvas_h` and `dpi`. Add `format_pct(Option<f64>) -> String` (`'{:.0}%'` or `'—'`) to `core/i18n.rs`. Before the first poll the head label and weekly countdown are `'…'` (`src/app.rs:329-345`); on an error the bubble keeps the countdowns of the stored windows and never carries `'!'`.
- `core/menu_model.rs`: a plain enum tree for the context menu (`src/app.rs:923-1193`) with string ids: `refresh`, `freq:<ms>`, `provider:<slug>`, `autostart`, `reset-position`, `version`, `update-interval:<secs|off>`, `size-smaller`, `size-larger`, `size-reset`, `toggle-widget`, `restart`, `exit`. It also carries the check and enabled flags and the greyed hint rows. Phase 05 maps it onto `tauri::menu`.

## Files

- Create: `src/core/mod.rs`, `src/core/geom.rs`, `src/core/settings.rs`, `src/core/i18n.rs`, `src/core/color.rs`, `src/core/usage_color.rs`, `src/core/state.rs`, `src/core/payload.rs`, `src/core/menu_model.rs`
- Modify: `src/lib.rs` (`pub mod core;`). Also re-add `pub mod usage;`, but only for `usage/types.rs`, through a temporary `usage/mod.rs` reduced to `pub mod types;`. Phase 03 restores the rest. The `Serialize` derive on `ProviderId` lands here.
- Delete: `src/settings.rs`, `src/i18n/mod.rs`, `src/usage_color.rs`, `src/os/color.rs`, `src/os/dpi.rs`

## Steps

1. Copy each function verbatim, then swap the Win32 types for `geom` types.
2. Port the existing tests, then add the tests named in the subsystem maps:
   - Aspect ratio and height at 140, 200, 220, 280, 300 and 360.
   - Corner snap tie order, the edge-zone boundaries, all four taskbar edges, peer alignment and the final clamp.
   - The Codex stagger lift. `place_near` flipping above, clamping left and right, and with a negative virtual origin.
   - Fill-colour band edges at 59.9, 60, 79.9, 80, 94.9 and 95.
   - Countdown buckets, including "now" and 1d23h showing "1d", and `time_until_display_change`.
   - Half-to-even rounding: 72.5 gives "72" and 73.5 gives "74".
   - Thresholds: 79→80 fires; 80→85 does not; 79→96 queues both, but the cooldown lets only one through.
   - The update schedule's due and remaining values. Settings JSON for the legacy two-provider and three-provider files and for a corrupt file. Probe validation against fake monitors.
   - The menu model's greyed sizes at 140, 360 and 200.
   - Payloads: `sessionPctText` `'—'` for `None`, `weeklyPctText` `null` for `None`, `'…'` placeholders before the first poll, stale countdowns (not `'!'`) in the bubble after an auth failure, and `'!'`/`'…'` in the panel texts.

## Todo

- [x] `core/geom.rs`: `Rect`, `Point`, `scale`, bubble sizing, `default_position`, `clamp_into_work_area`, `snap` over `SnapInput`, `point_in_rounded_rect`, `place_near`, `position_on_any_monitor`, `rect_covers_monitor`, `window_style_bits_allow_fullscreen`
- [x] `core/settings.rs`: `parse_and_normalise`, `drop_offscreen_positions`, same path, serde attributes, atomic save and toggle invariant
- [x] `core/i18n.rs`: `start_at_login` selected by `cfg(windows)`, `format_pct`, `format_window`, `format_countdown`, `time_until_display_change`
- [x] `core/color.rs` and `core/usage_color.rs`: `Rgb::to_hex`, `accent_color_for`, `bar_fill_color`, `brighten`, `pulse_triangle`
- [x] `core/state.rs`: `apply_results`, 30-minute cooldown, `tray_tooltip`, `UpdateStatus`, `version_action_label`, update-check due/remaining
- [x] `core/payload.rs`: `BubblePayload` and `PanelPayload` builders
- [x] `core/menu_model.rs`: menu tree with string ids, check/enabled flags and hint rows
- [x] `src/lib.rs` declares `core` and `usage`; `usage/mod.rs` reduced to `types`; `ProviderId` derives `Serialize`
- [x] Deleted `src/settings.rs`, `src/i18n/mod.rs`, `src/usage_color.rs`, `src/os/color.rs`, `src/os/dpi.rs`
- [x] Host: `cargo test core::` (65 tests), `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `npm run lint`, `npm test`, `npm run tauri build -- --no-bundle`

## Implementation notes

- `apply_results` takes `core::state::PollOutcome` (`Ok`, `AuthFailed`,
  `Failed`) instead of `usage::Error`, because the error type needs `net` and
  `creds`, which return in Phase 03. The caller maps `AuthRequired` and
  `TokenExpired` to `AuthFailed`, everything else to `Failed`, and logs the
  transient error as before.
- The cooldown lives in `UsageState::admit_alert`. Threshold alerts pass it
  inside `apply_results`; `session_expired_alert` uses the same gate; the
  update-applied alert does not (parity).
- Time-dependent helpers have `_at(…, now)` variants so the countdown and
  payload tests are deterministic. `bubble_payload` takes `now` as well as
  the `Canvas { w, h, dpi }`.
- `version_action_label` drops the old WinGet suffix: no WinGet package
  exists and Phase 09 replaces the channel enum.
- `drop_offscreen_positions` keeps every position when the monitor list is
  empty, so a failed `available_monitors()` cannot wipe them. The probe
  needs real overlap with a monitor rect, like `MonitorFromRect` with
  `MONITOR_DEFAULTTONULL`.
- `Settings::save` now returns `io::Result` so the caller can log a failure;
  the tmp-and-rename write and its direct-write fallback are unchanged.
- `Settings::toggle_live_provider` holds the toggle invariant from
  `toggle_model`: providers without live usage never change, and the last
  enabled live provider is re-enabled.
- `MenuCommand::parse` accepts only the poll and update intervals the menu
  offers, so a malformed id cannot set an arbitrary interval.
- The old `pulse_triangle` doc comment had the peaks wrong; the code is
  unchanged and the comment now says 0 at phase 0 and 1 at phase 12.
- `src/os/mod.rs` still names the deleted `color` and `dpi` files. It is
  outside the module tree, and the phase that deletes `src/os/` removes it.

## Validation

- Host: `cargo test core::`, with all tests green and at least 40 of them.
- Host: `cargo clippy --all-targets -- -D warnings`.

## Risks

| Risk | L x I | Mitigation |
|------|-------|------------|
| Behaviour drifts while refactoring the snap code off `HWND` | M x H | Port line by line and add tests before deleting the source. The originals stay in git history for diffing. |
| Peer order depended on `HashMap` iteration | L x L | Take peers in `LIVE_USAGE` order. The order is deterministic now, and only a tie can differ from today. |

## Rollback

Revert the commits. `core` has no runtime consumers yet; because it is `pub` in the lib (D1), unused items do not trip `dead_code`.
