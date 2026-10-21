# Cutting a release

The `release.yml` workflow builds and publishes on every pushed plain
`vX.Y.Z` tag. Pushing a tag with a suffix (`vX.Y.Z-rc1`) publishes
nothing: the updaters ignore suffixes, so an rc would reach every user
as a stable release. The workflow asserts that the pushed tag matches
`Cargo.toml` `version` and **fails fast on mismatch**, so the order
below matters: bump Cargo *before* you tag.

## Version source

The version lives in `Cargo.toml` only. `tauri.conf.json` has no
`version` field, so the app, the Windows file properties (written by
`build.rs`) and the bundles all inherit the Cargo version. Never add a
`version` to `tauri.conf.json`.

## Signing key and secrets

Every release is signed with one minisign key pair, the updater key.
Installed copies verify each update against the public key they were
built with, so losing the private key means no installed copy can ever
update again.

- The public key is `plugins.updater.pubkey` in `tauri.conf.json`.
  Both release workflows refuse to build while it is empty.
- The private key and its password are the repository secrets
  `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
  Only the workflow steps that check or sign see them; dependency
  installs and compilation run without them. A signing step fails hard
  when the key is empty, so no unsigned build is published.
- Keep an **offline backup** of the private key file and its password
  (for example an encrypted copy on removable media and the password in
  a password manager). GitHub secrets cannot be read back, so the
  backup is the only copy that survives a lost or rotated secret.
- Generate the pair once with `npx tauri signer generate`. Rotating it
  strands every installed copy on the old key, so do it only if the
  private key leaks.
- `build.rs` refuses to build when `TAURI_SIGNING_PRIVATE_KEY` is set
  and the effective `pubkey` is empty, so a local build with the key in
  the environment cannot produce a release that never verifies updates.

## Steps for a new version

1. Bump `Cargo.toml` `version` (`X.Y.Z`).
2. Run `cargo build` locally to refresh `Cargo.lock`.
3. Commit: `chore: bump version to X.Y.Z`.
4. Tag and push:

   ```powershell
   git tag -a vX.Y.Z -m "vX.Y.Z"
   git push origin main
   git push origin vX.Y.Z
   ```

5. Watch the "Release" workflow run. It creates a non-draft,
   non-prerelease GitHub Release marked latest, with auto-generated
   notes and the assets listed below, then fails if
   `/releases/latest` does not return the new tag.

## Release assets

A release carries exactly these files and nothing else:

| Asset | Built by | Read by |
|-------|----------|---------|
| `claude-code-usage-bubble.exe` | Windows job, `npm run tauri build -- --no-bundle` | Legacy Win32 clients (GitHub API) and Tauri Windows clients (`latest.json`) |
| `claude-code-usage-bubble.exe.sig` | `tauri signer sign` on the exe | Tauri Windows clients |
| `claude-code-usage-bubble_X.Y.Z_amd64.AppImage` | Linux job, `tauri bundle` | Tauri AppImage clients |
| `claude-code-usage-bubble_X.Y.Z_amd64.AppImage.sig` | the bundler, with the signing key | Tauri AppImage clients |
| `claude-code-usage-bubble_X.Y.Z_amd64.deb` | Linux job, `tauri bundle` | Users installing by hand (`.deb` installs check for updates but do not apply them) |
| `latest.json` | `scripts/make-latest-json.js` in the publish job | Tauri clients, through the updater endpoint in `tauri.conf.json` |

`latest.json` holds `version`, `notes`, `pub_date` and exactly two
platforms, `windows-x86_64` and `linux-x86_64`, each with the asset's
release-download URL and the content of its `.sig`. There is no
Linux ARM64 or macOS entry.

### Rename rule

The bundler names the Linux outputs after `productName`
("Claude Code Usage Bubble"), and GitHub rewrites spaces in asset
names, which would break the `latest.json` URLs. The Linux job
therefore copies the bundles to space-free kebab names before upload:
`claude-code-usage-bubble_<version>_amd64.AppImage`, its `.sig`, and
`claude-code-usage-bubble_<version>_amd64.deb`. The signature covers
the file content, not its name, so renaming keeps it valid. The
bundler also signs the deb, but that signature is not published,
because no updater entry uses it.

### Legacy-client asset contract

Clients from v0.5.0 and earlier read `/releases/latest` through the
GitHub API, which skips drafts and prereleases. Checked against their
updater code at v0.4.0, v0.4.1 and v0.5.0, they need:

- A non-draft, non-prerelease release marked latest in this repo.
- A `tag_name` of plain `vX.Y.Z`, higher than the running version
  (suffixes are ignored when comparing). The first Tauri release must
  therefore be above v0.5.0.
- An asset named `claude-code-usage-bubble.exe` (matched
  case-insensitively), else the first asset ending in `.exe`. The
  release must carry exactly one `.exe` asset, under that name. The
  `.exe.sig`, AppImage, deb and `latest.json` do not end in `.exe`.
- The exe uploaded unmodified, so the `digest` (`sha256:<hex>`) that
  GitHub computes matches the downloaded body they hash. `tauri signer
  sign` writes a separate `.sig` and leaves the exe bytes untouched.
- A new exe that starts from any of their hand-over modes: v0.4.0 runs
  it with `--updated-to X` only, v0.4.1 and v0.5.0 with
  `--wait-pid <pid> --updated-to X`, and v0.1.9 and earlier with no
  arguments. The Tauri exe accepts all three, and its startup cleanup
  deletes the `<exe>.old.<pid>`, `updates/update.exe` and
  `updates/updater-helper-*.exe` files they leave behind.

`scripts/make-latest-json.js` enforces the asset rules before
`gh release create` runs: it fails on a missing platform entry or
signature, on any `.exe` other than `claude-code-usage-bubble.exe`
(in any case), on any asset name with a space, and on any file outside
the exact set above. Tauri clients read only `latest.json`, whose URLs
must equal the asset names exactly.

## Testing without a real tag (dry run)

A dry run creates a **draft** release, which does not show up on the
public Releases feed and does not reach any client, legacy or Tauri.

1. Pick a throwaway tag with a suffix, like `v0.0.0-test`. The suffix
   keeps the tag push from starting a publishing run; never use a plain
   `vX.Y.Z` tag for a dry run, because pushing it publishes a release
   marked latest.
2. On a scratch branch, set `Cargo.toml` `version` to `0.0.0-test`, run
   `cargo build` locally to refresh `Cargo.lock`, and commit both files.
   The release jobs build with `--locked`, so a stale `Cargo.lock` fails
   them before any draft exists. The tag check also runs for drafts, so
   the tag must point at a commit whose `Cargo.toml` version matches it.
3. Tag that commit and push the tag, then start the "Release" workflow
   from the Actions tab (`workflow_dispatch`) with that tag as input.
4. Check the draft: it must hold exactly the six assets in the table
   above (with `0.0.0-test` as the version), and `latest.json` must list
   only `windows-x86_64` and `linux-x86_64`, with URLs equal to the
   asset names.
5. Delete the draft release, the tag and the scratch branch.

## Rollback to the Win32 build

If a Tauri release is broken in the field, publish the Win32 sources
again under a higher version with the "Release legacy" workflow
(`release-legacy.yml`):

1. Pick a version `X.Y.Z` higher than every release in the field, so
   every client sees it as an update.
2. Start "Release legacy" from the Actions tab with `ref` set to a ref
   holding the Win32 sources (for example `v0.5.0`), `version` set to
   `X.Y.Z`, `tag` set to `vX.Y.Z`, and `draft` left at `true`.
3. The workflow patches the `Cargo.toml` version at that ref, builds
   with `cargo build --release --locked`, signs the exe with the same
   updater key and writes a Windows-only `latest.json`. Check the draft:
   it holds `claude-code-usage-bubble.exe`, its `.sig` and `latest.json`.
4. Publish it: either edit the draft into a release marked latest, or
   delete the draft and re-run the workflow with `draft` set to `false`,
   which publishes it as latest.

Legacy clients take the exe through the GitHub API, Tauri Windows
clients through the signed `latest.json`, and Tauri Linux clients find
no entry for their platform, which they report as up to date. The
Win32 app accepts the `--wait-pid <pid> --updated-to X` relaunch the
Tauri updater uses.

## Versioning

Semver-ish: bump patch for fixes, minor for features, major for
breaking changes (e.g. `settings.json` schema change). The in-app
updater compares `Version { major, minor, patch }` lexicographically,
so a higher tuple wins.

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
