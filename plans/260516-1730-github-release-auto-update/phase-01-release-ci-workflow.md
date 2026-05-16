---
phase: 1
title: "Release CI workflow"
status: in_progress
priority: P1
effort: "2h"
dependencies: []
---

# Phase 1: Release CI workflow

## Overview

Ship `.github/workflows/release.yml`. On a pushed tag matching `v*`,
the workflow verifies the tag matches `Cargo.toml`'s `version` field
(fail-fast if not), builds `cargo build --release` on `windows-latest`,
renames/copies the binary to the exact asset name the updater expects
(`claude-code-usage-bubble.exe`), and creates a GitHub Release with
that asset attached and auto-generated notes. Also supports
`workflow_dispatch` for dry-run testing without cutting a real tag.

<!-- Updated: Validation Session 1 — version-tag match enforcement added per plan.md Validation Log decision 1 -->
<!-- Updated: Validation Session 1 — *.exe fallback at src/update/release.rs:67-69 is out of scope per plan.md Validation Log decision 3 -->


## Requirements

### Functional
- Trigger on tag push matching `v*.*.*` (and `workflow_dispatch` for testing).
- **Verify the tag matches `Cargo.toml` `version`** before building. Workflow aborts on mismatch.
- Build on `windows-latest` with stable Rust toolchain, `x86_64-pc-windows-msvc` target.
- Cache cargo registry + target dir to keep wall time under ~5 min.
- Upload `target/release/claude-code-usage-bubble.exe` as a Release asset.
- Use `gh release create` with `--generate-notes` for the body.
- Use `--draft` on `workflow_dispatch` runs so test runs don't become public.
- On real tag runs (`vX.Y.Z`), publish immediately (not draft).

### Non-functional
- Workflow file under ~100 lines.
- No third-party Marketplace actions other than `actions/checkout` and `Swatinem/rust-cache` (or just `actions/cache`). Avoid `softprops/action-gh-release`-style wrappers — `gh` CLI is preinstalled on the runner and is one less supply-chain risk.
- Default `GITHUB_TOKEN` permissions, with explicit `contents: write` only on the release job.

## Architecture

### Flow

```
push tag v0.1.1
  └─→ release.yml (job: build, runs-on: windows-latest)
        ├─ checkout (at the tag)
        ├─ Swatinem/rust-cache (uses runner-default stable Rust)
        ├─ resolve tag (from refs/tags or workflow_dispatch input)
        ├─ verify Cargo.toml version == tag (strip leading 'v') → abort on mismatch
        ├─ cargo build --release --locked
        ├─ gh release create v0.1.1 target/release/claude-code-usage-bubble.exe \
        │     --title "v0.1.1" --generate-notes [--draft on workflow_dispatch]
        └─ done
```

### Asset name verification

The updater's primary matcher is `eq_ignore_ascii_case("claude-code-usage-bubble.exe")`
(`src/update/release.rs:64`). Cargo's `name = "claude-code-usage-bubble"`
already produces that exe name in `target/release/`, so no rename is
needed — just upload the file as-is.

## Related Code Files

- Create: `.github/workflows/release.yml`
- Reference (do not modify in this phase): `src/update/release.rs`, `Cargo.toml`

## Implementation Steps

