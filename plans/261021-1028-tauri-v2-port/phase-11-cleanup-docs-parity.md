---
phase: 11
title: "Cleanup, docs, parity sign-off"
status: in-progress
priority: P1
effort: "3h"
dependencies: [7, 8, 10]
---

# Phase 11: Cleanup, Docs, Parity Sign-Off

## Context

Earlier phases deleted each Win32 file when they replaced it. This phase
confirms that nothing is left, updates the user-facing docs and runs the Windows
parity checklist before the first Tauri release.

## Requirements

- `grep -rn "windows::" src` must find matches only in `src/platform/windows.rs`.
  No untracked leftovers may remain under `src/os`, `src/tray` or `res/`.
- `README.md`:
  - Platform badges for Windows and Linux x86_64, plus a note that Linux ARM64 and macOS are not yet supported (see `ROADMAP.md`).
  - The WebView2 requirement on Windows 10.
  - Linux runtime deps (webkit2gtk-4.1 and libayatana-appindicator3), the need for a compositor, Wayland limits (positioning works only via Xwayland) and the tray limits (menu only, no tooltip).
  - Settings and log paths per OS, plus the WebView2/WebKit profile dir under the identifier.
  - The privacy list, unchanged apart from removing WinHTTP.
  - The intentional changes from plan.md that users can see (dead corners, toasts, PAC).
  - Fix the drift: tray icons are not optional.
  - Rewrite "Build from source" (`README.md:67-72`, today `cargo build --release`): Node/npm prerequisite, `npm ci`, `npm run tauri build -- --no-bundle` on Windows, `npm run tauri build -- --no-bundle` or `--bundles deb,appimage` on Linux x86_64, the Linux apt list from Phase 01 plus `patchelf` for AppImage, and a note that local bundles are unsigned for the updater (`createUpdaterArtifacts` is off).
- `docs/release-process.md`:
  - The version lives in `Cargo.toml` only (`tauri.conf.json` inherits it). Bump it, then run `cargo build` to refresh `Cargo.lock`.
  - Signing-key custody and secrets, with an offline backup.
  - The asset contract: the exact `usage-monitor.exe`, with no other `*.exe`, plus `.sig` files and `latest.json`.
  - The dry-run steps, the asset rename rule, the legacy-client asset contract from the Phase 10 compatibility table, its residual risk (no end-to-end rehearsal), and the `release-legacy.yml` rollback procedure.
- No plan IDs or phase numbers in code comments or commit messages.

## Windows parity checklist (manual, Windows 10 and 11, at 100% and 150% scale)

- [ ] The bubble shows bottom right, with Codex above Claude. It has no taskbar or Alt-Tab entry, stays topmost, and never steals focus.
- [ ] A saved position on an unplugged monitor resets to the default position at startup.
- [ ] Rings, bars, text, colours and the 95% pulse match a v0.5.0 screenshot. The light and dark switch applies live.
- [ ] A click toggles the panel and a drag moves the bubble. Corner, edge, taskbar and peer snaps match, and the position persists across restarts. A hover right after a drag does not start a new drag.
- [ ] Panel: layout and palette match v0.5.0; it opens below the bubble and flips above near the bottom edge; it hides on blur and on a second click of the same bubble; clicking the other bubble swaps content; no Alt-Tab or taskbar entry; Windows 11 corners round; values refresh live while open; right-click, F5 and Ctrl+wheel do nothing.
- [ ] Ctrl+wheel resizes within 140 to 360 around the centre and resizes every bubble. The menu size items grey out at the limits.
- [ ] Right-click menu: the full structure, check marks and every action work. Re-clicking a checked frequency or interval and unchecking the last provider leave the marks correct. The menu dismisses on a click elsewhere.
- [ ] Tray: one icon per provider, the badge ring, the 4-line tooltip, left-click toggle and right-click menu.
- [ ] Alerts: threshold, session-expired and update-applied notifications appear from a fresh portable exe with no Start-menu shortcut, with the cooldown respected.
- [ ] Fullscreen auto-hide works, as do the DPI change and monitor unplug cases (the bubble is re-clamped).
- [ ] Autostart: a legacy v0.5.0 entry shows checked, there is one entry after two launches, the toggle works, and a Task Manager disable survives two launches.
- [ ] The Phase 10 legacy-client compatibility table has every row marked pass for v0.4.0, v0.4.1 and v0.5.0, and the residual risk is recorded.
- [ ] A second launch exits silently. Restart relaunches (and the old process is gone). Exit ends the process.
- [ ] Without the WebView2 runtime (Windows Sandbox or an LTSC VM), the exe shows the message box with the download link and exits.
- [ ] `--diagnose` writes `%TEMP%\usage-monitor.log`.

