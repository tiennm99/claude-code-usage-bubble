# Phase 01 — Implement Restart Action

## Context Links

- Reused pattern: `src/update/install.rs:1-120` (cmd-handoff swap-and-restart). Documented in `docs/release-process.md` if it exists.
- Menu wiring reference: `src/app.rs:870-1045` (`show_context_menu`) and `src/app.rs:363-392` (`on_menu_command`).
- Mutex acquisition: `src/app.rs:152-168` (`Global\ClaudeCodeUsageBubble`).
- i18n schema: `src/i18n/mod.rs` (`LocaleStrings` struct around line 22-80).

## Overview

- **Priority:** Low (UX enhancement).
- **Status:** Done. Code-reviewer DONE_WITH_CONCERNS — M1 (match-arm ordering) + L3 (lock-during-save) addressed in follow-up edits.
- **Size:** ~50 LOC across 3 files (+ 8 locale TOMLs, one line each).

## Key Insights

- The existing mutex check rejects a second instance immediately. A naive "spawn-then-exit" races. The `cmd.exe /c timeout` handoff (1 s sleep, then `start ""`) is the simplest decoupling — same trick `update::install::begin` already uses.
- `cmd.exe` expands `%var%` in argument strings. Current `current_exe()` path containing `%` is an injection vector; reject it (existing precedent: `update::install` rejects too).
- `std::process::Command` with `creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)` ensures the helper outlives the parent silently.

## Requirements

### Functional

- Right-click context menu shows a "Restart" item directly above "Exit".
- Clicking "Restart" closes the current process and a new instance starts within ~1–2 seconds, restoring tray icons and bubbles.
- No confirmation prompt.
- Item label is i18n-aware: all 8 locales get a translation.

### Non-functional

- No regression in mutex single-instance behavior — second instance must still be blocked if user accidentally launches manually mid-restart.
- No console window flashes during handoff.

## Architecture

```
User clicks "Restart"
  → WM_COMMAND with IDM_RESTART
  → app::on_menu_command → app::restart_app()
    → settings::save current snapshot (defensive flush)
    → spawn detached cmd.exe with delayed `start ""` for current_exe
    → PostQuitMessage(0)
  → message loop exits → mutex released
  → cmd.exe wakes up → new instance launches → acquires mutex → run()
```

## Related Code Files

**Modify:**

- `src/app.rs` — add `IDM_RESTART: u16 = 33` const (next free in the 30-39 band), match arm in `on_menu_command`, menu append in `show_context_menu` between `IDM_TOGGLE_WIDGET` row and the separator before `IDM_EXIT`, new `fn restart_app()`.
- `src/i18n/mod.rs` — add `pub restart: String,` field to `LocaleStrings` (place near `exit`).
- `src/i18n/locales/en.toml`, `de.toml`, `es.toml`, `fr.toml`, `ja.toml`, `ko.toml`, `nl.toml`, `zh-TW.toml` — add `restart = "<translation>"`.

**Create:** none.

**Delete:** none.

## Implementation Steps

1. **Add menu ID and i18n field.**
   - `app.rs`: `const IDM_RESTART: u16 = 33;` (after `IDM_VERSION_ACTION`).
   - `i18n/mod.rs`: add `pub restart: String,` to `LocaleStrings`. Place adjacent to `exit`.
   - Add `restart = "Restart"` to `en.toml`. Translate for the other 7 locales (Vietnamese-quality is acceptable; native fluency not required for a single-word menu item).

2. **Wire the menu entry.**
   - `app.rs::show_context_menu` — between the `show_widget` append and the `MF_SEPARATOR` before `IDM_EXIT`, add `append_item(menu, IDM_RESTART, &snap.strings.restart, MENU_ITEM_FLAGS(0));`.
   - Add `IDM_RESTART => restart_app(),` arm in `on_menu_command` before the `_ => {}` catch-all.

3. **Implement `restart_app()`.**
   - Persist a final settings snapshot (defensive flush). Read current state, call `settings::save(&snap)`.
   - Resolve `std::env::current_exe()`. If `Err`, log error and `PostQuitMessage(0)` (degrade to plain Exit).
   - Convert path to string. If it contains `%`, log error and return (refuse — matches `update::install` precedent).
   - Build the cmd line: `timeout /t 1 >nul & start "" "<exe>"`.
   - Spawn via `std::process::Command::new("cmd.exe")` with `.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)` and a `raw_arg` payload `/c "<cmd>"` (mirrors `install.rs:104-114`).
   - On spawn success: `PostQuitMessage(0)`. On failure: log error and return (app stays running).

4. **Verify.**
   - `cargo check` — no warnings.
   - Manual smoke test: build release, right-click tray, Restart, observe close + relaunch within ~2 s, mutex acquired by new instance, bubbles + tray icons rendered.

## Todo List

- [x] Add `IDM_RESTART` const in `app.rs`.
- [x] Add `restart: String` to `LocaleStrings` in `i18n/mod.rs`.
- [x] Update all 8 locale `.toml` files.
- [x] Append menu item in `show_context_menu`.
- [x] Add match arm in `on_menu_command` (placed before `IDM_LANG_BASE` guard per reviewer M1).
- [x] Implement `restart_app()` in `app.rs` (clone-then-save per reviewer L3).
- [x] `cargo check` clean.
- [ ] Manual smoke test on Windows (deferred to user; needs release build).

## Success Criteria

- Right-click tray → menu shows "Restart" between "Show widget" group and "Exit".
- Clicking it closes the process and a new one starts within 2 s with identical state (settings honored, bubble positions persisted, tray icons restored).
- No console window flashes.
- `cargo check` passes with no new warnings.
- All 8 locales include the new key (no fallback to English).

## Risk Assessment

| Risk | Severity | Mitigation |
|------|----------|------------|
| Mutex race — new instance starts before old releases | Medium | 1 s `timeout` in cmd handoff; matches update module precedent. |
| `current_exe()` path contains `%` (injection) | Low | Reject, log, abort (same as `install.rs:90-94`). |
| `cmd.exe` not on PATH (broken Windows install) | Very Low | Log error, app stays running. User can Exit manually. |
| Settings not flushed before quit | Low | Explicit `settings::save()` before `PostQuitMessage`. Bubble positions already persist on drag, so worst case is a no-op. |
| User restart-spams the menu | Low | Each click queues a new cmd handoff; the timeout dedupes via mutex. Worst case: one extra instance attempt that exits immediately on `ERROR_ALREADY_EXISTS`. |

## Security Considerations

- `%`-in-path rejection prevents `cmd.exe` variable expansion injection.
- No user-supplied input enters the cmd line — only `std::env::current_exe()` output.
- Detached process flags prevent inherited stdio from leaking.

## Next Steps

- After merge: bump version (semver patch — UX addition with no API change).
- Consider an analogous restart action for the bubble's context menu (currently the bubble also fires `on_menu_command` via `WM_COMMAND`, so the same menu id works there for free).
