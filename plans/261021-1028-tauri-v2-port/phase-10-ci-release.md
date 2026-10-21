---
phase: 10
title: "Release pipeline and legacy-client compatibility"
status: pending
priority: P1
effort: "4h"
dependencies: [9]
---

# Phase 10: Release Pipeline And Legacy-Client Compatibility

## Context

`.github/workflows/release.yml` builds one Windows exe on tag pushes, with a
draft release on `workflow_dispatch`. `ci.yml` already exists from Phase 01.

v0.4.0 to v0.5.0 read `api.github.com/repos/tiennm99/claude-code-usage-bubble/releases/latest`,
which skips drafts and prereleases, and draft assets are not downloadable
without auth. No draft can exercise the migration, and the user decided against
an end-to-end rehearsal and a throwaway repo. This phase instead checks
statically, against the updater code at each released tag, that the new release
assets satisfy every legacy client, and records the residual risk.

Linux ships x86_64 only (user decision). Bundles are built only in CI; this
host has no patchelf and no display.

## Requirements

- `release.yml`:
  - Triggers: tag `v*.*.*`, or `workflow_dispatch` with a `tag` input (a draft release). Keep the tag-equals-`Cargo.toml`-version check.
  - Every build job runs `npm ci` and passes
    `--config '{"bundle":{"createUpdaterArtifacts":true}}'` where bundles are
    built, with `TAURI_SIGNING_PRIVATE_KEY` and `_PASSWORD` in the environment.
    A signing step fails hard when the key variable is empty.
  - Keep the step that fails before any build when `plugins.updater.pubkey` in
    `tauri.conf.json` is empty. `build.rs` only checks the key when
    `TAURI_SIGNING_PRIVATE_KEY` is in the build environment, so it does not
    cover a build step that runs without it.
  - **win-x64** job (`windows-latest`):
    - `npm run tauri build -- --no-bundle`.
    - `npx tauri signer sign target/release/claude-code-usage-bubble.exe`. This
      writes a separate `.sig` and leaves the exe bytes untouched, so the GitHub
      asset digest that old clients verify is the digest of the exe they run.
    - Upload the exe and its `.sig`.
  - **linux-x64** job (`ubuntu-22.04`):
    - Install the Phase 01 apt list plus `patchelf`.
    - `npm run tauri build -- --bundles appimage,deb --config …`.
    - Rename every output to space-free kebab names before upload, because the
      bundler names them after `productName` and GitHub rewrites spaces in asset
      names: `claude-code-usage-bubble_<ver>_amd64.AppImage`, the matching
      `.AppImage.sig`, and `claude-code-usage-bubble_<ver>_amd64.deb`. The
      signature covers the content, not the name.
  - **publish** job:
    - Download the artifacts.
    - Run `node scripts/make-latest-json.js <tag> <dir>`. It builds the URLs
      from the renamed files and writes
      `{version, notes, pub_date, platforms:{"windows-x86_64", "linux-x86_64"}}`,
      where each entry is a release-download URL plus the `.sig` content. It
      fails on any missing entry, on any `*.exe` asset other than
      `claude-code-usage-bubble.exe`, and on any asset name with a space (a
      guard that runs after the rename).
    - Then `gh release create <tag> <assets…> latest.json --generate-notes`,
      adding `--draft` for a dispatch run. A tag run is non-draft,
      non-prerelease and `--latest`, because legacy clients only read
      `/releases/latest`.
- `release-legacy.yml` (kept because it is one small file and the only
  rollback for Tauri clients; `workflow_dispatch` with `ref`, `version`, `tag`
  and a `draft` input that defaults to `true`): checks out `ref`, patches the
  `Cargo.toml` version, runs `cargo build --release --locked`, signs the exe
  with `tauri signer sign` (CLI from `npx --yes @tauri-apps/cli@2`, since the
  legacy ref has no `package.json`), runs `make-latest-json.js --windows-only`
  (from the current default branch) and publishes `claude-code-usage-bubble.exe`,
  its `.sig` and the Windows-only `latest.json`. Tauri Linux clients get
  `TargetNotFound`, which Phase 09 maps to Up to date. The v0.5.0 sources accept
  the Tauri relaunch args `--wait-pid <pid> --updated-to X`
  (`git show v0.5.0:src/main.rs`, lines 42 and 54).
- `scripts/make-latest-json.js`: plain JS with JSDoc, linted, with
  `tests/js/make-latest-json.test.js` covering the shape (exactly the two
  platforms), a space-containing input that fails, a second `*.exe` that fails,
  renamed names that match their URLs, and `--windows-only`.