## Files

- Modify: `README.md`, `docs/release-process.md`
- Delete: any leftover detached Win32 files (expected: none)

## Todo

- [x] Leftover sweep: the `windows` crate is used only in `src/platform/windows.rs`; the other `windows::` matches are calls into the `platform::windows` module (`src/bubble.rs`, `src/menu.rs`, `src/panel.rs`, `src/platform/mod.rs`). `src/os`, `src/tray` and `res/icon.rc` are gone; the empty untracked `src/i18n/` directory was removed. `res/windows-app-manifest.xml` stays (embedded by `build.rs`), and so do `src/icons/*` (the icon sources `icons/` was generated from)
- [x] Stale Windows-only header comment in `src/creds/local_fs.rs` corrected (the credentials path is per OS)
- [x] `README.md`: Windows and Linux x86_64 badges, Linux ARM64 and macOS unsupported (links `ROADMAP.md`), WebView2 requirement, Linux runtime deps, compositor, Wayland and tray limits, log, settings, update and webview profile paths per OS, the AUMID registration, privacy list, visible intentional changes (dead corners, toasts, PAC/WPAD, per-session lock), tray icons not optional, Linux install (AppImage self-updates, `.deb` check only), rewritten "Build from source"
- [x] `docs/release-process.md`: `Cargo.toml` as the only version source and `cargo build` for `Cargo.lock`, signing-key custody and secrets with an offline backup, the asset table and `latest.json` shape, the rename rule, the legacy-client asset contract, dry-run steps, the `release-legacy.yml` rollback procedure, residual risk
- [x] Links in both docs resolve; claims checked against the code and workflows
- [x] Host: `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked` (216 passed, 1 ignored), `npm run lint`, `npm test`, `npm run tauri build -- --no-bundle -- --locked`
- [ ] Windows parity checklist above, on Windows 10 and 11 at 100% and 150% (manual, needs a Windows machine with the CI artifact; this host has no Windows or display)
- [ ] Linux x86_64 X11 smoke run (manual, needs a desktop)

## Implementation notes

- **`.deb` dependencies checked.** A host-built `.deb` declares `Depends: libayatana-appindicator3-1, libwebkit2gtk-4.1-0, libgtk-3-0`, so the README's "the `.deb` pulls these in" holds.
- **Webview profile paths.** Tauri 2.12 sets the webview data directory to `app_local_data_dir()` on Windows and Linux (`tauri` `src/manager/webview.rs`), which is `<LocalAppData>/<identifier>`; the README lists it per OS.
- **Linux tray.** AppIndicator icons deliver no click events and no tooltip, so the README tells Linux users to use the menu; the badge still renders.
- **Privacy list.** The README never named WinHTTP, so the list is unchanged. The AUMID registry key and icon PNG are listed with the other written files instead.
- **Manual checklist open.** Every row needs a Windows 10/11 machine; the earlier phases' Windows manual checks are also still open. Nothing in this phase changes runtime behaviour, so no parity row can have regressed here.

## Validation

- The checklist is complete. Every failure is either fixed or recorded as an intentional change with user sign-off.
- Every link in the README and release doc resolves, and each claim matches the code.

## Risks

| Risk | L x I | Mitigation |
|------|-------|------------|
| A parity failure is found late | M x M | Each earlier phase already runs its Windows manual check. This phase only confirms them. |

## Rollback

Revert the doc commits. No code changes in this phase.
