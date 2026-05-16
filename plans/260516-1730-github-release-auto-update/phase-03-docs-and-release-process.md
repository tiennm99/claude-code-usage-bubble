---
phase: 3
title: "Docs and release process"
status: in_progress
priority: P2
effort: "1h"
dependencies: [2]
---

# Phase 3: Docs and release process

## Overview

Now that v0.1.x ships out of CI, update the user-facing docs to point
people at the GitHub Release instead of "build from source", and
write a short maintainer checklist that future-me can follow to cut
a release without re-deriving it from this plan.

## Requirements

### Functional
- `README.md` "Install" section points to the Releases page and the SmartScreen warning.
- A new `docs/release-process.md` (one page) lists the cut-a-release steps: bump `Cargo.toml`, commit, tag, push.
- Keep the "build from source" path as a secondary option for developers.

### Non-functional
- `docs/release-process.md` under 60 lines.
- No `CHANGELOG.md` — the GitHub auto-generated release notes are the changelog.

## Architecture

The README has one "Install" section (`README.md:56-66`). Replace it
with a two-track structure:

```
Install
├── Download binary (recommended, one paragraph + SmartScreen note)
└── Build from source (existing block, kept verbatim)
```

## Related Code Files

- Modify: `README.md` (Install section)
- Create: `docs/release-process.md`

## Implementation Steps

1. **README.md** — replace the "Install" section. Sketch:

   ```markdown
   ## Install

   ### Download the latest release

   Grab `claude-code-usage-bubble.exe` from the
   [Releases page](https://github.com/tiennm99/claude-code-usage-bubble/releases/latest).
   Put it anywhere on disk (e.g. `%LOCALAPPDATA%\ClaudeCodeUsageBubble\`)
   and run it. The app self-updates from the same Releases feed.

   First-run note: the binary is unsigned, so SmartScreen will show
   "Windows protected your PC". Click "More info" → "Run anyway".
   Code signing is on the roadmap.

   ### Build from source

   <existing block: git clone + cargo build --release>
   ```

2. **docs/release-process.md** — new file. Sketch:

   ```markdown
   # Cutting a release

   The `release.yml` workflow builds and publishes on every pushed
   tag matching `v*.*.*`. The workflow asserts that the pushed tag
   matches `Cargo.toml` `version` and **fails fast on mismatch**, so
   the order below matters: bump Cargo *before* you tag.

   Steps for a new version:

   1. Bump `Cargo.toml` `version` (`X.Y.Z`).
   2. `cargo build --release` locally to refresh `Cargo.lock`.
   3. Commit: `chore: bump version to X.Y.Z`.
   4. Tag and push:
      ```bash
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
   does not show up on the public Releases feed or trigger
   self-updates for users.

   ## Versioning

   Semver-ish: bump patch for fixes, minor for features, major for
   breaking changes (e.g. settings.json schema change). The in-app
   updater compares `Version { major, minor, patch }` lexicographically.
   ```

3. Verify the Releases page link in the README resolves (it will once Phase 2 has cut at least v0.1.0).

## Todo List

- [x] `README.md` Install section rewritten with two tracks
- [x] SmartScreen note added
- [x] `docs/release-process.md` created
- [ ] Links verified by clicking through (requires Phase 2 v0.1.0 release to exist)

## Success Criteria

- [ ] A new contributor reading only `README.md` knows how to install without building.
- [ ] A maintainer reading only `docs/release-process.md` can cut a release without re-reading this plan.
- [ ] No mention of "Until packaged binaries are published" remains anywhere.

## Risk Assessment

| Risk | Likelihood | Mitigation |
|---|---|---|
| README link to `/releases/latest` 404s before first release exists | Certain pre-Phase-2 | Land this phase **after** Phase 2 has cut v0.1.0 |
| Users skip the SmartScreen note and panic | Medium | Bold the "Click More info → Run anyway" line; mention it in the Releases body too if needed |

## Security Considerations

- The SmartScreen warning is the user's signal that the binary is unsigned. Be honest about it; do not obscure it.
- Recommending `%LOCALAPPDATA%` as the install location keeps the user inside their writable tree (no UAC needed for self-update).
