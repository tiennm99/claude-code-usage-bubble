---
phase: 1
title: "Scaffold Tauri crate, JS toolchain and CI"
status: in-progress
priority: P1
effort: "5h"
dependencies: []
---

# Phase 01: Scaffold Tauri Crate, JS Toolchain And CI

## Context

The crate today is Win32-only: `windows` 0.58, embed-resource and `res/icon.rc`
(`Cargo.toml`, `build.rs:1-16`). It does not compile on Linux. This phase gives
the package a Tauri skeleton (lib plus thin bin, D1) that compiles and runs on
Linux and Windows, plus the CI that produces the Windows exe every later phase
tests by hand. It detaches the Win32 modules from the module tree; their files
stay on disk until the phase that replaces each one deletes it.

## Requirements

- Single root package (D1). `[lib] name = "usage_monitor_lib"`,
  `path = "src/lib.rs"`; `[[bin]] name = "usage-monitor"`.
  `src/main.rs` only holds
  `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` and a call
  to `usage_monitor_lib::run()`.
- `tauri.conf.json` at the root: `build.frontendDist: "ui"`,
  `app.withGlobalTauri: true`, no `devUrl`, `app.windows: []`.
- `productName`, `mainBinaryName` and `identifier` as in D16. No `version` key.
- `app.security.csp`:
  `default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src ipc: http://ipc.localhost`.
- `app.macOSPrivateApi: true` **and** the `macos-private-api` feature on the
  `tauri` dependency. `tauri-build` checks that the config and the crate features
  match and fails `cargo check` otherwise; the feature is a no-op off macOS and
  keeps transparency for the later macOS port.
- `bundle`: `active: true`, `targets: ["appimage", "deb"]` (used only by the
  `release.yml` Linux x64 job; Windows and every other build use `--no-bundle`,
  D8), **`createUpdaterArtifacts: false`** (release turns it on
  through `--config`, D8), `copyright` "Copyright (C) 2026 tiennm99",
  `publisher` "tiennm99", `icon` set to the generated list. No `plugins.updater`
  section yet (Phase 09).
- `capabilities/default.json`: `windows: ["bubble-*", "panel"]`, permissions
  `core:default` and `core:window:allow-start-dragging`. Plugins are called from
  Rust only, so no plugin JS permissions.
- Cargo deps on major tags (per `version-pinning.md`):
  - `tauri = { version = "2", features = ["tray-icon", "macos-private-api"] }`, `tauri-build = "2"`.
  - `tauri-plugin-single-instance = "2"` and `tauri-plugin-autostart = "2"` now.
    `tauri-plugin-notification` is added in Phase 05 and `tauri-plugin-updater`
    in Phase 09.
  - `ureq = "3"`, plus the existing serde, serde_json, dirs, log, simplelog,
    thiserror, tiny-skia, sha2 and self-replace.
  - `[target.'cfg(unix)'.dependencies] libc = "0.2"` (the `kill(pid, 0)` wait).
  - `[target.'cfg(target_os = "linux")'.dependencies] x11rb = "0.13"` and
    `libloading` (major tag; the tray library probe in Phase 05).
  - `[target.'cfg(windows)'.dependencies] windows`: the major must match the one
    tauri already pulls (`cargo tree -i windows`). Features, the full set later
    phases need: `Win32_Foundation`, `Win32_System_Registry`,
    `Win32_System_Threading`, `Win32_UI_WindowsAndMessaging`, `Win32_UI_Shell`
    (SHAppBarMessage, SetWindowSubclass, AUMID), `Win32_Graphics_Dwm`,
    `Win32_Graphics_Gdi` (MonitorFromWindow/GetMonitorInfoW). A later phase that
    needs one more feature appends it and re-runs `cargo tree -i windows`.
  - `webview2-com` (the major tauri pulls) for the WebView2 runtime check in
    Phase 04, only if tauri does not already re-export it.
