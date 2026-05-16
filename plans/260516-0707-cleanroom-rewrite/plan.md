---
status: pending
created: 2026-05-16
mode: standard (non-TDD)
brainstorm: ../reports/brainstorm-260516-0707-cleanroom-reimplementation.md
---

# Clean-room rewrite of ported modules

Replace ~2,700 LOC of code copied from `CodeZeno/Claude-Code-Usage-Monitor` with genuinely-original implementations, then drop the `NOTICE` attribution file.

## Source of truth

All design decisions live in
[`../reports/brainstorm-260516-0707-cleanroom-reimplementation.md`](../reports/brainstorm-260516-0707-cleanroom-reimplementation.md).
Do not re-debate them during implementation. If a phase reveals a flaw,
note it in that phase's "Open questions" and ask the user before
deviating.

## Architecture (locked)

| Axis | Decision |
|---|---|
| HTTP | WinHTTP via `windows-rs` |
| Errors | `thiserror` per-module enums |
| Providers | `trait UsageProvider` + ClaudeProvider/ChatGptProvider |
| Credentials | `trait CredentialSource` + Vec<Box<dyn ...>> |
| Token refresh | `RefreshOrchestrator`, 8s timeout |
| i18n | TOML files via `include_str!` |
| Tray badges | `tiny-skia` anti-aliased |
| Updater | inline `cmd /c` handoff (no helper-exe) |
| Logging | `log` + `simplelog` |
| Build script | `embed-resource` crate |

## New module layout

```
src/
  main.rs                 (kept — update imports only)
  app.rs                  (kept — update imports + call-sites)
  bubble.rs               (kept — update imports only)
  panel.rs                (kept — update imports only)
  settings.rs             (kept — update imports only)

  diag/mod.rs             — log facade + simplelog file appender
  os/                     — Win32 helpers (color, string, dpi, registry, theme)
  net/                    — WinHTTP HTTP client
  usage/                  — Provider trait + types + impls + refresh
  creds/                  — CredentialSource trait + impls
  i18n/                   — TOML loader + 9 locale files
  tray/                   — Anti-aliased tray badges
  update/                 — Self-updater (release check, download, handoff, channel)
```

## Phases

| # | Phase | Hours | File |
|---|---|---|---|
| 1 | Infrastructure (`diag/`, `os/`, `net/winhttp.rs`, Cargo.toml) | ~9 | [`phase-01-infrastructure.md`](phase-01-infrastructure.md) |
| 2 | Types & i18n (`usage/types.rs`, `i18n/`) | ~6 | [`phase-02-types-and-i18n.md`](phase-02-types-and-i18n.md) |
| 3 | Credentials (`creds/`) | ~3 | [`phase-03-creds-module.md`](phase-03-creds-module.md) |
| 4 | Providers & refresh (`usage/anthropic.rs`, `chatgpt.rs`, `refresh.rs`) | ~8 | [`phase-04-providers-and-refresh.md`](phase-04-providers-and-refresh.md) |
| 5 | Tray badges (`tray/badge.rs`) | ~5 | [`phase-05-tray-badges.md`](phase-05-tray-badges.md) |
| 6 | Updater + remove NOTICE | ~7 | [`phase-06-updater-and-remove-notice.md`](phase-06-updater-and-remove-notice.md) |

**Total:** ~38h core work + 4–8h Windows-side debugging.

## Phase dependencies

```
Phase 1 (infra)
   ├─→ Phase 2 (types + i18n)
   │       └─→ Phase 4 (providers)
   │               └─→ Phase 5 (tray) — needs UsageProvider results
   │
   └─→ Phase 3 (creds)
           └─→ Phase 4 (providers) — depends on creds API
                   └─→ Phase 6 (updater + cleanup) — last
```

Phases 2 and 3 can technically run in parallel after Phase 1, but
serial execution (1→2→3→4→5→6) is cleaner for solo work.

## Out of scope

- `bubble.rs`, `panel.rs`, `app.rs`, `settings.rs`, `main.rs` — these
  stay; only their imports/call-sites get touched as new APIs come
  online.
- Adding new features beyond what the current copied code supports.
- Changing `bubble.rs`/`panel.rs` rendering or interaction behavior.

## External contracts that must NOT change

- Anthropic endpoints + headers
- ChatGPT endpoint + `User-Agent: codex-cli`
- Credential file paths and JSON shapes
- WSL access via `wsl.exe`
- CLI-driven token refresh (must invoke `claude` / `codex`)
- GitHub releases JSON format
- Settings file location (`%APPDATA%\ClaudeCodeUsageBubble\settings.json`)
- Windows registry path for startup (`Software\Microsoft\Windows\CurrentVersion\Run`)
- Single-instance mutex (`Global\ClaudeCodeUsageBubble`)

## Success criteria (cross-phase)

After all 6 phases:

- [ ] `cargo build --release` clean on Windows
- [ ] No file in `src/` shares a name with the upstream source's files
- [ ] `NOTICE` file removed from repo root
- [ ] `LICENSE` (Apache-2.0) header retained; copyright line updated
- [ ] `README.md` updated to drop the "derivative of" paragraph; replace
      with "inspired by [upstream]" link
- [ ] App functional: bubble, panel, tray, polling, auth, updater all
      working on Windows 10/11
- [ ] No regressions vs current behavior (poll cadence, snap, click→panel,
      Ctrl+Wheel resize, fullscreen auto-hide, dual-bubble)

## Rollback strategy

- Each phase lands as a separate commit (or PR). If a phase breaks
  the build/app, `git revert` that commit to restore the prior phase's
  state.
- Phase 6 is the only commit that removes upstream attribution — if any
  of Phases 1–5 is incomplete or buggy at that point, **do not** drop
  NOTICE; finish or revert first.

## Open questions

- Bump `windows-rs` from 0.58 → newer? (Brainstorm flagged this.)
  Defer decision to Phase 1 — try with 0.58 first, bump only if needed.
- Keep Git history showing the initial port? Recommended: **yes**.
  Transparent and consistent with the "I inspired/rewrote from X" framing
  even after NOTICE is gone.
