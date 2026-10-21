---
phase: 10
title: "Release pipeline and legacy-client compatibility"
status: in-progress
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
  | Endpoint: `GET /repos/tiennm99/claude-code-usage-bubble/releases/latest` (`release.rs:8-9`, `:49`) | same | same | same | The Tauri tag release is non-draft, non-prerelease and marked latest in the same repo | pass (code): re-read at all three tags (`release.rs:8-9`, `:49`); a tag run of `release.yml` publishes without `--draft`/`--prerelease`, with `--latest`, then fails if `gh api …/releases/latest` returns another tag. Draft check: pending |
  | Release JSON fields: `tag_name`, `assets[].name`, `assets[].browser_download_url`, optional `assets[].digest` (`release.rs:104-119`) | same | same | same | Standard GitHub release object; nothing from `latest.json` is read | pass (code): `release.rs:104-119` at all three tags; `gh release create` makes a standard release object |
  | Version gate: `tag_name` parsed with the suffix-ignoring compare, must be greater than the running version (`release.rs:60-63`) | same | same | same | Tag `vX.Y.Z` above `v0.5.0`, equal to `Cargo.toml` (release.yml tag check) | pass (code): `release.rs:60-63` at all three tags; the `prepare` job requires `vX.Y.Z[-suffix]` equal to `Cargo.toml`. The first Tauri tag must be above v0.5.0 (a version bump, not a workflow check) |
  | Asset pick: name equal, ASCII case-insensitive, to `claude-code-usage-bubble.exe`, else the first asset ending in `.exe` (`release.rs:7`, `:65-74`) | same | same | same | Exactly one `*.exe` asset, named `claude-code-usage-bubble.exe`; `.exe.sig`, `.AppImage`, `.AppImage.sig`, `.deb` and `latest.json` do not end in `.exe` (`make-latest-json.js` guard) | pass (code): `release.rs:7`, `:65-74` at all three tags; `make-latest-json.js` rejects any other `.exe` (any case) and any asset outside the exact set, and runs before `gh release create`. Draft check: pending |
  | Checksum: SHA-256 of the downloaded body must equal `digest` `sha256:<hex>` when present (`release.rs:78`, `:86-100`; `install.rs` `download`) | `install.rs:79` | `install.rs:88` | `install.rs:88` | GitHub computes `digest` for every uploaded asset (v0.5.0 shows one); the exe is uploaded unmodified after signing, so the digest matches | pass (code): `release.rs:78`, `:86-98`; `install.rs:95` (v0.4.0) and `:104` (v0.4.1, v0.5.0) compare the body hash. `tauri signer sign` was run on this host with a throwaway key: it writes `<file>.sig` beside the binary and leaves the binary as built |
  | Download: 2xx after redirects from `browser_download_url`, whole body in memory | same | same | same | Same GitHub download URL form as today; the exe grows from about 0.75 MB to several MB, which the in-memory read handles | pass (code): `install.rs:79-108` (v0.4.0) and `:88-117` (v0.4.1, v0.5.0) read the whole body after a 2xx; the URL is GitHub's `browser_download_url` as today |
  | Swap and relaunch | Helper copy of the old exe runs `--apply-update` (`install.rs:44-45`, `:128`), waits for the parent, then starts the new exe with `--updated-to X` only (`install.rs:173`) | `self_replace` in-process, then `--wait-pid <pid> --updated-to X` (`install.rs:155-172`) | same as v0.4.1 (`install.rs:155-172`) | The Tauri exe accepts a bare start, `--updated-to X` alone, and `--wait-pid <pid> --updated-to X` (Phase 04 CLI). It never needs `--apply-update`. The v0.4.0 helper runs `run_cli` before any mutex (`main.rs:33`), and the Tauri lock is a different, per-session lock, so the helper cannot block the new instance | pass (code): v0.4.0 `install.rs:44-45`, `:128-143` (helper args), `:173` (`--updated-to` only), `main.rs:33` (`run_cli` before `app::run`; `main.rs` is identical at all three tags); v0.4.1 and v0.5.0 `install.rs:155-172`. The Tauri `src/cli.rs` `parse` accepts a bare start, `--updated-to X` and `--wait-pid <pid> --updated-to X` and ignores `--apply-update` (`unknown_flags_are_ignored`) |
  | Leftovers | `<exe>.old.<pid>`, `updates/updater-helper-*.exe`, `updates/update.exe` | `updates/update.exe` | same as v0.4.1 | Phase 04 startup cleanup deletes all three patterns | pass (code): v0.4.0 `install.rs:214` (`.old.<pid>`), `:247` (`update.exe`), `:258` (`updater-helper-<pid>.exe`); v0.4.1 and v0.5.0 `install.rs:276` (`update.exe`). `src/handoff.rs` `cleanup_stale_files` (called at startup from `src/runtime.rs`) deletes all three patterns |

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
- Modify: `.github/workflows/release.yml`, `docs/release-process.md` (assets, name contract, residual risk), this phase file (the filled-in table and the residual risk)
- Added during implementation: `.yamllint.yaml` (see notes)

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

## Todo

