---
phase: 7
title: "Details panel window"
status: in-progress
priority: P2
effort: "3h"
dependencies: [6]
---

# Phase 07: Details Panel Window

## Context

`src/panel.rs` is a single opaque `WS_POPUP` toolwindow, fixed at 280x120
logical px and GDI-painted. A bubble click toggles it, and losing focus
dismisses it.

## Requirements

- One `panel` webview window, created hidden at startup with these builder
  options: `decorations(false)`, `always_on_top(true)`, `skip_taskbar(true)`,
  `resizable(false)`, `transparent(false)`, `shadow(false)`, `visible(false)`,
  `title("Usage panel")`, and `LogicalSize(280, 120)`. It is hidden and shown,
  never destroyed: `CloseRequested` (Alt+F4) calls `prevent_close` and `hide()`
  (D18).
- Windows: `platform::windows::apply_toolwindow(hwnd)` ORs `WS_EX_TOOLWINDOW`
  (no `NOACTIVATE`, the panel must take focus) before the first show, so the
  panel has no Alt-Tab entry; `skip_taskbar` alone only removes the taskbar
  button. Keep `shadow(false)` unless the v0.5.0 screenshot comparison shows the
  original had a DWM shadow; record the result here.
- `panel::toggle(slug)`:
  - If the panel is visible with the same provider, hide it.
  - Otherwise set the provider, compute the position with
    `core::geom::place_near`, using the bubble's outer rect, the panel size
    scaled by the bubble monitor's DPI, and the union of all monitor rects
    (physical 8 px gap, parity).
  - Emit `panel:update` with the `PanelPayload` (plan.md, Data flow), `show()`,
    then `set_focus()`.
- Windows: re-apply `DwmSetWindowAttribute(DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND)`
  through `platform::windows::round_corners(hwnd)` on each show.
- Dismissal: `WindowEvent::Focused(false)` hides the panel. Ignore a toggle-show
  that arrives within 150 ms of a blur-hide for the same provider, so a second
  click on the bubble closes the panel instead of reopening it. Test this on
  Windows first. If v0.5.0 behaves differently, match v0.5.0.
- `refresh_if_visible(payload)` updates the panel in place, without moving it,
  only for the matching provider. Called from `propagate` outside the state lock
  (D14).
- UI (`ui/panel.html`, `ui/panel.css`, `ui/panel.js`):
  - Absolute layout with the exact coordinates in `src/panel.rs:299-499`: 4 px
    accent stripe, a 13 px/600 header at (14,14), rows at y=38 and y=68, bars at
    x=46 that are 120 wide and 14 high, and right text at x=174, 96 wide,
    clipped with no ellipsis.
  - Square bars whose fill width is `round(clamp(pct)/100*120)`.
  - No pulse and no transitions.
  - Panel palette: dark `#1F1F1F`/`#EAEAEA`/`#3A3A3A`, light `#FAFAFA`/`#1F1F1F`/`#D6D6D6`.
  - Fill colours come from the payload.
  - `get_panel_state` is called on load so the first paint already has data.
  - Texts come from the payload (`sessionText`, `weeklyText`: `format_window`,
    `'!'`, `'…'` or `''`) and the header from `label`; JS formats nothing.
  - WebView defaults are neutralised, because today the panel ignores all
    input: `preventDefault` on `contextmenu`, on `wheel` with `ctrlKey`, and on
    `keydown` for browser accelerators (F5, F7, F12, Ctrl/Cmd+R, P, F, G, S, U,
    O, +, -, 0). If the pinned Tauri/wry exposes a WebView2 "browser accelerator
    keys" setting, also turn it off for the panel on Windows.

## Files

- Create: `src/panel.rs` (rewrite), `ui/panel.html`, `ui/panel.css`, `ui/panel.js`
- Modify: `src/commands.rs` (`get_panel_state`), `src/runtime.rs` (create the panel), `src/platform/windows.rs` (`round_corners`, `apply_toolwindow`), `src/lib.rs`

## Steps

1. Port `place_near` (already done in Phase 02) and wire the window.
2. Write the HTML and CSS, then compare against a v0.5.0 screenshot.

