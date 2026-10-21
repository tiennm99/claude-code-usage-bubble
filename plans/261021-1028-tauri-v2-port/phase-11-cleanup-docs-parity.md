---
phase: 11
title: "Cleanup, docs, parity sign-off"
status: pending
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
  - The asset contract: the exact `claude-code-usage-bubble.exe`, with no other `*.exe`, plus `.sig` files and `latest.json`.
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
- [ ] `--diagnose` writes `%TEMP%\claude-code-usage-bubble.log`.

## Files

- Modify: `README.md`, `docs/release-process.md`
- Delete: any leftover detached Win32 files (expected: none)

## Validation

- The checklist is complete. Every failure is either fixed or recorded as an intentional change with user sign-off.
- Every link in the README and release doc resolves, and each claim matches the code.

## Risks

| Risk | L x I | Mitigation |
|------|-------|------------|
| A parity failure is found late | M x M | Each earlier phase already runs its Windows manual check. This phase only confirms them. |

## Rollback

Revert the doc commits. No code changes in this phase.
