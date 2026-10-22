---
phase: 6
title: "Bubble window and UI"
status: in-progress
priority: P1
effort: "9h"
dependencies: [5]
---

# Phase 06: Bubble Window And UI

## Context

Today the bubble is a layered `WS_POPUP` drawn by tiny-skia and GDI
(`src/bubble.rs`). It handles hit-testing, drag, snap, clamp, DPI, pulse and
time-progress in `wnd_proc`. This phase rebuilds it as one transparent webview
window per enabled live provider (D2 to D5, D18). The user approved dead
corners for the hit area (D4).

## Requirements

- **Window** (`src/bubble.rs`, rewritten):
  - Created with `WebviewWindowBuilder::new(app, "bubble-<slug>", WebviewUrl::App("bubble.html"))`.
  - Builder options: `transparent(true)`, `decorations(false)`, `shadow(false)`, `always_on_top(true)`, `skip_taskbar(true)`, `resizable(false)`, `maximizable(false)`, `minimizable(false)`, `focused(false)`, `focusable(false)`, `visible(false)`, `title("Usage Monitor")`.
  - Size is `PhysicalSize(scale(w,dpi), scale(h_logical,dpi))`, recomputed on `ScaleFactorChanged`. Each size change re-sends the payload, whose `canvasW`, `canvasH` and `dpi` drive `layout.js` (D2).
  - Position is the saved position or `default_position` from the primary monitor's `work_area()`, then clamped.
  - Honour `widget_visible` when creating the window (intentional fix #7).
  - Removal is `destroy()` only. `create` waits for the previous window's `Destroyed` event when the same label still exists (fast off/on toggle) (D18).
  - Show: on Windows through `platform::windows::show_no_activate`, elsewhere `show()`. Triggered by `bubble_ready`, or by a 1.5 s fallback timer after creation if `bubble_ready` has not arrived (D5).
- **Windows platform** (`src/platform/windows.rs`):
  - `apply_no_activate(hwnd)` ORs `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` into the extended style before the first show. `show_no_activate(hwnd)` (`SW_SHOWNOACTIVATE`) and `hide(hwnd)` (`SW_HIDE`) are the only show/hide paths for bubbles on Windows.
  - A `SetWindowSubclass` hook delivers `WM_EXITSIZEMOVE` as move end, `WM_SETTINGCHANGE` as a theme recheck plus reclamp, and `WM_DISPLAYCHANGE` as a reclamp.
  - `is_dark()` reads `SystemUsesLightTheme`; a missing value means dark.
  - `taskbar_rect()` uses `SHAppBarMessage(ABM_GETTASKBARPOS)`.
- **Linux and macOS** (D3):
  - Move end is a 250 ms debounce on `Moved`, armed only by `bubble_drag_start`. On X11, when the debounce fires, `platform::linux::button1_down()` (x11rb `QueryPointer`) re-arms it while Button1 is still held.
  - Theme defaults to dark and is rechecked on bubble creation and `ThemeChanged`. macOS uses `window.theme()`. Linux reads the portal `org.freedesktop.appearance color-scheme` itself (1 dark, 2 light) and keeps the current theme on "no preference" or an unreachable portal, because tao maps both to light.
  - There is no taskbar rect. On Wayland without Xwayland, skip snap, clamp and saving positions.
- **Move end** (Rust): disarm, run `core::geom::snap` with the peers' `outer_position`/`outer_size` in `LIVE_USAGE` order, apply `set_position(Physical)` only if the position changed, then persist `bubble_positions[slug]`.
- **Commands:**
  - `bubble_click(slug)` calls `panel::toggle` (Phase 07; stubbed until then).
  - `bubble_drag_start(slug)` arms the move-end debounce (no-op on Windows).
  - `bubble_resize_step(dir)` calls `set_bubble_size(±20)`. That resizes every bubble around its own centre, saves the size and refreshes the menu, with no snap and no position save (parity). Programmatic `set_position` never arms the debounce.
  - `bubble_ready(label)` shows the window without activation.
- **Reset position** (D18): clear the saved positions and save, then `set_position` each existing bubble to its `default_position` (no destroy and recreate). Push the cached payload and poll.
- **Visibility transitions** (pure function `core::visibility::next(state, event)`, tested in Phase 08), matching `src/bubble.rs:407-427` and the fullscreen path:

  | Event | Result |
  |-------|--------|
  | User shows | `user_hidden = false`; show immediately, whatever `hidden_by_fullscreen` is |
  | User hides | `user_hidden = true`; hide |
  | Fullscreen on | `hidden_by_fullscreen = true`; hide |
  | Fullscreen off | `hidden_by_fullscreen = false`; show only if `!user_hidden` |

- **Hit area** (D4): `pointer-events: none` outside the pill, so the transparent corners are dead zones. No window region is set.
- **Bubble palette** (`src/bubble.rs:1406-1427`, `1857-1866`), as CSS custom properties on `[data-theme=dark]` / `[data-theme=light]`:

  | Token | Dark | Light |
  |-------|------|-------|
  | `--bg` (stadium) | `#1F1F1F` | `#F3F3F3` |
  | `--track` (ring and usage bar) | `#2C2C2C` | `#E2E2E2` |
  | `--time-track` (inner ring and time bar) | `#404040` | `#E0E0E0` |
  | `--time-fill` | `#B0B0B0` | `#666666` |
  | `--text` | `#EAEAEA` | `#1F1F1F` |
  | `--muted` | `#A8A8A8` | `#5E5E5E` |

  Accent and fill colours come from the payload.
- **Pulse** (`src/bubble.rs:340-368`, `1465-1473`, `1536-1541`, `1836-1852`, `1916-1923`):
  - The ring arc pulses only when `sessionPct >= 95`. The weekly bar fill and the weekly percent text pulse only when `weeklyPct >= 95`. The head big percent never pulses.
  - An 80 ms `setInterval` runs while either value is 95 or above. It increments `phase`; `t = 1 - |(phase % 24) - 12| / 12` (exactly the Rust code, so `t` is 0 at phase 0 and 1 at phase 12). The colour is `round(c + (255 - c) * t * 0.30)` per channel.
  - When neither value is 95 or above, the interval stops and `phase` resets to 0. Every `bubble:update` re-syncs the interval.
- **UI** (`ui/bubble.html`, `ui/bubble.css`, `ui/bubble.js`, `ui/layout.js`):
  - `html, body { background: transparent; margin: 0; overflow: hidden; user-select: none }`.
  - The `.pill` has `cursor: move`.
  - `layout.js` exports pure functions with JSDoc types, all driven by `canvasW`, `canvasH` and `dpi` from the payload:
    - `headMetrics` and `tailLayout`, using the formulas from `src/bubble.rs:1249-1396` and `1450-1592` in physical px.
    - `ringDash`, where the session arc runs clockwise from 12 o'clock and the remaining arc ends at 12 o'clock.
    - `remainingFraction`.
    - `pulseT(phase)` and `brighten(hex, t)`.
  - Text comes only from the payload: `sessionPctText` in the head, `headLabel` as the head label (replaced by `'5H'` when empty or overflowing), `weeklyPctText` (skipped when `null`), `weeklyCountdown` (skipped when empty). JS never formats a number for display.
  - The font stack is `"Segoe UI", system-ui, sans-serif`, with weights 700, 600 and 400. Sizes come from `big`, `small` and `main` divided by `dpr`.
  - Timers: the pulse above, and a 60 s timer that advances the time progress while any `resets_at` is set. Recompute immediately on `visibilitychange`.
  - Events (D3):
    - `pointerdown` with button 0: `setPointerCapture`, record the point. `pointermove` requires `(e.buttons & 1) === 1`; at 3 or more physical px it invokes `bubble_drag_start`, clears the pressed state and calls `startDragging()`. `pointerup` before the threshold calls `bubble_click`. `pointercancel`, `lostpointercapture` and `blur` clear the pressed state.
    - `contextmenu` calls `preventDefault()` and `show_context_menu`.
    - A non-passive `wheel` handler with `ctrlKey` calls `preventDefault()` and invokes `bubble_resize_step` with `sign(-deltaY)`.
    - Listen to `bubble:update` and `theme`. Call `get_bubble_state` on load, apply it to the DOM, then invoke `bubble_ready` in the same promise callback (no `requestAnimationFrame`).
- The JS test file is `tests/js/layout.test.js`.

## Files

- Create: `ui/bubble.html`, `ui/bubble.css`, `ui/bubble.js`, `ui/layout.js`, `ui/tauri-bridge.js` (thin JSDoc-typed wrappers over `window.__TAURI__`, shared with the panel), `tests/js/layout.test.js`, `src/core/visibility.rs`
- Modify: `src/bubble.rs` (full rewrite), `src/platform/windows.rs`, `src/platform/linux.rs`, `src/platform/macos.rs`, `src/platform/mod.rs`, `src/commands.rs`, `src/runtime.rs`, `src/lib.rs`
- Delete: `src/os/theme.rs`, `src/os/mod.rs`, `ui/` placeholder content from Phase 01

## Steps

1. Write `layout.js` and its tests first. Compare the outputs against hand-computed values from the Rust formulas at widths 200 and 360, at dpi 96 and 144 (including 300 px at 144 dpi, where `innerWidth * dpr` would drift).
2. Build the window module and the platform hooks.
3. Wire the HTML. Check visually against a v0.5.0 screenshot at 100% and 150% scale.

## Todo

- [x] `ui/layout.js`: `headMetrics`, `tailLayout`, `ringDash`, `remainingFraction`, `pulseT`, `brighten`, plus `usageFillWidth`, `timeFillWidth`, `headLabelText` and `cssPx`; `tests/js/layout.test.js` checks them against values from the Rust formulas at 200 and 360 logical, dpi 96 and 144 (300 px at 144 dpi included) and 140 at 120 dpi
- [x] `ui/bubble.html`, `ui/bubble.css` (palette per `[data-theme]`, dead corners), `ui/bubble.js` (layout, texts from the payload only, pulse, 60 s time progress, `visibilitychange`, pointer drag/click, `contextmenu`, Ctrl+wheel), `ui/tauri-bridge.js`
- [x] `src/bubble.rs` rewritten: builder options as listed, exact physical size and saved/default position clamped into the work area, refit on `ScaleFactorChanged`, `destroy()` only with recreate after `Destroyed`, show on `bubble_ready` or the 1.5 s fallback, `widget_visible` honoured at creation
- [x] `src/platform/windows.rs`: `apply_no_activate`, `show_no_activate`, `hide`, `install_bubble_hook` (`WM_EXITSIZEMOVE`, `WM_SETTINGCHANGE`, `WM_DISPLAYCHANGE`, plus `MA_NOACTIVATE`), `taskbar`
- [x] Linux/macOS move end: 250 ms debounce armed by `bubble_drag_start`, re-armed while X11 Button1 is held (`platform::linux::button1_down`); theme from `window.theme()` (macOS) or the portal color-scheme (Linux, only a stated preference applies) on creation and `ThemeChanged`; native Wayland skips snap, clamp and saving
- [x] Move end snaps with peers in `LIVE_USAGE` order, moves only on a change, saves the position
- [x] Commands `bubble_click` (logs until the panel exists), `bubble_drag_start`, `bubble_resize_step`, `bubble_ready`; resize around each centre without snap or save
- [x] Reset position moves the existing bubbles to their defaults
- [x] `src/core/visibility.rs` with tests for every row of the transition table; user show/hide goes through it
- [x] Deleted `src/os/mod.rs` and `src/os/string.rs` (`src/os/theme.rs` was already gone)
- [x] Host: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (183 passed, 1 ignored), `npm run lint`, `npm test` (32 passed), `npm run tauri build -- --no-bundle`; CI green on `windows-latest`, `ubuntu-22.04` and `macos-latest`
- [x] Headless Chromium render of the page with a stubbed `__TAURI__` at 100%, 150% and 200% (layout, both themes, dead corners) and synthetic pointer checks (click, drag past 3 px, no drag on a later hover, Ctrl+wheel, right-click on the pill only)
- [ ] Visual comparison against a v0.5.0 screenshot at 100% and 150% (none in the repo; Windows manual check)
- [ ] Linux desktop (X11) and Windows manual checks below

## Implementation notes

- Rust sends a `theme` event (`{ isDark }`) to every webview when the system theme changes, then fresh payloads, because the fill colours also depend on the theme. Windows reads `SystemUsesLightTheme` on `WM_SETTINGCHANGE` and ignores tao's `ThemeChanged`, which follows the apps setting.
- The visibility state (`user_hidden`, `hidden_by_fullscreen`) lives in `AppState`; `Runtime::apply_visibility(event)` applies `core::visibility::next` and is the entry point for the fullscreen thread. A user show or hide also saves `widget_visible`.
- Window event handlers and the Windows hook hand their Tauri work to a short-lived thread, so a refit runs after tao's own `WM_DPICHANGED` resize and nothing runs inside a window procedure.
- A new bubble picks the monitor its saved position overlaps most (else the nearest, else the primary) for its DPI and work area.
- The head label is measured at its drawn weight (600) for the "5H" fallback; the reserved tail widths ("999d", "100%") are measured at regular weight, as the GDI bubble did.
- The details panel does not exist yet, so a bubble click is only logged.
- On Linux there is no display-change hook, so clamping after a resolution change happens only on Windows. The README note on transparency without a compositor belongs to the docs phase.

## Validation

- Host: `npm test`, `npm run lint` and `cargo clippy --all-targets -- -D warnings` are green.
- Linux desktop (X11, manual; this host has no display): bubbles appear at the bottom right, drag and snap work and persist, pausing mid-drag does not snap, Ctrl+wheel resizes without a snap, right-click opens the menu, the corners are transparent, and the theme switches.
- Windows (CI artifact, manual): no focus is stolen when clicking or showing, and there is no white flash. Move end snaps exactly once, and the Codex bubble stacks above Claude. Mixed-DPI drags keep the size. A hover after a drag does not start another drag. Hide by fullscreen then show renders the bubble (not blank). Reset position moves both bubbles and does not quit the app with the panel never opened. Fast provider off/on recreates the bubble.

## Risks

| Risk | L x I | Mitigation |
|------|-------|------------|
| WebView2 still activates the window on click | M x H | The `WS_EX_NOACTIVATE` patch. If focus is still taken, also return `MA_NOACTIVATE` for `WM_MOUSEACTIVATE` in the subclass hook. |
| `startDragging` swallows `pointerup` | H x L | By design: the end comes from Rust; JS clears its pressed state when it starts the drag. |
| A raw show leaves WebView2 blank | M x M | Show and hide both go through the raw path on Windows; the Validation row checks it. |
| Ctrl+wheel zooms the page | M x M | `preventDefault` in a non-passive listener. Zoom hotkeys are off by default. |
| Transparency fails (no Linux compositor; NVIDIA DMABUF) | M x M | Documented in the README. Do not apply the DMABUF workaround unconditionally. |
| Text metrics differ on Linux | H x L | Accepted (intentional change #10). The "5H" fallback adapts. |

## Rollback

Revert the commits. Phase 04's test window comes back.
