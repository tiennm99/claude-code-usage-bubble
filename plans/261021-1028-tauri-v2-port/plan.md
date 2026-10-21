---
title: Port the usage bubble to Tauri v2
description: >-
  Rebuild the Win32 bubble, panel, tray and updater on Tauri v2 with a plain
  HTML/CSS/SVG/JS UI, keeping Windows parity, running on Linux x86_64 and cfg-gating macOS.
status: pending
priority: P1
effort: 51h
branch: feat/tauri-port
tags:
  - tauri
  - cross-platform
  - port
  - updater
created: '2026-10-21'
---

# Port The Usage Bubble To Tauri v2

## Outcome

The app runs on Tauri v2. Windows behaves the same as v0.5.0, and existing
v0.4.0+ users self-update into the Tauri build without losing settings or
autostart. Linux x86_64 builds and runs (X11 full, Wayland best-effort). macOS
compiles behind cfg-gated stubs so it can be finished later. Linux ARM64, a macOS
build, Wayland-native positioning and a rename are later work in the root
`ROADMAP.md`, not part of this plan.

## Stack (approved)

- Rust core plus Tauri v2. Approved plugins: tray (core `tray-icon` feature),
  updater, autostart, single-instance and notification (approved for the alerts,
  with Windows AUMID registration, D9).
- UI: plain HTML/CSS/SVG and plain JavaScript ES modules in a static `ui/`
  folder, with `withGlobalTauri`. No framework, bundler or TypeScript.
- ESLint flat config with `eslint-plugin-jsdoc`. Pure JS modules are tested with
  `node --test`, which adds no dependency.
- npm only. `@tauri-apps/cli` is a local devDependency.
- `ureq` 3 (rustls plus platform verifier) replaces WinHTTP.
- macOS Keychain credentials (keyring) are deferred, as the user requested.

## User decisions (2026-10-21)

- Alerts use `tauri-plugin-notification` with Windows AUMID registration (D9).
- The transparent stadium corners become dead corners (D4).
- There is no end-to-end update rehearsal and no throwaway repo. Phase 10
  instead checks, statically and against the code at each released tag, that the
  new release assets satisfy every legacy client. The end-to-end path stays
  unproven until the first real release (residual risk, Phase 10).
- Linux ships x86_64 only. There are no arm64 CI rows, release jobs or
  `linux-aarch64` `latest.json` entry.
- This host (headless Linux ARM64, no display, no patchelf, no xvfb) verifies with
  `cargo fmt --check`, `cargo clippy`, `cargo test`, `npm run lint`, `npm test`
  and `npm run tauri build -- --no-bundle` (or `cargo build`). Bundles (AppImage,
  `.deb`) are built only in CI, where the runner installs patchelf. Checks that
  need a display are marked "Linux desktop" and run by hand on an X11 desktop.

## Decisions

