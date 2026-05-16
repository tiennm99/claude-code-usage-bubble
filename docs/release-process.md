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
   with `claude-code-usage-bubble.exe` attached and auto-generated
   notes.

## Testing without a real tag

Use the workflow's `workflow_dispatch` input with a throwaway tag
like `v0.0.0-test`. The release is created as a **draft**, so it
does not show up on the public Releases feed or trigger self-updates
for users.

## Versioning

Semver-ish: bump patch for fixes, minor for features, major for
breaking changes (e.g. `settings.json` schema change). The in-app
updater compares `Version { major, minor, patch }` lexicographically,
so a higher tuple wins.

## Asset name contract

The updater matches the release asset by exact filename
`claude-code-usage-bubble.exe` (case-insensitive). Cargo's
`name = "claude-code-usage-bubble"` already produces that name in
`target/release/`, so the workflow uploads the file as-is — do not
rename it.
