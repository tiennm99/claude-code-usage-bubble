---
phase: 9
title: "Updater"
status: in-progress
priority: P1
effort: "4h"
dependencies: [5]
---

# Phase 09: Updater

## Context

How v0.5.0 updates today:

1. It queries the GitHub API (`src/update/release.rs:48-80`) and takes the
   asset named exactly `usage-monitor.exe` (`release.rs:7`, `:68`),
   or else the first asset ending in `*.exe`.
2. It checks the asset's sha256 digest.
3. It `self_replace`s its own exe, relaunches with `--wait-pid <pid>` and
   `--updated-to <v>`, and exits.

Older versions hand over differently (plan.md, Backwards compatibility table).
None of them runs the downloaded binary with `--apply-update`, so the Tauri exe
has no such handler; the startup cleanup from Phase 04 removes the v0.3.13 to
v0.4.0 helper leftovers.

The decision is D8: the plugin does the signed check and download, then the app
self-replaces on Windows and runs the plugin's install on the AppImage.

## Requirements

- Register `tauri_plugin_updater::Builder::new().build()` in this phase, in the
  same commit that adds `plugins.updater` to `tauri.conf.json`, because the
  plugin's config requires `pubkey` and plugin init fails without it:
  - `pubkey`: the content of the public key generated with `npx tauri signer generate`.
  - `endpoints`: `["https://github.com/tiennm99/usage-monitor/releases/latest/download/latest.json"]`.
- `tauri.conf.json` keeps `bundle.createUpdaterArtifacts: false`; `release.yml`
  enables it through `--config` (Phase 10).
- `src/update/mod.rs` keeps the status machine (`core::state::UpdateStatus`).
  - **Check:** `app.updater_builder()` with
    `.version_comparator(|cur, rel| core::version::newer(rel, cur))`, which
    keeps the suffix-ignoring tuple compare (`release.rs:21-46`, moved to
    `core::version`), `.timeout(Duration::from_secs(30))`, and on Windows
    `.proxy(url)` from `platform::windows::system_proxy()` when no env proxy is
    set. Then `.build()?.check()`. `Some(update)` maps to Available, `None` to
    Up to date, `Err(TargetNotFound)` to Up to date, any other error to Failed.
    Record `last_update_check_unix` on every check, whatever the result, then
    send `UpdateCheckDone` to the scheduler.
  - **Apply:** build a second updater the same way but with
    `.timeout(Duration::from_secs(600))`, call `check()` again, and call
    `download(..)` on that result. The builder timeout is a total-request
    timeout that also covers the download, so the 30 s check timeout must not
    reach it. If the second check returns `None`, set Up to date. Then dispatch
    on the channel:
    - `Portable` (Windows): `ensure_writable` and `reject_unsafe_path` (kept),
      write the bytes to `data_local_dir()/UsageMonitor/updates/update.exe`,
      call `self_replace`, delete the staged file, then
      `handoff::relaunch(&["--updated-to", v])`.
    - `AppImage` (Linux with `$APPIMAGE` set): `update.install(bytes)`, then
      `handoff::relaunch` with the same args.
    - `PackageManager` (Linux without `$APPIMAGE`): apply is disabled.
    - `Unsupported` (macOS for now): apply is disabled.
    - On relaunch `Ok`, `app.exit(0)`. On `Err`, the new binary is already in
      place but not running: set Failed, write `update-error.log`, and keep the
      current process running (the next Restart or login starts the new build).
  - **Failure:** Failed status, plus `write_update_error` to `update-error.log`
    in the existing format, with the plugin errors mapped onto the existing
    `Error` enum.
- Remove the GitHub-API release lookup, the WinHTTP download and the legacy
  `--apply-update` helper (`src/update/install.rs:58-86`, `175-267`). Keep
  `parse_sha256_digest` only if something still uses it; otherwise delete it.
- Capabilities: none. Everything runs in Rust.

## Files

- Create: `src/core/version.rs`
- Modify: `src/update/mod.rs`, `src/update/install.rs` (portable apply and paths only), `src/update/channel.rs` (Portable, AppImage, PackageManager, Unsupported), `tauri.conf.json`, `Cargo.toml` (`tauri-plugin-updater = "2"`), `src/runtime.rs` (plugin registration), `src/lib.rs`
- Delete: `src/update/release.rs`

## Steps

1. Generate the key pair locally. Store the private key, and its password if
   one is used, as the GitHub secrets `TAURI_SIGNING_PRIVATE_KEY` and
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Keep an offline backup. **This is a
   user action:** the agent must not create the secret.
2. Port the version tests: `v1.2.3-rc1` equals 1.2.3, `v1` is 1.0.0, `abc` is
   0.0.0, and there is no downgrade. Add tests for channel detection, the
   staging path and the error mapping (`TargetNotFound` to Up to date).
3. Verify that the plugin's `Update::download` checks the signature, both in
   the docs.rs source and with a tampered `.sig`. If it does not, verify with
   `minisign-verify` against the same public key before calling `self_replace`.
4. Verify in the pinned plugin source that the builder timeout is applied to the
   download request; if it is not, a single builder with the 30 s timeout is
   enough and step "Apply" drops the second check. Record the finding here.

## Todo

