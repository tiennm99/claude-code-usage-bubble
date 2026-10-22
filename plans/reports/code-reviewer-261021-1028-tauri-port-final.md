# Final Audit: Tauri v2 Port (`feat/tauri-port` vs `main`)

Date: 2026-10-21. Scope: 55 commits `main..HEAD`, 124 files, about +22.9k / -6.7k
lines. Plan: `plans/261021-1028-tauri-v2-port/plan.md`.

## Verdict

The automatable side is done. Every row of the feature-to-phase map has code
behind it, the latest CI run is green on Windows, Linux and macOS, nothing
Win32-only is left over, and the commits are clean. What remains is user-only
work: the signing key, the release dry runs, and the manual Windows parity and
Linux X11 checks.

## 1. Feature parity (plan.md feature-to-phase map)

| Feature | Evidence |
|---------|----------|
| Bubble styles: topmost, no taskbar, no activation, move cursor | `src/bubble.rs:323-329`, `src/platform/windows.rs:193` (`apply_no_activate`), `:292` (`SW_SHOWNOACTIVATE`), `ui/bubble.css:40` |
| Stadium, size 140..360 step 20 | `src/core/geom.rs:77-80` |
| Rings, time ring, bars, text, '5H' fallback, placeholders | `ui/bubble.js:198` (dasharray), `ui/layout.js:10`, `:334`; strings in `src/core/payload.rs`, `src/core/i18n.rs` |
| Colours, theme, live re-theme (intentional change 5) | `src/platform/windows.rs:57-62`, `WM_SETTINGCHANGE` hook `:309` and `src/bubble.rs:414`; Linux portal `src/platform/linux.rs:217-220` |
| Pulse at 95%+, 60 s time-progress refresh | `ui/layout.js:19`, `:275`; `ui/bubble.js:53-54` |
| Drag, click vs drag, snap, clamp | `ui/bubble.js:52`, `:389-414`; `src/core/geom.rs:179` (clamp), `:220` (snap); move-end hook `src/platform/windows.rs:307`; X11 debounce `src/bubble.rs:40`, `:517` |
| Offscreen check on load, reclamp on setting/display change, Codex stagger | `src/runtime.rs:939` to `src/core/settings.rs:242`; `src/bubble.rs:420`; `src/core/geom.rs:89` |
| Default position, DPI change, Ctrl+wheel resize | `src/core/geom.rs:157`; `src/bubble.rs:512`; `ui/bubble.js:453-462` to `src/commands.rs:84` |
| Hit area: dead corners (intentional change 1) | `ui/bubble.css:30-47` |
| Fullscreen auto-hide | `src/fullscreen.rs:15`; `src/platform/windows.rs:449-471`; `src/platform/linux.rs:163` |
| `user_hidden` / `hidden_by_fullscreen` | `src/core/visibility.rs:10-25`; `src/runtime.rs:471-481` |
| Panel: 280x120, blur dismissal, no Alt-Tab, Win11 corners, live refresh | `src/panel.rs:35-36`, `:352`, `:187`, `:318`; `src/platform/windows.rs:246-252`; `src/core/geom.rs:366` (`place_near`) |
| Tray per provider, badge, 4-line tooltip, left-click toggle | `src/tray.rs:2`, `:202`, `:249-265`; `src/badge.rs:20` |
| Context menu, check and grey states, OpenCode Go greyed | `src/core/menu_model.rs:173`, `:374`; `src/menu.rs:190`, `:208` |
| Threshold, session-expired and update-applied alerts, 30 min cooldown, AUMID | `src/core/state.rs:16`, `:48-50`; `src/runtime.rs:351`, `:1026`; `src/platform/windows.rs:124` |
| Poll scheduling, in-flight gate, credential refresh | `src/scheduler.rs:1-31`; `src/runtime.rs:170`, `:282-294`, `:305-345`; `src/usage/refresh.rs:65-104` |
| Provider toggle invariant | `src/core/settings.rs:165-173`; `src/runtime.rs:441-446` |
| Reset position, Restart, Exit, CLI flags, `--diagnose`, WebView2 guard | `src/runtime.rs:499`, `:404-405`, `:708`; `src/cli.rs:44`; `src/diag/mod.rs:19`; `src/runtime.rs:844-848`, `src/platform/windows.rs:90-110` |
| Single instance (per session, intentional change 2) | `src/runtime.rs:873` |
| Settings path, atomic save, normalise | `src/core/settings.rs:191`, `:250-257` |
| Start with Windows (same Run value name) | `src/runtime.rs:56`; `src/platform/windows.rs:169`; `src/menu.rs:78-108` |
| Updater: check, comparator, `TargetNotFound` mapped to up to date, self-replace, relaunch, cleanup | `src/update/mod.rs:109-141`, `:145-163`; `src/update/install.rs:43`; `src/handoff.rs:17`, `:79`; `src/runtime.rs:981` |
| Providers, creds, HTTP, proxy, TLS | `src/net/mod.rs:19`, `:55`, `:118`; `src/core/proxy.rs`; `Cargo.toml` (`ureq` with `platform-verifier`) |
| Icons, version info, manifest | `build.rs` (version merged in, manifest embedded), `res/windows-app-manifest.xml`, `icons/`; CI checks `ProductVersion` against `Cargo.toml` |
| CI, release, legacy compatibility, docs | `.github/workflows/ci.yml`, `release.yml`, `release-legacy.yml`, `scripts/make-latest-json.js`, `docs/release-process.md`, `README.md` |