| ID | Decision | Reason |
|----|----------|--------|
| D1 | **Layout: one package at the repo root, lib plus thin bin.** `Cargo.toml`, `build.rs`, `tauri.conf.json`, `.taurignore`, `capabilities/`, `icons/`, `src/`, `ui/`, `tests/js/` and `package.json` live at the root. There is no `src-tauri/`. `src/lib.rs` (`[lib] name = "claude_code_usage_bubble_lib"`) declares every module `pub` and exposes `run()`. `src/main.rs` only calls `run()` and carries `windows_subsystem`. Portable logic goes in `src/core/`, OS code in `src/platform/{windows,linux,macos}.rs`. JS tests live in `tests/js/`, so they are not embedded in the binary. `.taurignore` lists `ui/`, `tests/`, `scripts/`, `plans/`, `docs/` and `node_modules/`, so UI edits do not trigger a cargo rebuild. | Git history stays on `src/`. Unused `pub` items in a lib do not trip `dead_code`, so `clippy -D warnings` holds while later phases are still wiring consumers. The `_lib` suffix avoids the Windows pdb name clash between a lib and a bin of the same name (the Tauri template does the same). |
| D2 | **Rendering:** the bubble is an HTML stadium pill (`border-radius: 9999px`). Head rings are SVG circles drawn with `stroke-dasharray`, tail bars are pill divs. Rust sets the window to the exact integer physical size and sends `canvasW`, `canvasH` (physical) and `dpi` in the payload; `ui/layout.js` works from those, never from `innerWidth * devicePixelRatio`. Stroke clamps (ring 2..4, time ring 2..3 physical px) carry over. Text widths come from canvas `measureText`. The bubble palette (Phase 06 table, `src/bubble.rs:1406-1427`, `1857-1866`) is CSS custom properties keyed by `[data-theme]`. Fill colours come from Rust `bar_fill_color` as hex. **Every displayed string is formatted in Rust** (half-to-even percent, countdowns, placeholders); JS never formats a number for display. The pulse is a JS 80 ms step with the exact per-element rules in Phase 06. | One owner per rule. Rust owns colours and strings; JS owns pixels. |
| D3 | **Interaction:** pointerdown (button 0) calls `setPointerCapture` and records the screen point. Pointermove requires `(e.buttons & 1) === 1`; once the pointer has moved 3 or more physical px, JS invokes `bubble_drag_start`, clears its pressed state and calls `startDragging()`. Pressed state also clears on `pointercancel`, `lostpointercapture` and `blur`. A release before the threshold invokes `bubble_click`. On Windows, a `SetWindowSubclass` hook on the bubble HWND catches `WM_EXITSIZEMOVE` (move end), `WM_SETTINGCHANGE` (theme recheck and reclamp) and `WM_DISPLAYCHANGE` (reclamp). On Linux and macOS, move end is a 250 ms debounce on `WindowEvent::Moved` that is **armed only by `bubble_drag_start`**; on X11 the debounce also checks Button1 with x11rb `QueryPointer` and re-arms while it is still held. Snap disarms before its own `set_position`, so programmatic moves (snap, clamp, resize recentre, Reset position) never snap or persist. `contextmenu` invokes `show_context_menu`. Ctrl+wheel uses a non-passive `wheel` listener that invokes `bubble_resize_step`. | Matches the HTCAPTION move loop on Windows, avoids mid-drag snaps on X11 and keeps "no snap or save after resize or clamp". |
| D4 | **Hit area (user-approved):** the transparent stadium corners become dead zones (CSS `pointer-events: none` outside the pill) instead of click-through. | WebView windows take input across their whole rectangle. `SetWindowRgn` loses the anti-aliased edge. The area involved is about 0.21·h² (roughly 920 px² at 66 px). |
| D5 | **No activation (Windows):** bubbles are built with `focused(false)` and `focusable(false)`. `platform::windows::apply_no_activate` ORs `WS_EX_NOACTIVATE \| WS_EX_TOOLWINDOW` into bubble HWNDs; `apply_toolwindow` ORs only `WS_EX_TOOLWINDOW` into the panel HWND (it must take focus). Both run before the first show. On Windows, bubble show and hide both go through `platform::windows` (`SW_SHOWNOACTIVATE` / `SW_HIDE`), never mixed with Tauri `show()`/`hide()`. Bubbles are created hidden; JS invokes `bubble_ready` right after it applies the first `get_bubble_state` result to the DOM (no `requestAnimationFrame`), and Rust shows the bubble anyway 1.5 s after creation if `bubble_ready` has not arrived. | Keeps today's never-steals-focus behaviour, keeps the panel out of Alt-Tab, avoids a white flash, and cannot leave a bubble invisible because a hidden webview throttles rendering. |
| D6 | **Data layer:** the `usage/` and `creds/` modules stay. `net/` keeps the same `Client`/`Response` surface, backed by ureq with `http_status_as_error(false)`, gzip, a 60 s connect timeout, a 30 s receive timeout, the same User-Agent and the platform verifier. A Windows system proxy is read from `HKCU\…\Internet Settings` with per-scheme parsing and `ProxyOverride` bypass (Phase 03); PAC/WPAD is dropped (intentional change). `refresh.rs` gates `CREATE_NO_WINDOW` and the `cmd.exe`/`powershell` candidates behind `cfg(windows)`; Unix uses the plain `claude` and `codex` commands. | Providers compile unchanged and 401/403 handling and header fallbacks keep working. |
| D7 | **Settings: no migration.** The path stays `dirs::config_dir()/ClaudeCodeUsageBubble/settings.json`, and the schema, key names and physical-px positions are byte-compatible. No new keys. Tauri `app_config_dir` and the store plugin are not used. Monitor validation moves to a pure function fed by `available_monitors()`. | Existing users keep their position, size, providers and intervals without any conversion step. |
| D8 | **Updater: hybrid on one key.** All platforms use `tauri-plugin-updater` for `check()` and `download()`, which verifies the minisign signature against `latest.json`. A custom `version_comparator` keeps the suffix-ignoring comparison. **Check** uses a builder with a 30 s timeout. **Apply** builds a fresh updater with a 10 min timeout, re-runs `check()` and calls `download()` on that result, because the builder timeout is a total-request timeout that also covers the download. On Windows both builders get the parsed system proxy. A `TargetNotFound` error from `check()` maps to Up to date. Windows stays a **portable exe**: the downloaded bytes go through `self_replace`. The Linux AppImage uses the plugin's `install()`. In both cases the app relaunches itself with `--wait-pid <pid> --updated-to X.Y.Z` and exits only if the spawn succeeded. A `.deb` install is a package-manager channel: the check runs, apply is greyed out. `tauri.conf.json` keeps `createUpdaterArtifacts: false`; only `release.yml` turns it on through `--config`, so local bundle builds need no key. The updater plugin is registered only from Phase 09, together with its `pubkey` and endpoint. No NSIS installer is published. | The portable model is part of Windows parity. Old clients pick the exact asset name through the GitHub API, so they swap straight into the Tauri exe. No released version ever ran the downloaded binary as `--apply-update` (see Backwards compatibility), so that handler is dropped. |
| D9 | **Notifications (user-approved):** `tauri-plugin-notification` carries the threshold, session-expired and update-applied alerts. On Windows, before the first toast, `platform::windows::register_aumid()` writes `HKCU\Software\Classes\AppUserModelId\io.github.tiennm99.claudecodeusagebubble` with `DisplayName` "Claude Code Usage Bubble" and `IconUri` (a PNG written to `data_local_dir()/ClaudeCodeUsageBubble/icon.png`), then sets that AUMID for the process. A Windows spike at the start of the Phase 05 alerts work checks that a fresh portable exe with no Start-menu shortcut shows a toast on Windows 10 and 11; if it does not, stop and report to the user before writing more alert code. The 30-minute cooldown logic stays in Rust. | Tauri tray icons have no balloon API, and these alerts are part of Windows parity. Windows drops toasts for an unregistered AUMID. |
| D10 | **Autostart:** `tauri-plugin-autostart` with `app_name("ClaudeCodeUsageBubble")`, the same Run value name as v0.5.0, so a legacy entry is already the plugin's entry. No migration code runs by default. Phase 05 verifies on the pinned plugin that `is_enabled()` does not compare the value data, that `enable()` writes HKCU (pin the current-user mode if the Builder exposes it) and what data `enable()` writes. Only if `is_enabled()` rejects the legacy `"<exe>"` data does a one-time rewrite happen: when the Run value data equals the legacy quoted path exactly and differs from the plugin format, overwrite the data directly in `platform::windows`. StartupApproved is never written by our code. The label is "Start with Windows" on Windows and "Start at login" elsewhere. | Avoids a double launch, a wrongly unchecked item, and silently undoing a Task Manager disable on every start. |
| D11 | **Fullscreen auto-hide:** Windows reuses `evaluate_foreground_fullscreen` verbatim, with the bubble-class check replaced by "foreground window owned by our PID". Linux X11/Xwayland uses `x11rb` with `_NET_ACTIVE_WINDOW` and `_NET_WM_STATE_FULLSCREEN`. Native Wayland and macOS return `false` (stubs). A 1500 ms thread drives all of them. | The proven logic is kept as-is. The PID check also covers the panel, the popup menu and WebView child windows. |
| D12 | **Wayland:** when `WAYLAND_DISPLAY` is set and `GDK_BACKEND` is unset, set `GDK_BACKEND=x11` before Tauri starts. If no X server is present, positioning features become no-ops and a `(0,0)` position is never saved. | Positioning, snapping and topmost only work under Xwayland. |
| D13 | **Single instance:** `tauri-plugin-single-instance` is registered first, and its callback does nothing. With `--wait-pid <pid>`, the pre-Builder wait loops until that PID is gone, capped at 15 s (today: 5 s wait plus about 3 s of mutex retries), and logs under `--diagnose`. `handoff::relaunch` returns `Result`; callers call `app.exit(0)` only on `Ok`. **Intentional change:** the lock is per user session instead of the `Global\` machine-wide mutex. | The plugin cannot retry, so the wait must outlast a slow WebView2 teardown. A failed spawn must not leave the user with no app. |
| D14 | **Timers and threading:** one `std::thread` scheduler owns the poll deadline, the countdown deadline and the update-check deadline, fed by an `mpsc` channel with `recv_timeout`. The `AtomicBool` in-flight poll gate is kept. Polls run on worker threads. **Hard rule:** no Tauri window, tray, menu or emit API is called while the `AppState` mutex is held. Code takes an owned snapshot through `with_state(\|s\| …)`, drops the guard, then calls Tauri. `propagate`, `menu::refresh`, `tray::sync`, the fullscreen thread and every command follow it. | Plain std, no tokio dependency. Tauri tray, menu and window calls from a worker block on the main thread, so holding the lock across them deadlocks. |
| D15 | **Logging:** keep `diag` (simplelog) behind the `--diagnose` gate, writing to `temp_dir()/claude-code-usage-bubble.log` and truncating on each start. It is initialised before the Builder. A `log_js` command forwards `window.onerror`. | The existing behaviour is kept with no new plugin. |
| D16 | **Identity:** `productName` "Claude Code Usage Bubble", `mainBinaryName` "claude-code-usage-bubble", `identifier` "io.github.tiennm99.claudecodeusagebubble". `tauri.conf.json` has no `version`, so it inherits it from `Cargo.toml`. Release assets are renamed to space-free kebab names (Phase 10). | One version source for the CI tag check. The identifier is permanent, because it names the WebView data dir, the D-Bus single-instance name and the AUMID. |
| D17 | **CI:** `ci.yml` lands in **Phase 01** and runs on pushes to `feat/tauri-port` and `main` and on PRs. Rows: `windows-latest` and `ubuntu-22.04` run fmt, clippy, test, lint, `npm test` and `npm run tauri build -- --no-bundle`; the Windows row uploads the unsigned exe as an artifact (7-day retention), which is the input for every phase's Windows manual check. A `macos-latest` row runs only `cargo check --all-targets` and is the macOS compile gate. `release.yml` (Phase 10) is a matrix covering Windows x64 and Linux x64, plus a publish job. Only CI builds bundles (AppImage needs patchelf, which the runner installs). | Every phase can be verified on Windows without a Windows build host. |
| D18 | **Window lifecycle:** bubbles are removed only with `destroy()`. A new window with the same label is created only after the old one's `Destroyed` event. Reset position moves the existing bubbles with `set_position` instead of recreating them. Bubbles get `prevent_close` on `CloseRequested`; the panel gets `prevent_close` plus `hide()`. `run()` handles `RunEvent::ExitRequested { code: None, api }` with `api.prevent_exit()`, so only an explicit `app.exit(0)` ends the process. | `close()` would be blocked by our own handler, re-using a label before destruction completes fails, and destroying the last window would otherwise quit the app. |

### Intentionally changed behaviour

1. Corner click-through is replaced by dead corners (D4).
2. The single-instance lock is per session, not machine-wide (D13).
3. Windows PAC/WPAD proxy discovery is dropped. The static system proxy, its bypass list and env proxies are still honoured (D6).
4. Balloon notifications become toasts, and the warning/info icon distinction is lost (D9).
5. **Bug fix:** a theme change now reaches already-open bubbles. Today `update_dark_mode` is never called (`src/bubble.rs:396`).
6. **Bug fix:** the auto-update check now repeats every interval. Today the timer is re-armed from a worker thread (`src/app.rs:1450-1455`).
7. **Bug fix:** a saved `widget_visible=false` is applied when bubbles are created at startup and on provider toggle (`src/app.rs:331-353`).
8. The autostart checked state now honours a Task Manager disable, because the plugin reads StartupApproved.
9. Windows file properties now show the real version instead of the stale `0.1.0` (`res/icon.rc`).
10. Fonts: Segoe UI stays first in the stack. Linux and macOS fall back to `system-ui`, so text metrics differ there.
11. The app refuses to start without the WebView2 runtime and shows a message box with the download link instead (Phase 04).

Everything else in the subsystem maps is kept as-is. The quirks are kept too:
the global `last_poll_ok`, the first-poll threshold alert, the unscaled 24 px
Codex stagger, panel placement against the virtual-screen union with a physical
8 px gap, the tray badge always rendering dark, and OpenCode Go staying a greyed,
non-live entry.

## Phases

| # | Phase | Effort | Depends on | Status |
|---|-------|--------|------------|--------|
| 01 | [Scaffold Tauri crate, JS toolchain and CI](phase-01-scaffold-tauri-and-toolchain.md) | 5h | none | pending |
| 02 | [Extract portable core with tests](phase-02-portable-core.md) | 6h | 01 | pending |
| 03 | [Data layer on ureq](phase-03-data-layer-ureq.md) | 4h | 02 | pending |
| 04 | [App runtime: state, scheduler, CLI, single instance](phase-04-app-runtime.md) | 5h | 03 | pending |
| 05 | [Menu, tray, alerts, autostart](phase-05-menu-tray-notify-autostart.md) | 6h | 04 | pending |
| 06 | [Bubble window and UI](phase-06-bubble-window-ui.md) | 9h | 05 | pending |
| 07 | [Details panel window](phase-07-panel-window.md) | 3h | 06 | pending |
| 08 | [Fullscreen auto-hide](phase-08-fullscreen-auto-hide.md) | 2h | 06 | pending |
| 09 | [Updater](phase-09-updater-migration.md) | 4h | 05 | pending |
| 10 | [Release pipeline and legacy-client compatibility](phase-10-ci-release.md) | 4h | 09 | pending |
| 11 | [Cleanup, docs, parity sign-off](phase-11-cleanup-docs-parity.md) | 3h | 07, 08, 10 | pending |

Run the phases strictly in order. Phases 07, 08 and 09 all edit
`src/platform/windows.rs` and `src/lib.rs`, so they must not run in parallel.
Every phase's Windows manual check runs the exe artifact from the `ci.yml` run
of that phase's last commit.

## Feature-to-phase map

| Feature (subsystem maps) | Phase |
|--------------------------|-------|
| Bubble window styles, topmost, no taskbar, no activation, move cursor | 06 (D5) |
| Stadium shape, size 140..360 step 20, aspect ratios | 02 (math), 06 (UI) |
| Head rings, inner time ring, tail bars, text and fonts, '5H' fallback, placeholders | 02 (strings), 04 (payload), 06 |
| Colours, theme (SystemUsesLightTheme), re-theming of open bubbles | 02, 06 |
| Critical pulse at 95% and above, 60 s time-progress refresh | 06 |
| Drag, click vs drag, snap (corner, edge, taskbar, peer), clamp | 02 (math), 06 |
| Offscreen validation on load, clamp on settings/display change, Codex stagger | 02, 06 |
| Default position, DPI changes, Ctrl+wheel resize around the centre | 02, 06 |
| Hit area | 06 (D4) |
| Fullscreen auto-hide | 08 |
| User show/hide, `user_hidden` and `hidden_by_fullscreen` flags | 04, 06, 08 |
| Panel: toggle, single window, 280x120 layout, bars, colours, theme, placement, focus dismissal, no Alt-Tab, Win11 corners, live refresh | 07 |
| Tray icon per live provider, badge, tooltip, left-click toggle, menu | 05 |
| Context menu (full structure, check and grey states, ids) | 05 |
| Threshold, session-expired and update-applied alerts, 30 min cooldown | 02 (logic), 05 (D9) |
| Poll scheduling, in-flight gate, apply results, credential refresh | 02, 03, 04 |
| Countdown re-render scheduling | 02, 04 |
| Provider toggle and its invariant, OpenCode Go greyed | 02, 05 |
| Reset position, Restart, Exit, CLI flags, `--diagnose`, WebView2 guard | 04, 06 |
| Single instance | 04 (D13) |
| Settings schema, path, atomic save, load normalisation | 02 (D7) |
| Start with Windows (same value name, D10) | 05 |
| Updater: check, status machine, schedule, signature, self-replace, relaunch, startup cleanup, error log | 02, 04 (cleanup), 09 (D8) |
| Usage providers, creds, HTTP, proxy, TLS | 03 (D6) |
| Icons, version info, manifest | 01 |
| CI, release, legacy-client compatibility table, release-process doc, README | 01, 10, 11 |

## Data flow

1. Scheduler thread sends a poll tick to a worker. The worker calls
   `Registry::poll_enabled`, which runs ureq requests against the provider
   endpoints and returns `Vec<(ProviderId, Result)>`.
2. `core::state::apply_results` updates the snapshots, the alerts to fire and the
   auth failures. Auth failures go to the refresh orchestrator, which spawns the
   CLI for every failed provider.
3. `propagate` snapshots state under the lock, drops it (D14), and builds two
   payloads in `core::payload` (pure, unit-tested):
   - `BubblePayload` → `emit_to("bubble-<slug>", "bubble:update")`:
     `{provider, sessionPct|null, weeklyPct|null, sessionPctText, weeklyPctText|null, headLabel, weeklyCountdown, sessionResetsAtMs|null, weeklyResetsAtMs|null, primaryDurSecs, secondaryDurSecs, sessionFill, weeklyFill, accent, isDark, canvasW, canvasH, dpi}`.
     `sessionPctText` is `'{:.0}%'` or `'—'` when there is no value.
     `weeklyPctText` is `null` when there is no weekly value (the text is not
     drawn). `headLabel` is the session countdown only (`format_countdown`), `'…'`
     before the first poll; JS replaces an empty or overflowing label with `'5H'`.
     `weeklyCountdown` is countdown-only, `'…'` before the first poll, `''` hides
     it. On a poll error the bubble keeps its last values and countdowns; **the
     bubble never shows `'!'`**.
   - `PanelPayload` → `emit_to("panel", "panel:update")` only when the panel is
     visible and showing that provider, and returned by `get_panel_state`:
     `{provider, label, isDark, accent, sessionPct, weeklyPct, sessionText, weeklyText, sessionFill, weeklyFill}`.
     The texts are `format_window` output (`'73% · 2h'`), `'!'` after an auth
     failure, `'…'` after a transient error, `''` before the first poll.
   - The tray gets `set_icon`/`set_tooltip`, but only when a cached key changed.
     The tooltip uses the same panel texts, with `'...'` for an empty one.
4. In the other direction, JS calls `invoke` with `bubble_ready`, `bubble_click`,
   `bubble_drag_start`, `bubble_resize_step`, `show_context_menu`,
   `get_bubble_state`, `get_panel_state` and `log_js`. On Windows, the subclass
   hook delivers move end, setting changes and display changes to Rust. On other
   platforms the armed Moved debounce does the same.
5. Persistence: every change runs `Settings::save` (atomic write through tmp and
   rename). Other writes: the update staging file and `update-error.log` under
   `data_local_dir()/ClaudeCodeUsageBubble/updates/`, the AUMID icon PNG next to
   them and the HKCU AppUserModelId key.

## Backwards compatibility and migration

- **Settings:** same file and keys (D7). An old build reading a file written by
  the new one still loads, because there are no new keys.
- **Autostart:** same Run value name, so no migration by default (D10).
- **Updates:** how each released version hands over to the Tauri exe (verified
  with `git show <tag>:src/update/install.rs`):

  | Old version | Who swaps the file | How the Tauri exe is started |
  |-------------|--------------------|------------------------------|
  | v0.1.9 and earlier | `cmd.exe` one-liner (2 s wait, `move`, `start`) | no args |
  | v0.1.10 – v0.3.12 | the old app, in-process rename | `--wait-pid <pid> --updated-to X` |
  | v0.3.13 – v0.4.0 | `updater-helper-<pid>.exe`, a copy of the **old** exe, run as `--apply-update`; leaves `<exe>.old.<pid>` | `--updated-to X` only |
  | v0.4.1 – v0.5.0 | `self_replace` in the old app | `--wait-pid <pid> --updated-to X` |

  No version runs the downloaded binary with `--apply-update`, so the Tauri exe
  drops that handler. It still accepts `--wait-pid`, `--updated-to` and a bare
  start, and its startup cleanup deletes `<exe>.old.*` and
  `updates/updater-helper-*.exe`. Phase 10 checks statically, against the
  updater code at the v0.4.0, v0.4.1 and v0.5.0 tags, that the new release
  assets satisfy each of them. The end-to-end swap is not rehearsed.
- **WebView2:** before the Builder, `cfg(windows)` checks
  `GetAvailableCoreWebView2BrowserVersionString`. If the runtime is missing, a
  `MessageBoxW` shows the Evergreen download link and the process exits 0. An
  auto-updated user on such a machine therefore sees why and how to fix it,
  instead of a silent dead app. The README documents the same requirement.

## Test matrix

| Level | What | Where |
|-------|------|-------|
| Unit (Rust) | Geometry (aspect, height, snap, clamp, default position, `place_near`, `rect_covers_monitor`, style bits), settings load/normalise/validate and legacy JSON, i18n `format_window`/`format_countdown`/`time_until_display_change`/half-to-even, `usage_color` band edges, `apply_results` thresholds and cooldown, payload builders, update schedule, version parsing, badge pixel sampling, provider JSON/header/ISO fixtures, proxy string parsing, menu model, visibility transitions, CLI parsing | `cargo test` on Linux host and CI Windows/Linux |
| Unit (JS) | `ui/layout.js` geometry, pulse triangle and brighten, remaining fraction, `scripts/make-latest-json.js` | `npm test` (`node --test "tests/js/**/*.test.js"`) |
| Static | `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `npm run lint` | host plus CI |
| Build | `npm run tauri build -- --no-bundle` (or `cargo build`) on the host, on `windows-latest` and on `ubuntu-22.04`; `cargo check --all-targets` on `macos-latest`; AppImage and `.deb` bundles only in the `release.yml` Linux x64 job (patchelf on the runner) | host plus CI |
| Static compatibility | The legacy-client table in Phase 10: the asset name, digest and release JSON fields that v0.4.0, v0.4.1 and v0.5.0 read, checked against their tagged code, plus `make-latest-json.js` guards | Phase 10 (written review plus `npm test`) |
| E2E (manual) | Windows parity checklist (Phase 11), Linux X11 smoke run | Windows machine with the CI artifact, Linux x86_64 X11 desktop |