- [ ] Step 1, user action: generate the key pair (`npx tauri signer generate`), put the public key in `tauri.conf.json` `plugins.updater.pubkey` (empty until then), store `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` as GitHub secrets and keep an offline backup
- [x] `tauri-plugin-updater = "2"` registered in `src/runtime.rs` in the same commit as `plugins.updater` (`pubkey`, `endpoints`) in `tauri.conf.json`; `createUpdaterArtifacts` stays `false`; no capability added
- [x] `src/core/version.rs`: suffix-ignoring `Version` and `newer`, used as the plugin's `version_comparator`
- [x] Check: 30 s timeout, Windows system proxy unless an env proxy is set (skipped when an update host is in the bypass list), `TargetNotFound`/`TargetsNotFound` map to Up to date, `last_update_check_unix` saved on every check, then `UpdateCheckDone`
- [x] Apply: `Portable` stages `updates/update.exe`, runs `ensure_writable`, `reject_unsafe_path` and `self_replace`, deletes the stage; `AppImage` runs the plugin `install`; both relaunch with `--updated-to X.Y.Z` and exit only on `Ok`; a relaunch failure sets Failed and writes `update-error.log`
- [x] `PackageManager` and `Unsupported` channels: the check runs, "Update available" is greyed (`MenuInput::update_can_apply`)
- [x] Plugin errors mapped onto `update::Error`; apply failures write `update-error.log` in the existing format
- [x] Removed `src/update/release.rs`, the WinHTTP download, the `--apply-update` helper, `parse_sha256_digest` and the `sha2` dependency
- [x] Tests: version (`v1.2.3-rc1` = 1.2.3, `v1` = 1.0.0, `abc` = 0.0.0, no downgrade), channel detection, staging and log paths, stage-and-swap, error mapping and `TargetNotFound` to Up to date, update proxy bypass, plugin config parse
- [x] Step 3: signature check verified in the pinned plugin source and by running its tamper tests (see notes)
- [x] Step 4: builder timeout vs download verified in the pinned plugin source (see notes)
- [x] Host: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (216 passed, 1 ignored), `cargo test -- version:: update::` (18 passed), `npm run lint`, `npm test` (37 passed), `npm run tauri build -- --no-bundle`; CI green on `windows-latest`, `ubuntu-22.04` and `macos-latest`
- [ ] Linux desktop (X11) manual check below

## Implementation notes

- **Step 3 (signature):** in `tauri-plugin-updater` 2.14.0, `Update::download` ends with `self.verify(&buffer)?`, which runs `minisign_verify::PublicKey::verify` against the configured `pubkey` before any bytes are returned, and `Update::install` verifies again. The plugin's own offline tests (`verifies_cached_bytes_and_rejects_tampering`, `install_rejects_bytes_that_do_not_match_the_signature`, plus the signed-version ones) were run on this host against the pinned source and pass: tampered bytes, a tampered signature, an invalid signature and a different key are all rejected. No separate `minisign-verify` call is needed.
- **Step 4 (timeout):** the builder timeout does **not** reach the download. `Updater::update_from_release` builds every `Update` with `timeout: None`, so `download()` would run without any limit. Following the phase, apply drops the second builder and the second `check()`: it downloads the `Update` the last check found, after setting its public `timeout` field to 10 min (`update::DOWNLOAD_TIMEOUT`). The found update is kept in `Runtime::pending_update`, outside the state lock, as v0.5.0 kept its release.
- **Public key:** the agent could not generate the signing key (key generation is the user action in step 1, and it was refused here as credential handling). `plugins.updater.pubkey` is therefore an empty string, which the plugin accepts at registration: checks work, and a download fails signature verification, so nothing unverified is ever installed. `build.rs` refuses to build when `TAURI_SIGNING_PRIVATE_KEY` is set and the effective `pubkey` (file or `--config`) is empty, so a signed release cannot ship without it. `release.yml` also refuses to build, before any build step, while `pubkey` is empty, so an unsigned tag build cannot publish an exe that could never verify an update.
- **Proxy:** the plugin's reqwest reads env proxies itself, so the parsed system proxy is passed only when no env proxy is set. `Proxy::all` cannot bypass per host, so it is skipped when `github.com` or a GitHub release-asset host is in `ProxyOverride`; reqwest's own system-proxy support then routes those requests.
- **Error log:** only apply failures write `update-error.log`, as in v0.5.0; check failures are logged under `--diagnose` only, so a transient network error never overwrites the record of a failed swap.
- **Concurrency:** check and apply claim their status under the state lock (`Idle/UpToDate/Available/Failed` to `Checking`, `Available` to `Applying`), so a scheduled check cannot run alongside an apply. A scheduled check that fires during an apply re-arms the schedule instead of being lost.
- Not enabled: `requireSignedVersion` would also bind the announced version to the signature. It needs every published signature to carry `version:` in its trusted comment, which Phase 10's signing step would have to guarantee.

## Validation

- Host: `cargo test version:: update::` is green.
- Linux desktop (X11, manual): `npm run tauri dev` starts with the updater registered, and a manual
  "Check for updates" against the real endpoint reports Up to date or Failed
  without crashing.
- The handover from v0.4.0, v0.4.1 and v0.5.0 into the Tauri exe is checked
  statically in Phase 10 (legacy-client compatibility table). There is no
  end-to-end rehearsal; the residual risk is recorded there.

## Risks

| Risk | L x I | Mitigation |
|------|-------|------------|
| The signing key is lost, which strands every Tauri install | L x H | Offline backup, documented in `docs/release-process.md`. |
| An old client picks the wrong asset | L x H | No other `*.exe` asset is published. The exact name is required, and the Phase 10 table and `make-latest-json.js` guard check it. |
| The plugin rejects a `latest.json` that lacks a platform | M x M | Phase 10's script emits every entry it builds and fails if one is missing. A deliberately Windows-only legacy release maps `TargetNotFound` to Up to date. |
| A slow link times out the download | M x M | 10 min apply timeout, separate from the 30 s check. |
| GitHub API rate limits still hit old clients | L x L | Unchanged. New clients use the CDN `latest.json`. |

## Rollback

Revert the commits. If this ships broken, use the plan-level rollback
(`release-legacy.yml`).
