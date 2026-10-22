---
phase: 5
title: "Menu, tray, alerts, autostart"
status: in-progress
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
- The Run-key autostart (`src/app.rs:1514-1527`, value name `UsageMonitor` at `src/app.rs:51`).

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
  - Linux: before building any tray, probe with `libloading` for the names the tray backend loads, in its order: `libayatana-appindicator3.so.1`, `libappindicator3.so.1`, then the unversioned `libayatana-appindicator3.so` and `libappindicator3.so` (its default `backcompat` feature; sometimes the only names in an AppImage). The loaded library stays loaded. If none loads, log it and skip tray creation, because the tray library panics on a missing library and the release profile aborts on panic. Any other tray creation error is logged and the app continues.
- **Alerts** (D9):
  - Windows spike first: a fresh portable exe (CI artifact, no Start-menu shortcut, outside `target/`) on Windows 10 and 11 calls `platform::windows::register_aumid()` and shows one toast. If no toast appears, stop and report to the user before writing more alert code.
  - `register_aumid()` runs at startup before any toast: write `HKCU\Software\Classes\AppUserModelId\io.github.tiennm99.usagemonitor` with `DisplayName` "Usage Monitor" and `IconUri` pointing at a PNG written to `data_local_dir()/UsageMonitor/icon.png`, then `SetCurrentProcessExplicitAppUserModelID` with the same id.
  - `tauri-plugin-notification` shows the alerts returned by `apply_results` and the pending "Update applied" alert from `--updated-to`. The update-applied alert ignores the cooldown.
- **Autostart** (D10):
  - `tauri_plugin_autostart::Builder::new().app_name("UsageMonitor")`, `MacosLauncher::LaunchAgent`, no args. Pin the Windows current-user mode if the Builder exposes it.
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

## Todo