No end-to-end update rehearsal runs. The legacy-to-Tauri swap is first
exercised by real users of the first Tauri release (residual risk, Phase 10).

## Rollback

- Each phase is one or more focused commits on `feat/tauri-port`. Rolling a
  phase back means reverting its commits; `main` is untouched until merge.
- After release, if the Tauri build is broken in the field, run
  `release-legacy.yml` (Phase 10) with a ref holding the v0.5.0 sources and a
  higher version. It builds with plain `cargo build --release`, signs the exe
  with the same key and publishes a Windows-only `latest.json`. Old clients take
  it through the GitHub API, Tauri Windows clients through the plugin, and Tauri
  Linux clients get `TargetNotFound`, which maps to Up to date. The workflow is
  kept because it is one small file. Phase 10 only lints it and checks its build
  and signing in a draft dry run; the rollback swap itself is unproven.

## Success criteria

- [ ] Every row of the Windows parity checklist (Phase 11) passes on Windows 10 and 11.
- [ ] The Phase 10 legacy-client compatibility table is filled in from `git show <tag>:src/update/…` for v0.4.0, v0.4.1 and v0.5.0, every row passes against the dry-run asset list, and the residual risk is recorded.
- [ ] `cargo fmt --check`, `cargo test`, `clippy -D warnings`, `npm run lint`, `npm test` and `npm run tauri build -- --no-bundle` are green on `windows-latest`, `ubuntu-22.04` and this host.
- [ ] The `release.yml` dry-run draft holds exactly `claude-code-usage-bubble.exe`, its `.sig`, one x86_64 AppImage with `.sig`, one x86_64 `.deb` and a `latest.json` with only `windows-x86_64` and `linux-x86_64`.
- [ ] The `macos-latest` `cargo check --all-targets` CI row is green.
- [ ] The Linux x86_64 build runs on an X11 desktop, showing the bubble, data, menu, drag and snap.
- [ ] No `windows` crate use exists outside `src/platform/windows.rs`.

## Open questions

None. Later and undecided work lives in the root `ROADMAP.md`.

User actions (not questions): generate and store the signing key (Phase 09), and
approve the Phase 10 `workflow_dispatch` dry run, which creates (and afterwards
deletes) a draft release in this repo.