Deviations from the plan text are documented in the phase notes, not in the
plan.md "Intentionally changed behaviour" list:

- **Windows autostart** goes through `auto-launch` directly (`WindowsEnableMode::CurrentUser`) instead of `tauri-plugin-autostart`. The plugin's builder cannot pin the current-user mode. The Run value name and `is_enabled` behaviour stay the same, and this is recorded in the phase 05 notes.
- **Update apply** sets `Update.timeout = 600 s` instead of building a second updater and checking again. The pinned plugin builds each `Update` with no timeout, so a second check was not needed. This is recorded in the phase 09 notes. Neither change alters behaviour users can see.

## 2. CI

Latest run on `feat/tauri-port` (HEAD `6407e49`, after this audit's doc fix):
<https://github.com/tiennm99/usage-monitor/actions/runs/39590034376>
is **success**. `windows-latest` ran fmt, clippy, tests, lint, `npm test`, the
no-bundle build, the version-resource check and the exe artifact.
`ubuntu-22.04` ran the same plus the deb bundle. `macos-latest` ran
`cargo check`. The previous HEAD `fa889c1` was also green (run 39588741335).

Host re-run: `cargo fmt --check` OK. `cargo clippy --all-targets --locked -D warnings`
OK. `cargo test --locked` passed 216, with 1 ignored. `npm run lint` OK.
`npm test` passed 52. `actionlint` and `yamllint` OK on every workflow.

## 3. Hygiene

- **Leftover Win32 code:** none. `src/os/`, `src/tray/`, `src/app.rs`, `src/settings.rs`, `src/net/winhttp.rs`, `src/update/{release,handoff}.rs` and `res/icon.rc` are deleted. The `windows` crate is used only in `src/platform/windows.rs`; `std::os::windows` in `src/handoff.rs:52` and `src/usage/refresh.rs:103` is std, behind `cfg(windows)`. `res/windows-app-manifest.xml` is embedded by `build.rs`. `src/icons/*` are the icon sources and stay.
- **Commits:** all 55 are authored by `tiennm99 <tiennm99@outlook.com>`, carry no trailers and contain no AI attribution. Messages follow conventional commits and hold no phase numbers, decision IDs or finding codes.
- **Code comments:** no phase, decision or finding IDs. The only hits are `F5`/`F12` key names, `Alt+F4` and the pulse wave "phase", all of which are legitimate.
- **Docs:**
  - `README.md` matches the code: badges, WebView2, Linux dependencies, Wayland and tray limits, the `--diagnose` and file paths, the autostart label, the visible intentional changes, and build from source.
  - `ROADMAP.md` matches the plan's out-of-scope list.
  - `docs/release-process.md` said "Both release workflows refuse to build while [pubkey] is empty". `release-legacy.yml` never reads the pubkey; it checks only the private key secret. **Fixed** in `6407e49` (`docs: say only the Tauri release workflow checks the updater public key`), and CI is green on it.

## Larger gaps (not fixed here; user action or plan bookkeeping)

1. **Release blocker:** `tauri.conf.json` `plugins.updater.pubkey` is `""`. `release.yml` refuses to run until it is set, and `build.rs` refuses a signed build without it. The user must generate the key pair, set the pubkey, store both secrets and keep an offline backup.
2. **`release.yml` dispatch dry run not run.** This blocks the success criterion on the six-asset draft. Its dry run needs the key (item 1). `release-legacy.yml` has a separate, later prerequisite: GitHub dispatches it only from the default branch, so it can only be dry-run after the merge (`docs/release-process.md`).
3. **Manual checks open:** the Windows parity checklist (below), the D9 toast spike (phase 05 gate), and the Linux X11 smoke run.
4. **Plan bookkeeping is stale:** the plan.md frontmatter says `status: pending`, and the phase table shows 05 in progress and 06-11 pending. The phase files say 01-04 are completed and 05-11 are in progress, waiting only on manual checks. Leave the fix to the plan owner.
5. **Low:** `Registry::credential_signatures` (`src/usage/registry.rs:69`) has no callers. It is portable code and was already dead on `main`, so it is not Win32 debris. Remove it opportunistically.

## Manual Windows parity checklist (CI exe artifact; Windows 10 and 11; 100% and 150%)

1. **First launch:** the bubble appears bottom right, with Codex stacked above Claude. It has no taskbar or Alt-Tab entry, stays topmost and never steals focus.
2. **Saved position on an unplugged monitor:** at startup the bubble resets to the default position.
3. **Visuals** match a v0.5.0 screenshot: rings, bars, text, colours, the 95% pulse. A light/dark switch applies live.
4. **Click and drag:** a click toggles the panel and a drag moves the bubble. Corner, edge, taskbar and peer snaps work, and the position survives a restart. Hovering right after a drag does not start a new drag. The transparent corners ignore clicks.
5. **Panel:**
   - Its layout and palette match v0.5.0.
   - It opens below the bubble and flips above it near the bottom edge.
   - It hides on blur and on a second click of the same bubble. Clicking the other bubble swaps its content.
   - It has no Alt-Tab or taskbar entry, and its corners are round on Windows 11.
   - Values refresh live while it is open.
   - Right-click, F5 and Ctrl+wheel do nothing.
6. **Resize:** Ctrl+wheel resizes from 140 to 360 around the centre, and every bubble resizes. The menu size items grey out at the limits.
7. **Right-click menu:** the full structure, check marks and every action work. Re-checking a checked frequency or interval, and unchecking the last provider, leave the marks correct. A click elsewhere dismisses the menu.
8. **Tray:** one icon per provider, the badge, the 4-line tooltip, left-click toggle and right-click menu.
9. **Alerts:** the threshold, session-expired and update-applied toasts appear from a fresh portable exe with no Start-menu shortcut. The 30-minute cooldown holds.
10. **Fullscreen and displays:** the bubbles hide while an app is fullscreen and come back after it. A DPI change or monitor unplug reclamps them.
11. **Autostart:**
    - A legacy v0.5.0 Run entry shows checked.
    - Two launches leave one entry.
    - The toggle works.
    - A Task Manager disable survives two launches.
12. **Instances:** a second launch exits silently. Restart relaunches and the old process is gone. Exit ends the process.
13. **No WebView2:** in Windows Sandbox or an LTSC VM, the exe shows the download-link message box and exits.
14. **Diagnostics:** `--diagnose` writes `%TEMP%\usage-monitor.log`.
15. **Legacy update (after the first real release):** v0.4.0, v0.4.1 and v0.5.0 each self-update into the Tauri exe. Settings and autostart are kept, and the `.old.*` and `updater-helper-*` leftovers are cleaned up.

## Unresolved questions

None beyond the user actions above.