- Keep the release profile (`Cargo.toml:43-48`).
- `package.json`: `"private": true`, `"type": "module"`, scripts `tauri`,
  `lint` (`eslint .`) and `test` (`node --test "tests/js/**/*.test.js"`).
  A glob that matches nothing exits 0 on Node 24 (checked on this host); a bare
  directory argument (`node --test ui/`) fails with "Cannot find module".
  devDependencies, installed with `@latest` so they take the current caret
  major: `@tauri-apps/cli`, `eslint`, `@eslint/js`, `eslint-plugin-jsdoc`,
  `globals`.
- `eslint.config.js`: `js.configs.recommended` and
  `jsdoc.configs['flat/recommended-error']` everywhere; `ui/**/*.js` gets
  `globals.browser` plus a readonly `__TAURI__`; `tests/**`, `scripts/**` and
  `eslint.config.js` get `globals.node`; `target/`, `gen/` and `node_modules/`
  are ignored.
- `.taurignore` (D1): `ui/`, `tests/`, `scripts/`, `plans/`, `docs/`,
  `node_modules/`.
- `.github/workflows/ci.yml` (D17):
  - Triggers: pushes to `feat/tauri-port` and `main`, and `pull_request`.
  - Rows `windows-latest` and `ubuntu-22.04`: checkout,
    `dtolnay/rust-toolchain@stable` with clippy and rustfmt,
    `Swatinem/rust-cache@v2`, `actions/setup-node@v4` (node `lts/*`, npm cache).
    Linux apt list, the official Tauri v2 prerequisites:
    `libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev`.
    Then `npm ci`, `cargo fmt --check`,
    `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`,
    `npm run lint`, `npm test`, `npm run tauri build -- --no-bundle`.
  - The Linux row also runs `npm run tauri build -- --bundles deb` (no
    patchelf needed), so a broken `bundle.icon` list fails before release.
  - The Windows row uploads `target/release/usage-monitor.exe` with
    `actions/upload-artifact` (name `bubble-exe-<sha>`, `retention-days: 7`).
  - Row `macos-latest`: toolchain, cache, `cargo check --all-targets --locked`
    only (the macOS compile gate; `ui/` must exist, nothing else is needed).

## Files

- Modify: `Cargo.toml`, `build.rs` (becomes `tauri_build::build()`), `src/main.rs`, `.gitignore` (add `/node_modules`, `/gen`)
- Create: `src/lib.rs`, `tauri.conf.json`, `.taurignore`, `capabilities/default.json`, `package.json`, `package-lock.json`, `eslint.config.js`, `ui/bubble.html` (placeholder), `tests/js/.gitkeep`, `.github/workflows/ci.yml`, `icons/` (output of `npx tauri icon src/icons/256.svg`, with `icons/icon.ico` then overwritten by `src/icons/icon.ico` to keep the hand-tuned small sizes)
- Delete: `res/icon.rc`

## Steps

1. Run `npm init`, then `npm i -D @tauri-apps/cli@latest eslint@latest @eslint/js@latest eslint-plugin-jsdoc@latest globals@latest`.
2. Write `tauri.conf.json`, the capabilities file and `.taurignore`.
3. Create `src/lib.rs` with `pub mod diag;` and `run()`: diag init plus a `tauri::Builder` that opens one transparent test window loading `bubble.html` and handles `RunEvent::ExitRequested` as in D18. Reduce `src/main.rs` to the shim. Remove every `mod` line for Win32 modules (`app`, `bubble`, `panel`, `tray`, `os`, `net`, `update`, `usage`, `creds`, `settings`, `i18n`, `usage_color`). Later phases re-add modules to `src/lib.rs` as they port them.
4. Generate icons. Verify that `tauri-build` embeds the icon, a VERSIONINFO built from the Cargo version, and a PerMonitorV2 manifest.
5. Run `npx tauri info` to confirm the CLI resolves a root-level `tauri.conf.json`.
6. Add `ci.yml`, run `actionlint` and `yamllint`, push to `feat/tauri-port` and make all three rows green.

## Todo

