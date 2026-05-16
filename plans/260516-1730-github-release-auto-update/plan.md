---
title: "GitHub release CI + auto-update wiring"
description: "Publish Windows binaries to GitHub Releases via Actions so the existing in-app updater can self-update."
status: pending
priority: P2
created: 2026-05-16
---

# GitHub release CI + auto-update wiring

## Overview

The self-update subsystem already exists end-to-end in `src/update/`:
`release::fetch_latest` polls `releases/latest` on GitHub, parses the
`tag_name` into a `Version`, picks the asset whose name matches
`claude-code-usage-bubble.exe` (or the first `.exe` as fallback), and
`install::begin` downloads it + spawns an inline `cmd /c` handoff
that swaps the running exe and relaunches. `app.rs` wires this to a
24-hour timer (`UPDATE_CHECK_INTERVAL_SECS`) and the right-click menu
("Check for updates" / "Update available" / "Applying update…").

What is missing is the **producer side**: no `.github/workflows/`
directory exists, the repo has no tags, and the README explicitly
says "Until packaged binaries are published, build from source". The
updater therefore has nothing to pull from. Closing that loop is the
whole job.

This plan ships three things: (1) a tag-triggered GitHub Actions
workflow that builds release on `windows-latest` and attaches the
exe to a GitHub Release, (2) an end-to-end test that proves a running
v0.1.0 actually self-updates to v0.1.1 in the wild, and (3) the docs
and release-cutting checklist so future versions ship by pushing a
tag.

Out of scope: code signing, winget channel (`Channel::Winget` stays
stubbed), SHA256 sidecar verification (HTTPS + cert pinning by WinHTTP
is the security floor; checksum is a nice-to-have for later), and any
new updater code paths beyond what the existing code already supports.

## Phases

| Phase | Name | Status |
|-------|------|--------|
| 1 | [Release CI workflow](./phase-01-release-ci-workflow.md) | Files written, awaiting commit + push |
| 2 | [End-to-end update test](./phase-02-end-to-end-update-test.md) | Pending (user-driven: requires tag pushes + Windows runs) |
| 3 | [Docs and release process](./phase-03-docs-and-release-process.md) | Files written, awaiting commit; link verification gated on Phase 2 |

## Key contracts (must not break)

The updater is already shipped logic — these constants are the
contract the CI workflow has to satisfy:

| Contract | Source | Value |
|---|---|---|
| Asset filename (primary match) | `src/update/release.rs:7` | `claude-code-usage-bubble.exe` |
| Asset filename (fallback) | `src/update/release.rs:67-69` | any `*.exe` |
| Endpoint | `src/update/release.rs:45` | `https://api.github.com/repos/tiennm99/claude-code-usage-bubble/releases/latest` |
| Tag → version parse | `src/update/release.rs:33-41` | strips leading `v`, splits on `-`, takes `major.minor.patch` |
| Current version source | `Cargo.toml` `version` | bumped per release |

A tag like `v0.1.1` → parses as `Version { 0, 1, 1 }`. The workflow
MUST upload an asset named exactly `claude-code-usage-bubble.exe`.

## Dependencies

No cross-plan dependencies. The prior plan
[`260516-0707-cleanroom-rewrite/phase-06-updater-and-remove-notice.md`](../260516-0707-cleanroom-rewrite/phase-06-updater-and-remove-notice.md)
delivered the consumer side and is complete in code (whether its own
phase row is checked is independent of this plan).

## Validation Log

### Session 1 — 2026-05-16

#### Verification Results
- **Tier:** Standard (3 phases → Fact Checker + Contract Verifier)
- **Claims checked:** 9
- **Verified:** 9 | **Failed:** 0 | **Unverified:** 0
- Claims verified: `ASSET_NAME` constant at `src/update/release.rs:7`, matcher at `src/update/release.rs:64`, fallback at `src/update/release.rs:67-69`, URL endpoint at `src/update/release.rs:45`, version parse at `src/update/release.rs:33-41`, `version_action` apply branch at `src/app.rs:1037-1066`, 24-hour interval at `src/app.rs:48`, Cargo `name = "claude-code-usage-bubble"` and `version = "0.1.0"` at `Cargo.toml:2-3`, README "Until packaged binaries are published" at `README.md:58`.

#### Decisions

1. **Version-tag match enforcement: YES, fail-fast in CI.**
   The release workflow must parse `Cargo.toml` and abort if the tag (e.g. `v0.1.1`) does not equal the Cargo version. Prevents silent mismatch where a binary self-reports a different version than the release tag — which would in turn break the updater's `Version::current() vs Version::parse(tag_name)` comparison and either skip a real update or loop on the same one.
   → Propagated to `phase-01-release-ci-workflow.md` as a new step + extra success criterion.

2. **Initial release strategy: tag v0.1.0 first, then bump to v0.1.1 for the E2E test.**
   Phase 2 stays as written. Cut v0.1.0 from current `main` (no Cargo bump needed since Cargo.toml is already `0.1.0`), download the asset, bump Cargo to `0.1.1`, tag `v0.1.1`, watch the v0.1.0 binary self-update to v0.1.1. Two real releases, clean test.
   → No changes needed in Phase 2 — already aligned.

3. **Asset matcher `*.exe` fallback: defer.**
   Today only one asset ships so the fallback at `src/update/release.rs:67-69` is dead code. Once multi-arch lands (`x86_64`/`arm64`), the fallback could pick the wrong binary. Tracked as a future cleanup, NOT in scope for this plan.
   → Documented in Phase 1 success criteria as a non-action.

#### Whole-Plan Consistency Sweep
- Files reread: `plan.md`, `phase-01-release-ci-workflow.md`, `phase-02-end-to-end-update-test.md`, `phase-03-docs-and-release-process.md`
- Decision deltas checked: 3 (version-match enforcement, initial release version, asset fallback)
- Reconciled stale references: 4
  - Phase 1 architecture flow + todo list + success criteria updated to include version-tag check
  - Phase 1 non-functional line-count budget bumped 80 → ~100 lines to match the actual YAML after adding the check step
  - Phase 1 flow diagram cleaned up (removed phantom `rustup default stable` step; added cache + dispatch-draft notation)
  - Phase 3 `release-process.md` sketch now calls out that CI enforces tag-vs-Cargo match, explaining why step ordering matters
- Unresolved contradictions: 0
