# Code Review — Tray "Restart" Action

**Scope:** uncommitted changes on clean tree
**Files:** `src/app.rs`, `src/i18n/mod.rs`, 8x `src/i18n/locales/*.toml`
**Plan:** `plans/260518-0945-menu-restart-button/phase-01-implement-restart-action.md`

## Verdict
Clean implementation. All 7 acceptance criteria met. Build is `cargo check`-clean. No security regressions. Pattern faithfully borrowed from `update/install.rs`.

## Acceptance Criteria — all PASS
1. Menu order verified `app.rs:1040-1042`: separator → `IDM_RESTART` → `IDM_EXIT`.
2. `IDM_RESTART => restart_app()` arm wired `app.rs:393`.
3. `restart_app()` `app.rs:1382-1421` flushes settings, gets `current_exe`, rejects `%`, spawns detached `cmd.exe`, `PostQuitMessage(0)`.
4. 1 s `timeout` matches install.rs precedent (2 s there; 1 s sufficient — current process exits as soon as `PostQuitMessage(0)` drains the loop).
5. Verified `restart = "..."` in all 8 TOMLs at line 32 (en/de/es/fr/ja/ko/nl/zh-TW). `LocaleStrings` field at `mod.rs:52`. No `#[serde(default)]` → missing key = hard fail; all present.
6. Match-arm ordering unambiguous: `IDM_RESTART=33` < guard `x >= IDM_LANG_BASE=100`. Guard won't match 33. `tray::IDM_TOGGLE_WIDGET=50` likewise < 100. Safe.
7. No new clippy issues; no new unsafe blocks (`PostQuitMessage(0)` already unsafe at `IDM_EXIT`; matches that idiom).

## Critical
None.

## High
None.

## Medium
**M1. Restart arm sits below the `IDM_LANG_BASE` guard arm.** `app.rs:391-393`. The guard `x if x >= IDM_LANG_BASE => …` is exhaustive for any id `>= 100`. Today `IDM_RESTART=33` is fine, but future readers adding a static id `>= 100` between lines 392 and 393 would silently route into language switching. Cheap fix: move `IDM_RESTART => restart_app()` and `tray::IDM_TOGGLE_WIDGET => …` ABOVE the guard arm. Plan note at `app.rs:82-84` already warns about this — the new arm violates that guidance.

## Low / Info
**L1. Double-restart not deduped.** Rapid clicks queue multiple `cmd.exe` children. First wins the mutex; second's `start ""` succeeds, the resulting bubble process exits at `ERROR_ALREADY_EXISTS`. Acceptable per plan §Risk Assessment. No fix needed.

**L2. `to_string_lossy()` on `current_exe()` will mangle non-UTF-8 paths.** Same pattern in `install.rs:100`. On real Windows installs paths are UTF-16; lossy → UTF-8 is virtually always faithful. Consistent with existing precedent.

**L3. `settings::save()` runs while `lock_state()` read-guard is held** (`app.rs:1385-1387`). If `save` ever takes a lock on the same mutex this would deadlock — it currently does not, but the pattern elsewhere (e.g. `set_poll_interval` at 1087-1100) clones, releases, then saves. Recommend matching that pattern: clone snapshot inside scope, drop guard, then `settings::save(&snap)`. Defensive only.

**L4. No regression to existing menu wiring** — verified by inspection: `show_widget` append at 1034-1039 still preceded by no separator, then separator 1040, then Restart, then Exit. Matches plan exactly.

## Pattern-Parity Check vs `install.rs`
- Flags: `CREATE_NO_WINDOW | DETACHED_PROCESS` → identical bit pattern (`0x0800_0000 | 0x0000_0008`). New constants `RESTART_*` duplicate the values; minor DRY nit but they're file-local and the comment explains why. Acceptable.
- `raw_arg` quoting: `/c` then `"<cmd>"` with inner `"` preserved → byte-for-byte same shape as `install.rs:113-114`. Correct.
- `%` rejection: present, logs and aborts. Matches `install.rs:89-96`.
- `stdin/out/err = Null`: present, matches.

## PostQuitMessage on UI thread
`IDM_EXIT` does the same at `app.rs:370`, called from `on_menu_command` via WM_COMMAND on the UI thread. `restart_app()` is reached the same way. Safe — identical control-flow shape.

## Metrics
- New code: ~40 LOC in `app.rs`, 1 field in `mod.rs`, 8x 1-line TOML adds.
- Type coverage: 100%.
- New warnings: 0 (`cargo check` clean per user).

## Recommended Actions
1. **M1** (nice-to-have): reorder match arms so `IDM_RESTART` / `IDM_TOGGLE_WIDGET` precede the `x if x >= IDM_LANG_BASE` guard. Defends against future id collisions.
2. **L3** (optional): mirror `set_poll_interval`'s clone-then-save pattern in `restart_app()` for consistency.

## Unresolved Questions
- None blocking. Plan §Next Steps suggests a semver patch bump and analogous bubble-menu entry; out of scope for this review.

---

**Status:** DONE_WITH_CONCERNS
**Summary:** Implementation matches plan and acceptance criteria; cmd-handoff faithfully mirrors `update/install.rs`; all 8 locales updated; no critical or high issues. One medium suggestion (reorder match arms to defend against future static-id collisions with the `IDM_LANG_BASE` guard) and two low/optional refinements.
**Concerns:** M1 — new `IDM_RESTART` and `tray::IDM_TOGGLE_WIDGET` arms sit below a catch-all `x >= IDM_LANG_BASE` guard. Today safe (33, 50 < 100); future-fragile. Code comment at `app.rs:82-84` already flags the rule that was bent.