1. Create `.github/workflows/release.yml` with this shape:

   ```yaml
   name: Release

   on:
     push:
       tags: ['v*.*.*']
     workflow_dispatch:
       inputs:
         tag:
           description: 'Tag to release (must already exist, e.g. v0.1.1)'
           required: true

   permissions:
     contents: write

   jobs:
     build:
       runs-on: windows-latest
       steps:
         - uses: actions/checkout@v4
           with:
             ref: ${{ github.event.inputs.tag || github.ref }}

         - uses: Swatinem/rust-cache@v2

         - name: Resolve tag
           id: tag
           shell: pwsh
           run: |
             $tag = if ($env:GITHUB_REF -like 'refs/tags/*') {
               $env:GITHUB_REF -replace '^refs/tags/',''
             } else {
               '${{ github.event.inputs.tag }}'
             }
             "tag=$tag" | Out-File -FilePath $env:GITHUB_OUTPUT -Append

         - name: Verify Cargo.toml version matches tag
           shell: pwsh
           run: |
             $tag = '${{ steps.tag.outputs.tag }}'
             $expected = $tag -replace '^v',''
             $cargoVersion = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches.Groups[1].Value
             if ($cargoVersion -ne $expected) {
               Write-Error "Tag ($tag → $expected) does not match Cargo.toml version ($cargoVersion). Bump Cargo.toml before tagging."
               exit 1
             }
             Write-Host "Cargo version $cargoVersion matches tag $tag"

         - name: Build release
           run: cargo build --release --locked

         - name: Create release
           env:
             GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
           shell: pwsh
           run: |
             $tag = '${{ steps.tag.outputs.tag }}'
             $asset = 'target/release/claude-code-usage-bubble.exe'
             $draft = if ('${{ github.event_name }}' -eq 'workflow_dispatch') { '--draft' } else { '' }
             gh release create $tag $asset --title $tag --generate-notes $draft
   ```

2. Sanity-check: `gh workflow list` after pushing the file shows the new "Release" workflow.

3. Verify the workflow YAML lints clean by viewing it in the GitHub UI (or with `actionlint` locally if installed).

## Todo List

- [x] `.github/workflows/release.yml` written
- [x] `permissions: contents: write` set
- [x] Tag-push trigger and `workflow_dispatch` both wired
- [x] Cargo.toml-version-vs-tag check step added and fails on mismatch
- [x] Asset path is `target/release/claude-code-usage-bubble.exe` exactly
- [x] `--generate-notes` enabled
- [ ] Committed and pushed to `main`

## Success Criteria

- [ ] Workflow appears under Actions tab on GitHub.
- [ ] Manually dispatching with a throwaway tag (`v0.0.0-test`) produces a **draft** release with the `.exe` attached.
- [ ] Pushing a tag whose value disagrees with `Cargo.toml` fails the workflow before `cargo build` runs (verify by intentionally mismatching once on a throwaway dispatch).
- [ ] No third-party actions beyond `actions/checkout@v4` and `Swatinem/rust-cache@v2`.
- [ ] Workflow file is under ~100 lines including blank lines.
- [ ] Out of scope (do not touch): `*.exe` fallback at `src/update/release.rs:67-69`. Tracked as future cleanup once multi-arch ships.

## Risk Assessment

| Risk | Likelihood | Mitigation |
|---|---|---|
| `Cargo.lock` drift causes `--locked` to fail | Low | Lockfile is committed; bump it locally before tagging if deps changed |
| Build time >10 min and cache cold | Low | `rust-cache` covers cargo registry + `target/`; first run is slow, subsequent fast |
| Tag pushed without prior `Cargo.toml` version bump | Was Medium → now Mitigated | CI now fails fast in the "Verify Cargo.toml version matches tag" step; maintainer cannot accidentally ship a version-mismatched binary |
| `gh release create` fails because tag does not exist for workflow_dispatch | Medium | Workflow_dispatch input takes a tag string and `actions/checkout` is pinned to it — if the tag does not exist, checkout fails fast with a clear error |
| Pre-release tag like `v0.1.0-rc1` triggers workflow but does not match Cargo's stable version | Low | Tag-match check uses string equality after `^v` strip; `0.1.0-rc1 != 0.1.0` fails fast. If pre-releases are wanted later, change Cargo `version` and the check still works |

## Security Considerations

- `permissions: contents: write` is the minimum scope needed to create a release; no `id-token` or package perms requested.
- `GH_TOKEN` is the default `GITHUB_TOKEN`, scoped to this repo only.
- No secrets are echoed; `gh` reads `GH_TOKEN` from env.
- The published `.exe` is unsigned. SmartScreen will show "Unknown publisher" the first time a user runs it. Document this in Phase 3; code signing is out of scope.
