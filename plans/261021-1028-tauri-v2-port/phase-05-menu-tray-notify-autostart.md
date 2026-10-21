---
phase: 5
title: "Menu, tray, alerts, autostart"
status: pending
priority: P1
effort: "6h"
dependencies: [4]
---

# Phase 05: Menu, Tray, Alerts, Autostart

## Context

These replace the following Win32 pieces:

- `src/tray/mod.rs`: `Shell_NotifyIconW` add/modify/delete and balloons.
- `src/tray/callback.rs`.
- The `pixmap_to_hicon` block in `src/tray/badge.rs`.
- The `TrackPopupMenu` menu (`src/app.rs:923-1193`), which calls `SetForegroundWindow(owner)` first (`src/app.rs:1151-1152`).
- The Run-key autostart (`src/app.rs:1514-1527`, value name `ClaudeCodeUsageBubble` at `src/app.rs:51`).

The user approved `tauri-plugin-notification` with Windows AUMID registration
for the alerts (D9).

## Requirements

- **Menu** (`src/menu.rs`):
  - Map `core::menu_model` onto `tauri::menu` (`Submenu`, `CheckMenuItem`, `MenuItem` with `enabled(false)`, `PredefinedMenuItem::separator`), keeping the exact order and labels.
  - The checked state of "Start with Windows" / "Start at login" comes from `autolaunch().is_enabled()`.
  - OpenCode Go is a disabled `CheckMenuItem`.
  - The version item reads `"<status> · v<version>"` and is disabled while Checking or Applying. In the package-manager channel it is also disabled while Available (Phase 09).
  - `menu::refresh()` rebuilds the menu from a state snapshot (D14) and calls `set_menu` on every tray. It runs whenever any input changes (settings, update status, size, autostart) **and after every handled check-item event, including no-op ones** (re-selecting the current frequency or interval, unchecking the last live provider). Native check items toggle themselves on click, so only a rebuild from state keeps the marks right.
  - `show_context_menu(label)`: on Windows, call `SetForegroundWindow(bubble hwnd)` first (parity with `src/app.rs:1151`; the own-PID fullscreen check already ignores it), then `window.popup_menu(&menu)` on that bubble.
  - A single `on_menu_event` dispatches the string ids to the same actions as `on_menu_command`.
- **Tray** (`src/tray.rs`):
  - One `TrayIconBuilder::with_id(slug)` per enabled live provider, in `LIVE_USAGE` order, with `show_menu_on_left_click(false)`.
  - A left-button `Click` with `button_state: Up` toggles the widget.
  - The badge is `badge::render_pixmap` (tiny-skia, kept), demultiplied into `Image::new_owned(rgba, 32, 32)`, with `is_dark=true` always (parity).
  - The tooltip comes from `core::state::tray_tooltip`, truncated to 127 UTF-16 units.
  - `sync()` diffs the set of icons. `set_icon` and `set_tooltip` run only when the cached `(pct, colour)` or the text changed.
  - Linux: before building any tray, probe with `libloading` for `libayatana-appindicator3.so.1`, then `libappindicator3.so.1`. If neither loads, log it and skip tray creation, because the tray library panics on a missing library and the release profile aborts on panic. Any other tray creation error is logged and the app continues.
- **Alerts** (D9):
  - Windows spike first: a fresh portable exe (CI artifact, no Start-menu shortcut, outside `target/`) on Windows 10 and 11 calls `platform::windows::register_aumid()` and shows one toast. If no toast appears, stop and report to the user before writing more alert code.
  - `register_aumid()` runs at startup before any toast: write `HKCU\Software\Classes\AppUserModelId\io.github.tiennm99.claudecodeusagebubble` with `DisplayName` "Claude Code Usage Bubble" and `IconUri` pointing at a PNG written to `data_local_dir()/ClaudeCodeUsageBubble/icon.png`, then `SetCurrentProcessExplicitAppUserModelID` with the same id.
  - `tauri-plugin-notification` shows the alerts returned by `apply_results` and the pending "Update applied" alert from `--updated-to`. The update-applied alert ignores the cooldown.
