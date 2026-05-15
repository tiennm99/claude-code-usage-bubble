# Plan: claude-code-usage-bubble — port from CodeZeno/Claude-Code-Usage-Monitor

**Mode:** `/ck:xia --port`
**Source repo:** `/config/workspace/CodeZeno/Claude-Code-Usage-Monitor` (Rust ~5.8k LOC, MIT)
**Target repo:** `/config/workspace/CodeZeno/claude-code-usage-bubble` (new)
**Date:** 2026-05-15

## Source Manifest

- Path: `/config/workspace/CodeZeno/Claude-Code-Usage-Monitor`
- Branch: `main` @ `b5f038d` (v1.4.1)
- License: MIT (attribution required in new repo README)
- Scope: portable subsystems only — see `phase-02-port-portable-modules.md`

## Decision Matrix (Approved)

| Decision | Choice |
|---|---|
| Platform | Windows-only (Win32 GDI + layered window) |
| WSL credential reading | Keep |
| Snap-to-edge | On, 12px zone, monitor work area |
| Click behavior | Left-click = toggle panel, right-click = menu |
| Dual-model layout | Two independent bubbles, positions persisted per model |
| Winget channel | Code kept, `current_install_channel` stubbed to Portable |
| Single-instance mutex | `Global\ClaudeCodeUsageBubble` |
| Auto-hide when fullscreen | **Yes** (added to phase 3) — detect via `SHQueryUserNotificationState` or `MonitorFromWindow + window-rect == monitor-rect` against foreground HWND |
| Bubble size customization | **Yes**, free range 32–128 px persisted in settings.json; resize via Ctrl+MouseWheel on bubble (no S/M/L menu) |
| Per-model bubble art | **No** — both models share the same bubble look; differentiation only via usage-percentage ring color |

## Dependency Matrix (Source → New)

| Source file | LOC | Action | Target file |
|---|---|---|---|
| `src/models.rs` | 19 | COPY | `src/models.rs` |
| `src/diagnose.rs` | 52 | COPY | `src/diagnose.rs` |
| `src/theme.rs` | 52 | COPY | `src/theme.rs` |
| `src/poller.rs` | 1099 | COPY | `src/poller.rs` |
| `src/updater.rs` | 510 | COPY + stub channel | `src/updater.rs` |
| `src/tray_icon.rs` | 441 | COPY | `src/tray_icon.rs` |
| `src/localization/*` | ~620 | COPY | `src/localization/*` |
| `src/native_interop.rs` | 179 | ADAPT (drop taskbar/WinEvent helpers) | `src/native_interop.rs` |
| `src/main.rs` | 40 | ADAPT (call `bubble::run` instead of `window::run`, rename single-instance mutex) | `src/main.rs` |
| `src/window.rs` | 2847 | **REWRITE** as `bubble.rs` + `panel.rs` + `settings.rs` + `app.rs` | NEW |
| `build.rs`, `Cargo.toml`, `src/icons/*` | — | ADAPT | NEW |

## Phases

| # | Phase | Status | File |
|---|---|---|---|
| 1 | Bootstrap repo (Cargo.toml, build.rs, LICENSE, README, icons) | pending | `phase-01-bootstrap-repo.md` |
| 2 | Port portable modules verbatim | pending | `phase-02-port-portable-modules.md` |
| 3 | Build floating bubble window (layered alpha, GDI ring, drag-anywhere, snap) | pending | `phase-03-build-bubble-window.md` |
| 4 | Build expanded panel + settings persistence + orchestration | pending | `phase-04-panel-and-orchestration.md` |
| 5 | Polish: HiDPI, multi-monitor, startup registry, mutex, tray icon wiring, README | pending | `phase-05-polish-and-finishing.md` |

## Risk Score

**Medium.** Highest-risk surface is **phase 3** — circular layered window with HiDPI-aware GDI ring drawing. Source codebase has no precedent for that exact pattern; needs fresh implementation. All other phases are straightforward ports or thin orchestration.

| Risk | Severity | Mitigation |
|---|---|---|
| GDI ring + ClearType text on layered alpha window | High | Reference `window.rs` UpdateLayeredWindow + DIB section pattern (lines around layered painting); keep ring math simple (parametric arc) |
| Drag + snap interaction on multi-monitor | Medium | Use `MonitorFromPoint` per move; clamp to nearest monitor work area |
| Two-bubble position state | Low | Independent `BubbleState` structs in settings.json |
| WSL credential read regressions | Low | Verbatim port; no behavioral changes |

## Estimated Effort

- Phase 1: 1–2h
- Phase 2: 1h (mostly file copies + import path fixes)
- Phase 3: 6–10h (the heavy lift)
- Phase 4: 3–5h
- Phase 5: 2–4h

**Total:** ~15–22h of focused implementation.

## Rollback Strategy

The new repo is greenfield — rollback means `rm -rf /config/workspace/CodeZeno/claude-code-usage-bubble`. No source-repo changes; this plan does not modify the source app.

## Open Questions

- None. All three formerly-deferred items resolved by user on 2026-05-15 (see Decision Matrix rows 8–10).
