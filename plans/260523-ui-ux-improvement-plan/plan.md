# UI/UX Improvement Plan

## Context
- Product: native Windows floating usage bubble for Claude Code/Codex.
- Current UI stack: Win32 popup/layered windows, tiny-skia drawing, GDI text, Shell tray icons.
- Current baseline: `cargo check` passes on 2026-05-23.
- Primary files: `src/bubble.rs`, `src/panel.rs`, `src/app.rs`, `src/tray/*`, `src/usage_color.rs`, `src/i18n/locales/*.toml`.

## Phase 1 - Accessibility And Status Clarity
- Status: Partially Complete
- Priority: High
- Files: `src/usage_color.rs`, `src/bubble.rs`, `src/panel.rs`, `src/tray/mod.rs`, locale TOMLs.
- Improve non-color status cues for normal/warning/critical/auth/error states.
- Add richer tray tooltips: model, 5h percent/countdown, 7d percent/countdown, current state.
- Add localized strings for warning/critical labels and unavailable/auth states.
- Keep usage colors centralized in `usage_color.rs`; avoid per-surface color drift.
- Validation: contrast check for light/dark colors, manual tray tooltip check, `cargo check`.
- Completed 2026-05-23: richer tray tooltip now includes model, 5h, 7d, and left-click hint.
- Completed 2026-05-23: tray tooltip uses shorter localized tray hint text to reduce truncation risk.

## Phase 2 - Discoverability And Native Controls
- Status: Partially Complete
- Priority: High
- Files: `src/app.rs`, `src/bubble.rs`, locale TOMLs.
- Add menu items for common hidden actions: resize smaller/larger, reset size, show details.
- Add a short localized "Help" or "Controls" submenu listing drag, click, right-click, Ctrl+wheel.
- Make resize available through menu commands, not only Ctrl+MouseWheel.
- Review context-menu grouping so status/update/model/settings actions scan as separate groups.
- Validation: keyboard-access menu traversal, menu command behavior, persisted settings.
- Completed 2026-05-23: added localized Controls submenu with resize actions and disabled help rows.
- Completed 2026-05-23: disabled resize commands when they would no-op and unified menu/wheel resize through shared bubble size.

## Phase 3 - Bubble Legibility And Interaction Robustness
- Status: Planned
- Priority: Medium
- Files: `src/bubble.rs`, optional extracted bubble modules.
- Improve layout for smallest sizes: reserve stable text bounds, handle `100%`, placeholder, `!`, and long countdowns.
- Consider minimum size/shape copy update because code uses 140-360 logical width while README mentions 32-128 pixels.
- Add drag threshold and click behavior review around `WM_EXITSIZEMOVE` to reduce accidental panel opens.
- Add optional pulse reduction path if Windows animation/reduced-motion preference is available.
- Validation: manual checks at min/default/max size, 100/125/150/200% DPI, both models enabled.

## Phase 4 - Expanded Panel Redesign
- Status: Planned
- Priority: Medium
- Files: `src/panel.rs`, locale TOMLs, maybe `src/app.rs`.
- Replace fixed 280x120 assumptions with measured or wider adaptive layout.
- Make rows self-explanatory: model header, 5h and 7d labels, percent plus reset countdown.
- Add explicit error/auth/loading state rendering instead of only symbols/placeholders.
- Improve panel placement near screen edges and multi-monitor boundaries.
- Consider extracting panel layout/painting into smaller modules before behavior changes.
- Validation: all locales, long countdown text, light/dark theme, focus-loss close behavior.

## Phase 5 - Tray And Notification Polish
- Status: Planned
- Priority: Medium
- Files: `src/tray/mod.rs`, `src/tray/badge.rs`, `src/app.rs`.
- Make tray icon state readable without exact color distinction: tooltip carries exact data, icon bands remain coarse.
- Review notification throttling and text for threshold crossings.
- Ensure tray left-click/right-click behavior matches Windows notification-area conventions.
- Add manual test matrix for one-provider and two-provider modes.
- Validation: tray add/modify/delete, balloon messages, no stale icons after exit/restart.

## Phase 6 - Structure And Verification
- Status: Planned
- Priority: Medium
- Files: `src/bubble.rs`, `src/panel.rs`, `src/app.rs`, docs if behavior changes.
- Split only where it reduces real risk: bubble layout/rendering/interaction and panel layout/rendering first.
- Keep public behavior stable while extracting.
- Add unit tests for pure functions where practical: color bands, size clamps, layout math, countdown formatting.
- Run `cargo check`; run `cargo test` if tests are added.
- Update README/docs after behavior changes, especially controls and size range.
- Completed 2026-05-23: added locale schema tests covering embedded locale parsing and Controls/tray strings.

## Success Criteria
- Bubble and panel remain readable at min/default/max sizes and common DPI scales.
- Warning/critical/auth/error states are understandable without relying only on color.
- Hidden interactions have menu alternatives or discoverable help text.
- Panel handles all existing locales without clipping core data.
- Tray tooltip and notifications communicate exact state.
- Source still compiles; new pure behavior has focused tests where feasible.

## Risks
- Native Win32 UI changes require manual Windows runtime verification; screenshots/tests are limited.
- `src/bubble.rs` and `src/app.rs` are large and coupled; extract before broad changes when touching multiple concerns.
- Adaptive text/layout can regress small-size readability if not verified at 140 logical width.

## Unresolved Questions
- Should the bubble stay stadium-shaped, or should compact circular mode return as an option?
- Should menu help be always present, or only shown on first run/first right-click?
- Should reduced-motion preference disable only pulse, or all nonessential animation?