## Todo

- [x] `src/panel.rs` rewritten: the `panel` webview with the listed builder options, created hidden at startup, hidden and shown but never destroyed; Alt+F4 hides it
- [x] `panel::toggle`: same provider hides, otherwise `place_near` with the bubble's outer rect, the panel sized for the bubble's DPI and the union of all monitors, then `panel:update`, `show()` and `set_focus()`
- [x] Focus loss hides the panel; a toggle within 150 ms of that hide for the same provider is ignored (pure `toggle_state`/`blur_state`, unit-tested)
- [x] `refresh_if_visible` from `propagate`, outside the state lock, checked on the main thread against the provider on screen
- [x] `src/platform/windows.rs`: `apply_toolwindow`, `round_corners` on each show, `disable_browser_accelerator_keys`; `apply_no_activate` shares the style helper
- [x] `get_panel_state` command; `bubble_click` toggles the panel
- [x] `ui/panel.html`, `ui/panel.css`, `ui/panel.js`: GDI coordinates, square bars, palette per `[data-theme]`, texts and colours from the payload only, row labels from `get_strings`, `contextmenu`, Ctrl+wheel and browser accelerator keys suppressed
- [x] `ui/panel-view.js` (`panelFillWidth`, `isBrowserAccelerator`) with `tests/js/panel-view.test.js`
- [x] Host: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (191 passed, 1 ignored), `npm run lint`, `npm test` (37 passed), `npm run tauri build -- --no-bundle`; CI green on `windows-latest`, `ubuntu-22.04` and `macos-latest`
- [x] Headless Chromium render of the page with a stubbed `__TAURI__` in both themes (coordinates, fill widths, clipping)
- [ ] Visual comparison against a v0.5.0 screenshot and the DWM shadow check (none in the repo; Windows manual check)
- [ ] Linux desktop (X11) and Windows manual checks below

## Implementation notes

- The row labels ("5h", "7d") are not in `PanelPayload`; the page reads them once from the existing `get_strings` command, so the payload contract stays as planned.
- The pure page helpers live in `ui/panel-view.js` instead of `ui/panel.js`, because `panel.js` reads `window.__TAURI__` on import and cannot run under `node --test`.
- Tauri does not expose wry's `with_browser_accelerator_keys`, so `platform::windows::disable_browser_accelerator_keys` sets `AreBrowserAcceleratorKeysEnabled` through `with_webview` and reloads the page once at startup, because the setting applies from the next navigation. The `keydown` handler covers the same keys on every platform.
- `bubble_click` is an async command, so the toggle is queued to the main thread instead of running inside the webview's IPC callback. Toggles and refreshes both run on the main thread, which keeps a refresh from drawing a provider the panel no longer shows.
- The panel background colour is set from the theme before every show, and the payload is emitted before `show()`, against a white or stale flash.
- `shadow(false)` is kept until the Windows screenshot comparison says otherwise.
- Under native Wayland the panel is only resized, not moved (D12).

## Validation

- Host: `npm run lint` and `cargo clippy -D warnings` are green.
- Linux desktop (X11, manual): clicking a bubble opens the panel below it, or above it near the bottom edge. Clicking elsewhere closes it, clicking the same bubble closes it, and clicking the other bubble swaps its content.
- Windows (CI artifact, manual): layout and palette match a v0.5.0 screenshot; placement below the bubble and flipped above near the bottom edge; dismissal on blur and on a second click of the same bubble; rounded corners on Windows 11; no Alt-Tab or taskbar entry; live refresh while open; Alt+F4 hides and a later click reopens; right-click and F5 do nothing.

## Risks

| Risk | L x I | Mitigation |
|------|-------|------------|
| Focus is refused (Windows foreground lock, GNOME), so the panel never auto-hides | M x M | Parity: clicking the bubble toggles it closed, as today. |
| White flash on the first show in dark theme | M x L | Pre-create the panel, call `set_background_color` from the theme, and populate it before `show()`. |
| Wayland ignores `set_position` | H x L | Best effort under D12. |

## Rollback

Revert the commits. `bubble_click` goes back to being a stub.
