---
title: Rename to usage-monitor
description: >-
  Rename the repository and product from claude-code-usage-bubble to
  usage-monitor once the Tauri port lands.
status: pending
priority: P3
branch: main
tags:
  - rename
blockedBy:
  - 261021-1028-tauri-v2-port
blocks: []
created: '2026-10-21T05:26:00.000Z'
---

# Rename To usage-monitor

## Decision

The user chose `usage-monitor` (display name "Usage Monitor") on 2026-10-21.
Both Claude Code and Codex call their quota "usage", and the name does not tie
the app to the bubble shape or to one provider. This is a personal tool, so the
user's preference decides the name.

## Constraints

- Installed legacy copies update only from an asset named
  `claude-code-usage-bubble.exe` in `tiennm99/claude-code-usage-bubble`
  releases. The first releases after the rename must keep publishing that
  asset name, and the old repository URL must keep redirecting.
- Whether the legacy WinHTTP updater follows GitHub's API redirect after a
  repository rename is unverified. Verify it before relying on the redirect.

## Steps

1. Rename the GitHub repository to `tiennm99/usage-monitor` with
   `gh repo rename`, then update `origin` and move the local checkout to
   `tiennm99/usage-monitor`.
2. Update the updater's repository constant, `productName`, README, release
   workflow and docs to the new name. Keep publishing the legacy `.exe` asset
   name alongside the new one.
3. Confirm a legacy copy still finds the update through the redirect.