- [x] `src/badge.rs`: moved from `src/tray/badge.rs` without the HICON code; `demultiplied_rgba` for `Image::new_owned`; pixel tests for the disk centre, the track at radius 12.15, the arc at 12 o'clock for every percent above 0, a quarter sweep, transparent corners, and demultiply
- [x] `src/menu.rs`: `core::menu_model` mapped onto `Menu`, `Submenu`, `CheckMenuItem`, `MenuItem` (hints disabled) and separators; autostart check from `autolaunch().is_enabled()`; one `on_menu_event` that runs the command and then rebuilds every tray menu (also for no-op clicks); `show_context_menu(label)` with `SetForegroundWindow` first on Windows
- [x] Headless menu test: the full tree (order, labels, ids, check and grey states) walked in `core::menu_model`
- [x] `src/tray.rs`: one `TrayIconBuilder::with_id(slug)` per enabled live provider in `LIVE_USAGE` order, `show_menu_on_left_click(false)`, left `Click`/`Up` toggles the widget, badge always dark, tooltip from `tray_tooltip` cut to 127 UTF-16 units, `set_icon`/`set_tooltip` only on a changed `(pct, colour)` or text
- [x] Linux: `platform::linux::appindicator_available()` probes `libayatana-appindicator3.so.1`, `libappindicator3.so.1`, `libayatana-appindicator3.so`, then `libappindicator3.so` (the backend's order); without any the tray is skipped and logged; other tray errors are logged
- [x] Autostart plugin with `app_name("UsageMonitor")`, no args, `MacosLauncher::LaunchAgent` on macOS (Linux and macOS); Windows pinned to a current-user entry with the quoted exe path through `auto-launch` directly (notes below); no migration code needed
- [x] `platform::windows::register_aumid()` (HKCU `AppUserModelId` key with `DisplayName` and `IconUri`, PNG under `data_local_dir()/UsageMonitor/icon.png`, `SetCurrentProcessExplicitAppUserModelID`) run in `setup()` before any toast
- [x] `tauri-plugin-notification` shows the `apply_results` and session-expired alerts and the pending "Update applied" alert (no cooldown), after the first tray sync
- [x] Provider toggle: at-least-one invariant, bubble window created or destroyed (stand-in window until the real bubble), tray sync, menu rebuild, poll
- [x] `propagate()` now syncs the trays and rebuilds the menus when the tree changed
- [x] Deleted `src/tray/mod.rs`, `src/tray/callback.rs`, `src/os/registry.rs` (and `src/os/theme.rs`, its only user)
- [x] Host: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (168 passed, 1 ignored), `npm run lint`, `npm test`, `npm run tauri build -- --no-bundle`
- [ ] Windows toast spike on Windows 10 and 11 from the CI artifact (manual; no Windows machine here). This is the D9 gate and the phase stays in progress until it passes; the alert wiring already exists, so a failed spike goes to the user before Phase 06 builds on it
- [ ] Linux desktop (X11) and Windows manual checks below

## Implementation notes

- **Autostart plugin findings** (`tauri-plugin-autostart` 2.7.0 on `auto-launch` 0.6.0):
  - Windows `enable()` writes the `Run` value data as `"<app_path> <args joined>"`, so with no args it is the **unquoted** exe path plus a trailing space. The legacy data was the quoted path.
  - Windows `is_enabled()` only checks that the `Run` value exists (in HKLM or HKCU) and is not disabled in `StartupApproved\Run`; it never compares the data. A legacy entry therefore shows checked and no data rewrite is written.
  - Windows `enable()` also writes the "enabled" bytes to `StartupApproved\Run` when that key exists. That only happens when the user ticks the item, which is the user re-enabling; our own code never touches `StartupApproved`, and `is_enabled()` honours a Task Manager disable.
  - The plugin Builder does not expose the Windows enable mode, so the plugin would use `auto-launch`'s default `Dynamic`: `enable()` tries HKLM first, so an elevated tick writes an all-users entry that a normal untick cannot remove (`disable()` ignores the HKLM access-denied error and `is_enabled()` still finds it), and that entry holds the unquoted path. Windows therefore does not register the plugin: `platform::windows::autostart_entry` builds an `auto_launch::AutoLaunch` with `WindowsEnableMode::CurrentUser` and the quoted exe path. Its data is `"<exe>" ` (the legacy quoted path plus the trailing space `auto-launch` always appends); `is_enabled()` is unchanged (value present in HKLM or HKCU, not disabled in `StartupApproved\Run`), so a legacy entry still shows checked. Linux and macOS keep the plugin (D10).
  - Linux `enable()` writes `~/.config/autostart/UsageMonitor.desktop`, and the plugin uses `$APPIMAGE` (Tauri's `env().appimage`) as the path when it is set, so no entry of our own is needed. The `Exec` path is not quoted.
- `menu::refresh` is `Runtime::refresh_shell(force)`: on the main thread it snapshots state (lock released before any Tauri call), then syncs the trays and, when the menu tree differs from the last one applied (or after any menu click), gives every tray a freshly built menu. Countdown ticks and polls therefore rebuild no menu unless something in it changed. The snapshot is taken inside the main-thread closure because a closure queued from a worker runs after one a menu click runs inline; a snapshot taken on the calling thread could otherwise re-apply a stale tree and recreate a removed tray icon.
- The left-click handler is registered once on the Tauri builder (`Builder::on_tray_icon_event`), not per icon: Tauri calls a per-icon handler while holding the per-icon listener table, and the inline main-thread refresh may build a missing icon, whose registration locks that same table (a same-thread deadlock).
- Each tray gets its own `Menu` instance; the bubble right-click builds a fresh one and pops it from the main thread after the async command returns.
- The bubble windows are still the decorated JSON stand-ins; one is created per enabled provider. A window destroyed by a provider toggle is recreated from its `Destroyed` event if the provider is enabled again, except while the process is exiting. Outside `setup()`, bubbles are built on a short-lived thread (Tauri documents that building a webview window inside a menu or window event handler deadlocks on Windows); a process-wide lock, never taken by the main thread, serialises the toggle and the recreate path so only one builds the window. Widget show/hide uses Tauri `show`/`hide` until the no-activation path exists. Reset position and size changes update and save the settings and push new payloads; moving and resizing windows arrives with the bubble window.
- The version item calls the update-check hook, which still only logs until the updater exists.
- The stand-in page forwards `contextmenu` to `show_context_menu` so the menu can be checked before the real bubble UI.
- The Windows toast spike could not run here. The AUMID registration and the notification wiring are implemented; the toast check stays a manual Windows check on the CI artifact, and if no toast appears the alerts need the user's decision before more work.

## Validation

- Host: `cargo test badge:: menu_model::` is green.
- Linux desktop (X11 session with AppIndicator, manual; this host has no display): one indicator per enabled provider, and the menu is complete with correct check states. Toggling a provider adds or removes an indicator. In a container without the appindicator library, the app runs without a tray instead of aborting.
- Windows (CI artifact, manual):
  - Left-click toggles; right-click shows the native menu; the tooltip has 4 lines.
  - Re-click the checked frequency, re-click the checked auto-update interval, and uncheck the only enabled provider: the check marks still match state afterwards.
  - Toggle a provider off and on from the tray menu, once slowly and once quickly: the app does not hang, and exactly one bubble and one tray icon come back.
  - Tick "Start with Windows" from an elevated run: only `HKCU\…\Run` gets the value, with the quoted path; a normal run can untick it.
  - The bubble right-click menu dismisses on a click elsewhere.
  - With a legacy `UsageMonitor` Run value preset, the item shows checked, and there is exactly one entry after two launches. After a Task Manager disable, the item stays unchecked across two launches.
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
