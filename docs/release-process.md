# Cutting a release

The `release.yml` workflow builds and publishes on every pushed tag
matching `v*.*.*`. The workflow asserts that the pushed tag matches
`Cargo.toml` `version` and **fails fast on mismatch**, so the order
below matters: bump Cargo *before* you tag.

## Steps for a new version

1. Bump `Cargo.toml` `version` (`X.Y.Z`).
2. `cargo build --release` locally to refresh `Cargo.lock`.
3. Commit: `chore: bump version to X.Y.Z`.
4. Tag and push:

   ```powershell
   git tag -a vX.Y.Z -m "vX.Y.Z"
   git push origin main
   git push origin vX.Y.Z
   ```

5. Watch the "Release" workflow run; it creates the GitHub Release
   with auto-generated notes and these assets:
   `claude-code-usage-bubble.exe` and its `.sig`,
   `claude-code-usage-bubble_X.Y.Z_amd64.AppImage` and its `.sig`,
   `claude-code-usage-bubble_X.Y.Z_amd64.deb`, and `latest.json`.
   The build jobs need the `TAURI_SIGNING_PRIVATE_KEY` and
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` secrets and a non-empty
   `plugins.updater.pubkey` in `tauri.conf.json`; the workflow fails
   before building without them.

## Testing without a real tag

Use the workflow's `workflow_dispatch` input with a throwaway tag
like `v0.0.0-test`. The release is created as a **draft**, so it
does not show up on the public Releases feed or trigger self-updates
for users.
The tag must already exist and point at a commit whose `Cargo.toml`
version matches it (`0.0.0-test` here), because the version check also
runs for drafts. Delete the draft and the tag after inspecting it.

## Versioning

Semver-ish: bump patch for fixes, minor for features, major for
breaking changes (e.g. `settings.json` schema change). The in-app
updater compares `Version { major, minor, patch }` lexicographically,
so a higher tuple wins.

## Asset name contract

Legacy clients (v0.5.0 and earlier) read `/releases/latest` through
the GitHub API and download the asset named
`claude-code-usage-bubble.exe` (case-insensitive), else the first asset
ending in `.exe`. The release must therefore carry exactly one `.exe`
asset, under that name, uploaded unmodified so the GitHub `digest`
they verify matches. Tauri clients read `latest.json`, whose URLs must
match the asset names exactly, so the Linux bundles are renamed to
space-free names before upload. `scripts/make-latest-json.js` enforces
both rules and fails the release otherwise.

## Residual risk

The hand-over from a legacy Win32 release to the Tauri exe has been
checked only by reading the updater code at v0.4.0, v0.4.1 and v0.5.0;
it has never run end to end before a release. Static reading cannot
prove runtime behaviour such as antivirus or SmartScreen reactions to
a self-replaced, larger, unsigned exe, WebView2 start-up from a
detached relaunch, or the v0.4.0 helper timing. The first Tauri
release is the first real run. Mitigations: the WebView2 guard, the
`--diagnose` log, `update-error.log`, and the `release-legacy.yml`
rollback, which rebuilds the Win32 sources at a given ref under a
higher version as a Windows-only release. That rollback is itself
unproven end to end.
