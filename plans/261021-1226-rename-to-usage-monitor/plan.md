---
title: Rename to usage-monitor
description: >-
  Rename the repository and product to usage-monitor, with no trace of the
  previous name left in the repository.
status: completed
priority: P3
branch: feat/tauri-port
tags:
  - rename
blockedBy: []
blocks: []
created: '2026-10-21T05:26:00.000Z'
---

# Rename To usage-monitor

## Decision

The user chose `usage-monitor` (display name "Usage Monitor") on 2026-10-21.
Both Claude Code and Codex call their quota "usage", and the name does not tie
the app to the bubble shape or to one provider. This is a personal tool, so the
user's preference decides the name.

On 2026-10-22 the user chose to remove the previous name completely rather
than keep it for compatibility, and to do the rename before the Tauri port
merges into `main`.

## Result

- The GitHub repository is `tiennm99/usage-monitor`, and the local checkout
  moved to match. GitHub answers API requests for the old repository path
  with a 301, which the legacy WinHTTP client follows.
- The crate, binary, `productName`, identifier, settings folder, log file,
  autostart entry, user agent, updater endpoint and release asset names all
  use the new name.
- Legacy Win32 clients still find updates: they fall back to the first `.exe`
  asset when their own exe name is missing, so a release must carry exactly
  one `.exe`. `release-legacy.yml` reads the package name from the ref it
  builds and publishes the exe as `usage-monitor.exe`.

## Accepted costs

- Installs that upgrade start with default settings: the portable Windows
  exe and the AppImage now keep `settings.json` next to the file, and nothing
  migrates the old one.
- On Windows, an install that had autostart on keeps its old Run entry, which
  points at the same exe file. Turning autostart on in the new app adds a
  second entry.
- Update leftovers in the old local data folder are no longer cleaned up.