- **Legacy-client compatibility table**, filled in by reading the updater code
  at each released tag (`git show <tag>:src/update/release.rs`,
  `git show <tag>:src/update/install.rs`, `git show <tag>:src/main.rs`) and
  checking every row against the dry-run draft. The rows below were verified at
  planning time; the implementer re-runs the `git show` reads and marks each row
  pass or fail. `src/update/release.rs` is byte-identical from v0.4.0 to v0.5.0
  (`git diff --stat v0.4.0 v0.5.0 -- src/update/release.rs` is empty).

  | What the old client reads or needs | v0.4.0 | v0.4.1 | v0.5.0 | New release provides | Result |
  |------------------------------------|--------|--------|--------|----------------------|--------|
  | Endpoint: `GET /repos/tiennm99/claude-code-usage-bubble/releases/latest` (`release.rs:8-9`, `:49`) | same | same | same | The Tauri tag release is non-draft, non-prerelease and marked latest in the same repo | to check |
  | Release JSON fields: `tag_name`, `assets[].name`, `assets[].browser_download_url`, optional `assets[].digest` (`release.rs:104-119`) | same | same | same | Standard GitHub release object; nothing from `latest.json` is read | to check |
  | Version gate: `tag_name` parsed with the suffix-ignoring compare, must be greater than the running version (`release.rs:60-63`) | same | same | same | Tag `vX.Y.Z` above `v0.5.0`, equal to `Cargo.toml` (release.yml tag check) | to check |
  | Asset pick: name equal, ASCII case-insensitive, to `claude-code-usage-bubble.exe`, else the first asset ending in `.exe` (`release.rs:7`, `:65-74`) | same | same | same | Exactly one `*.exe` asset, named `claude-code-usage-bubble.exe`; `.exe.sig`, `.AppImage`, `.AppImage.sig`, `.deb` and `latest.json` do not end in `.exe` (`make-latest-json.js` guard) | to check |
  | Checksum: SHA-256 of the downloaded body must equal `digest` `sha256:<hex>` when present (`release.rs:78`, `:86-100`; `install.rs` `download`) | `install.rs:79` | `install.rs:88` | `install.rs:88` | GitHub computes `digest` for every uploaded asset (v0.5.0 shows one); the exe is uploaded unmodified after signing, so the digest matches | to check |
  | Download: 2xx after redirects from `browser_download_url`, whole body in memory | same | same | same | Same GitHub download URL form as today; the exe grows from about 0.75 MB to several MB, which the in-memory read handles | to check |
  | Swap and relaunch | Helper copy of the old exe runs `--apply-update` (`install.rs:44-45`, `:128`), waits for the parent, then starts the new exe with `--updated-to X` only (`install.rs:173`) | `self_replace` in-process, then `--wait-pid <pid> --updated-to X` (`install.rs:155-172`) | same as v0.4.1 (`install.rs:155-172`) | The Tauri exe accepts a bare start, `--updated-to X` alone, and `--wait-pid <pid> --updated-to X` (Phase 04 CLI). It never needs `--apply-update`. The v0.4.0 helper runs `run_cli` before any mutex (`main.rs:33`), and the Tauri lock is a different, per-session lock, so the helper cannot block the new instance | to check |
  | Leftovers | `<exe>.old.<pid>`, `updates/updater-helper-*.exe`, `updates/update.exe` | `updates/update.exe` | same as v0.4.1 | Phase 04 startup cleanup deletes all three patterns | to check |

  Versions before v0.4.0 use the same `ASSET_NAME` constant (checked at
  v0.1.0, v0.1.9, v0.1.10, v0.2.0, v0.3.0, v0.3.12 and v0.3.13) and the
  hand-over modes in the plan.md backwards-compatibility table; they are not
  re-checked row by row.

- **Residual risk** (record it in this file and in `docs/release-process.md`):
  the legacy-to-Tauri swap is never run end to end before release. Static
  reading cannot prove runtime behaviour such as antivirus or SmartScreen
  reactions to a self-replaced, larger, unsigned exe, WebView2 start-up from a
  detached relaunch, or the v0.4.0 helper timing. The first Tauri release is the
  first real run. Mitigations: the WebView2 guard (Phase 04), `--diagnose`
  logging, `update-error.log`, and the `release-legacy.yml` rollback, which is
  itself unproven end to end.

## Files

- Create: `.github/workflows/release-legacy.yml`, `scripts/make-latest-json.js`, `tests/js/make-latest-json.test.js`
- Modify: `.github/workflows/release.yml`, this phase file (the filled-in table and the residual risk)

## Steps

1. Write the `latest.json` script and its test.
2. Rewrite `release.yml` and add `release-legacy.yml`. Run `actionlint` and `yamllint`.
3. With the user's OK (it creates a draft release in this repo), run a
   `workflow_dispatch` dry run of `release.yml` with a throwaway tag
   (`v0.0.0-test`) and inspect the draft's asset list. Run `release-legacy.yml`
   once with `draft: true` and the v0.5.0 sources to check that it builds,
   signs and writes a Windows-only `latest.json`. Delete both drafts afterwards.
4. Re-run the `git show` reads, fill in the Result column of the compatibility
   table against the dry-run asset list, and write the residual-risk note.

## Validation

- `actionlint` and `yamllint` are clean on all workflows.
- `npm test` covers `make-latest-json.js`, including the two-platform shape and the second-`*.exe` failure.
- The dry-run draft contains exactly `claude-code-usage-bubble.exe`, its `.exe.sig`, one renamed `_amd64.AppImage` with its `.sig`, one renamed `_amd64.deb` and a `latest.json` with only `windows-x86_64` and `linux-x86_64`, whose URLs match the asset names exactly.
- The `release-legacy.yml` draft contains the exe, its `.sig` and a Windows-only `latest.json`.
- Every row of the compatibility table reads pass, with the tag and `file:line` it was checked against, and the residual risk is written down.

## Risks

| Risk | L x I | Mitigation |
|------|-------|------------|
| The legacy-to-Tauri swap fails in the field (unproven end to end) | L x H | The static table above; WebView2 guard; `update-error.log`; `release-legacy.yml` rollback. Accepted by the user in place of a rehearsal. |
| A missing secret silently produces an unsigned build | L x H | The signing step fails hard when the env var is empty. |
| An old client picks the wrong asset | L x H | Exactly one `*.exe` asset, enforced by `make-latest-json.js`. |
| The Tauri release is not the one `/releases/latest` returns | L x H | Tag runs publish non-draft, non-prerelease, `--latest`; check with `gh api repos/tiennm99/claude-code-usage-bubble/releases/latest -q .tag_name` after publishing. |
| A draft dry run reaches users | L x H | `/releases/latest` skips drafts; both drafts are deleted after inspection. |

## Rollback

Revert the workflow commits. Already published releases are unaffected.