- [x] `scripts/make-latest-json.js` with `tests/js/make-latest-json.test.js` (two-platform shape, signature content, URLs equal to the renamed names, `--windows-only`, a name with a space, a second `.exe` in any case, a missing `.sig`, an AppImage of another version, an empty signature, bad arguments)
- [x] `release.yml` rewritten: `prepare` (tag form, tag equals `Cargo.toml`, non-empty `pubkey`) gates `win-x64` and `linux-x64`; both refuse an empty `TAURI_SIGNING_PRIVATE_KEY`; `publish` runs the script and `gh release create` (draft on dispatch, `--latest` on tags, then checks `/releases/latest`)
- [x] `release-legacy.yml` added (`ref`, `version`, `tag`, `draft` default `true`)
- [x] `actionlint` and `yamllint` clean on all three workflows (with the new `.yamllint.yaml`)
- [x] Compatibility table re-read from `git show <tag>:src/update/{release,install}.rs` and `src/main.rs` at v0.4.0, v0.4.1 and v0.5.0; every row passes against the code
- [x] Residual risk recorded here and in `docs/release-process.md`
- [x] Host: `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked` (216 passed, 1 ignored), `npm run lint`, `npm test` (52 passed), `npm run tauri build -- --no-bundle -- --locked`; CI green on `windows-latest`, `ubuntu-22.04` and `macos-latest` for the workflow commits
- [ ] Step 3, needs the user: the `release.yml` dispatch dry run and the `release-legacy.yml` draft run, then the draft asset-list check of every table row. Blocked until the signing key exists (Phase 09 step 1), and it creates draft releases, which this run was not allowed to do

## Implementation notes

- **Dry runs not run.** Both workflows refuse to build without the signing secret, and `release.yml` also without `plugins.updater.pubkey`, which is still empty (Phase 09 step 1 is a user action). Creating draft releases was also outside what this run may touch. The table therefore says "pass (code)"; the "Draft check" items stay open until the user runs step 3.
- **Signing flow checked locally.** With a throwaway key (deleted afterwards) and the matching `pubkey` passed through `--config`, `npm run tauri build -- --bundles deb --config '{"bundle":{"createUpdaterArtifacts":true},…}' -- --locked` produced `Claude Code Usage Bubble_<ver>_<arch>.deb` plus a `.deb.sig`, and `tauri signer sign --app-version` wrote `<binary>.sig` beside an untouched binary. The bundler also signs the deb, so the rename step copies only the AppImage, its `.sig` and the deb; the deb signature is not published (no updater entry uses it). Bundle names carry spaces, as the phase expected.
- **Runner args.** `npm run tauri build -- <opts> -- --locked` hands `--locked` to cargo (checked with an invalid flag, which cargo rejected), so release builds stay locked like the old `cargo build --release --locked`.
- **`--app-version`.** Both signing steps bind the version into the signature's trusted comment, as `tauri build` does for the AppImage, so `requireSignedVersion` can be turned on later without re-signing anything.
- **Legacy version patch.** `release-legacy.yml` patches the first `version =` line of `Cargo.toml`, then runs `cargo update --workspace`, which rewrites only the package's own entry in `Cargo.lock`. Checked on this host against an archive of v0.5.0: without it `--locked` fails, with it the lock diff is the single version line. The release is created with `--target` set to the built commit, so the tag lands on the legacy sources, not the default branch.
- **`make-latest-json.js` scope.** Besides the listed guards it requires the exact asset set (any unexpected file fails), so a stray upload cannot slip into a release. It writes `<dir>/latest.json` and ignores an existing one, so a re-run is safe.
- **yamllint.** The repo had no yamllint config, and the default rules reject every GitHub workflow (`on:` as a truthy key, no `---`, 80-column lines), `ci.yml` included. `.yamllint.yaml` relaxes exactly those three rules (line length 140), so the check is reproducible.
- **Draft dry-run tag.** The tag check also runs for dispatch, so `v0.0.0-test` must point at a commit whose `Cargo.toml` says `0.0.0-test`; `docs/release-process.md` says so.
- **Suffixed tags never publish.** The push trigger matches only plain `vX.Y.Z`, and `prepare` also fails a non-dispatch run on a suffixed tag. Before this, pushing `v0.0.0-test` (or an rc tag) started a run that published it with `--latest`, and both client comparators ignore the suffix.

## Residual risk

The legacy-to-Tauri swap is never run end to end before release. Static
reading cannot prove runtime behaviour such as antivirus or SmartScreen
reactions to a self-replaced, larger, unsigned exe, WebView2 start-up from a
detached relaunch, or the v0.4.0 helper timing. The first Tauri release is the
first real run. Mitigations: the WebView2 guard (Phase 04), `--diagnose`
logging, `update-error.log`, and the `release-legacy.yml` rollback, which is
itself unproven end to end. The draft dry runs (step 3) are also still
outstanding, so the asset list has been checked against the workflow and the
script, not against a real draft.

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
| A draft dry run reaches users | L x H | `/releases/latest` skips drafts; the dry-run tag carries a suffix, which the push trigger and `prepare` refuse to publish; both drafts are deleted after inspection. |

## Rollback

Revert the workflow commits. Already published releases are unaffected.