- [x] npm toolchain: `package.json`, `package-lock.json`, `eslint.config.js`, `tests/js/.gitkeep`
- [x] `tauri.conf.json`, `capabilities/default.json`, `.taurignore`, `.gitignore`
- [x] `src/lib.rs` (`diag`, `run()`, test window, D18 exit handling) and the `src/main.rs` shim; Win32 modules out of the module tree
- [x] Icons generated, `icons/icon.ico` replaced by `src/icons/icon.ico`; `res/icon.rc` deleted
- [x] `tauri-build` embeds the icon, VERSIONINFO from the Cargo version and a PerMonitorV2 manifest
- [x] `npx tauri info` resolves the root-level `tauri.conf.json`
- [x] `ci.yml` passes `actionlint` and `yamllint`; all three rows green with a `bubble-exe-<sha>` artifact
- [x] Host: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `npm run lint`, `npm test`, `npm run tauri build -- --no-bundle`
- [ ] Windows manual check: the CI exe starts and shows the test window (needs a Windows machine)

## Implementation notes

- `windows` is `0.62` and `webview2-com` is `0.39`, the majors tauri 2.12 pulls
  (`cargo tree -i windows` shows a single copy). `webview2-com` is a direct
  dependency because tauri does not re-export it.
- `x11rb` is `0.14` and `libloading` is `0.9`, the current releases.
- Actions use their current majors: `actions/checkout@v7`,
  `actions/setup-node@v7`, `actions/upload-artifact@v7`.
- `tauri-build` writes VERSIONINFO only from the config `version`, which D16
  leaves out. `build.rs` merges the Cargo version into `TAURI_CONFIG` before
  `try_build`, keeping any CLI `--config` override. The Windows CI row asserts
  that the exe `ProductVersion` equals the `Cargo.toml` version.
- The default tauri manifest has no DPI awareness, so
  `res/windows-app-manifest.xml` keeps its Common-Controls v6 dependency and
  adds `PerMonitorV2`.
- Only desktop icons are kept: the mobile (`android/`, `ios/`) and Windows
  Store (`Square*Logo.png`, `StoreLogo.png`) outputs of `tauri icon` are unused.
- `bundle.icon` lists every desktop icon. The bundler has no fallback (an
  empty list panics the AppImage step and ships a deb without hicolor icons).
  `icons/icon.png` comes first because tauri-codegen takes the first `.png` as
  the Linux and macOS default window icon.
- The test window (`bubble-test`) exits the app on close, since D18 keeps the
  process alive after its last window is destroyed.

## Validation

- Host: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `npm run lint` and `npm test` (zero tests, exit 0) all pass.
- Host: `npm run tauri build -- --no-bundle` builds the binary with no signing key in the environment. `npm run tauri build -- --bundles deb` builds a deb with 32, 128, 256 and 512 px hicolor icons. The AppImage needs patchelf, so it is first exercised by the Phase 10 dry run.
- CI: all three rows green; the Windows run has a downloadable `bubble-exe-<sha>` artifact that starts and shows the test window.

## Risks

| Risk | L x I | Mitigation |
|------|-------|------------|
| The CLI or `tauri-build` assumes `src-tauri/` | L x M | `tauri info` in step 5. Fallback: `TAURI_CONFIG`/`--config` paths. Last resort is a `src-tauri/` move, recorded as a change to D1. |
| Config/feature mismatch fails `build.rs` | M x H | `macOSPrivateApi` and the `macos-private-api` feature are set together; `cargo check` in Validation catches any other allowlist mismatch. |
| Two `windows` crate versions get compiled | M x L | Align the major with `cargo tree -i windows`. |
| The `macos-latest` check fails on a build script | L x M | It runs on real macOS, so no cross toolchain is needed. A failure there is a real macOS compile bug to fix behind `cfg`. |
| The `gen/schemas` dir is regenerated on every build | L x L | Gitignore it. Capabilities point `$schema` at it only for editor help. |

## Rollback

Revert the phase commits. The Win32 sources are untouched on disk.