- **Autostart** (D10):
  - `tauri_plugin_autostart::Builder::new().app_name("ClaudeCodeUsageBubble")`, `MacosLauncher::LaunchAgent`, no args. Pin the Windows current-user mode if the Builder exposes it.
  - On the pinned plugin version, read its Windows source and record in this file: what data `enable()` writes, whether `is_enabled()` compares the data, and whether `enable()` writes StartupApproved. If `is_enabled()` ignores the data, no migration code is written. Otherwise add the one-time data rewrite from D10 (legacy data exactly `"<current exe>"` and different from the plugin format), done through `platform::windows`, never touching StartupApproved.
  - Confirm that the Linux `.desktop` `Exec` uses `$APPIMAGE` when that variable is set. Otherwise, write the entry ourselves.
- **Provider toggle:** keeps the at-least-one invariant, creates or destroys the bubble (Phase 06's `bubble::create`/`destroy`, stubbed until then; D18), syncs the tray, refreshes the menu and polls.

## Files

- Create: `src/menu.rs`, `src/tray.rs`
- Modify: `src/tray/badge.rs`, which moves to `src/badge.rs` and loses the HICON code. Also `src/runtime.rs` (hooks, plugin registration), `src/platform/windows.rs` (`register_aumid`, `set_foreground`, and the autostart data rewrite only if needed), `src/platform/linux.rs` (`appindicator_available`) and `src/lib.rs`. `Cargo.toml` gains `tauri-plugin-notification = "2"`.
- Delete: `src/tray/mod.rs`, `src/tray/callback.rs`, `src/os/registry.rs` (only the theme, proxy and Run reads stay, inside `platform::windows`), `src/os/string.rs` (if no other user remains)

## Steps

1. Port `badge.rs`. Add pixel-sampling tests: the disk colour at the centre, the track colour at radius 12.15 with no percent, and the arc colour at 12 o'clock for any percent above 0. Add a demultiply test.
2. Build the menu mapping. Add a test that walks the `tauri::menu`-independent model, so it can run headless.
3. Add the tray and autostart.
4. Run the toast spike, then register the notification plugin and wire the alerts.

## Validation

- Host: `cargo test badge:: menu_model::` is green.
- Linux desktop (X11 session with AppIndicator, manual; this host has no display): one indicator per enabled provider, and the menu is complete with correct check states. Toggling a provider adds or removes an indicator. In a container without the appindicator library, the app runs without a tray instead of aborting.
- Windows (CI artifact, manual):
  - Left-click toggles; right-click shows the native menu; the tooltip has 4 lines.
  - Re-click the checked frequency, re-click the checked auto-update interval, and uncheck the only enabled provider: the check marks still match state afterwards.
  - The bubble right-click menu dismisses on a click elsewhere.
  - With a legacy `ClaudeCodeUsageBubble` Run value preset, the item shows checked, and there is exactly one entry after two launches. After a Task Manager disable, the item stays unchecked across two launches.
  - Threshold, session-expired and update-applied toasts appear from a fresh portable exe with no shortcut.

## Risks

| Risk | L x I | Mitigation |
|------|-------|------------|
| Multiple tray icons on Windows are unverified | M x H | Manual check in Validation. Fallback: one tray icon whose badge shows the provider with the higher usage. That would be a recorded parity change, so ask first. |
| Toasts do not appear for the portable exe | M x H | AUMID registration, checked by the spike before the alert wiring. On failure, stop and ask the user. |
| `app_name` is missing on the pinned autostart Builder | L x M | The value name would differ; then add the D10 migration (delete legacy value, `enable()` once, guarded by the legacy data format so it never runs twice). |
| Stale menu state, because the tray menu has no on-open hook | M x M | Rebuild on every state mutation and after every check-item event. |
| Deadlock from building menus while holding the state lock | M x H | D14 snapshot rule. |

## Rollback

Revert the commits. The app keeps running without a tray or menu, as at the end of Phase 04.
