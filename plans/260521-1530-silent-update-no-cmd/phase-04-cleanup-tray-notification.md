---
phase: 4
title: "Cleanup + tray notification"
status: complete
priority: P2
effort: "1.5h"
dependencies: [1, 3]
---

# Phase 4: Cleanup + tray notification

## Overview

Two additions that make the silent update visible to the user without being intrusive: (a) startup cleanup of stale `bubble.exe.old.<pid>` files from previous updates, and (b) a tray balloon "Updated to vX.Y.Z" on first launch after auto-update (driven by the `--updated-to` flag passed by phase 3).

<!-- Updated: Validation Session 1 - i18n approach corrected to match TOML struct-field architecture -->

## Requirements

**Functional**
- On every startup, scan `current_exe().parent()` for files matching `<exe-stem>.exe.old.*` and remove them silently. Errors logged at debug level, never surfaced to user.
- When `--updated-to vX.Y.Z` is passed AND the tray subsystem is initialized, show a balloon notification with localized title + body.
- Add **3 new fields** to `LocaleStrings` (`src/i18n/mod.rs:23-71`): `update_applied_title`, `update_applied_body`, `update_rollback_failed_body` (the last one is consumed by Phase 3 but the i18n change belongs to this phase's pattern). Body strings use Rust `format!` at call site — TOML strings hold raw text (e.g. body = `"Updated to v"`, then call site does `format!("{}{}", strings.update_applied_body, version)`). Choice of suffix vs prefix vs `{}` placeholder substitution is dialect-sensitive; use literal positional substitution via `format!` because TOML doesn't support template placeholders the i18n loader recognizes.
- Translate the 3 new strings in all **8 existing locale files**: `src/i18n/locales/{en,nl,es,fr,de,ja,ko,zh-TW}.toml`.

**Non-functional**
- Cleanup runs in the foreground startup path (it's a few file operations — no need for a thread).
- Balloon uses `NIIF_INFO` (blue info icon) not `NIIF_WARNING` (yellow triangle). The existing `tray::notify` (`src/tray/mod.rs:85`) hardcodes `NIIF_WARNING` and has **2 callers** (`src/app.rs:831` usage threshold, `src/app.rs:859` token expired) — both correctly semantically "warning". Rename existing `notify` → `notify_warning`; add new sibling `notify_info`.

## Architecture

### Cleanup

Triggered from `app::run` after tray icons register but before main message loop. Implementation lives in `update::handoff::cleanup_stale_old_exes` (stub was added in phase 1).

```rust
pub fn cleanup_stale_old_exes(current_exe: &Path) {
    let Some(dir) = current_exe.parent() else { return };
    let Some(stem) = current_exe.file_name() else { return };
    let prefix = format!("{}.old.", stem.to_string_lossy());
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with(&prefix) {
            if let Err(e) = std::fs::remove_file(entry.path()) {
                log::debug!("cleanup_stale_old_exes: remove {:?} failed: {e}", entry.path());
            }
        }
    }
}
```

### Balloon notification

Parse `--updated-to` argument early (alongside `--wait-pid` in main.rs), stash it on `AppState`. After the tray icons are registered for the first time, if the version string is present, call `tray::notify_info(hwnd, kind, title, body)`.

```rust
// main.rs (after --wait-pid parse):
let updated_to = args.iter()
    .position(|a| a == "--updated-to")
    .and_then(|i| args.get(i + 1))
    .cloned();
// pass into app::run via existing arg-threading or a static OnceLock
```

```rust
// tray/mod.rs: split notify into two variants
pub fn notify_info(owner: HWND, kind: IconKind, title: &str, body: &str) {
    notify_inner(owner, kind, title, body, NIIF_INFO);
}
pub fn notify_warning(owner: HWND, kind: IconKind, title: &str, body: &str) {
    notify_inner(owner, kind, title, body, NIIF_WARNING);
}
fn notify_inner(owner: HWND, kind: IconKind, title: &str, body: &str, flags: NOTIFY_ICON_INFOTIP_FLAGS) {
    // existing body, but use `flags` instead of hardcoded NIIF_WARNING
}
```

If `tray::notify` has only one caller today (which the brainstorm scout suggested), just rename it to `notify_warning` and add `notify_info`.

## Related Code Files

- **Modify**: `src/update/handoff.rs::cleanup_stale_old_exes` (fill in phase-1 stub)
- **Modify**: `src/app.rs::run` (call cleanup; call tray::notify_info after tray registration if updated_to is set)
- **Modify**: `src/app.rs:831,859` (rename `tray::notify` → `tray::notify_warning` at both existing call sites)
- **Modify**: `src/main.rs` (parse `--updated-to`, stash for app)
- **Modify**: `src/tray/mod.rs` — rename `notify` → `notify_warning`; add `notify_info`; extract shared `notify_inner(... flags: NOTIFY_ICON_INFOTIP_FLAGS)`
- **Modify**: `src/i18n/mod.rs::LocaleStrings` — add 3 new `String` fields: `update_applied_title`, `update_applied_body`, `update_rollback_failed_body`
- **Modify**: `src/i18n/locales/{en,nl,es,fr,de,ja,ko,zh-TW}.toml` — 8 files, add the 3 new keys to each. Use machine translation for non-English where idiomatic translation unavailable; flag with comment for native-speaker review later

## Implementation Steps

1. **i18n: add 3 new fields to `LocaleStrings`** in `src/i18n/mod.rs:23-71`. After the existing `threshold_95_body` field, add:
   ```rust
   pub update_applied_title: String,
   pub update_applied_body: String,
   pub update_rollback_failed_body: String,
   ```

2. **Translate in 8 locale files** (`src/i18n/locales/{en,nl,es,fr,de,ja,ko,zh-TW}.toml`). Suggested English values (mirror style of existing `token_expired_title`/`token_expired_body`):
   ```toml
   update_applied_title = "Update applied"
   update_applied_body = "Updated to v"          # call site appends version string
   update_rollback_failed_body = "Update failed. Your original binary is saved as "  # call site appends backup path + suffix
   ```
   For non-English, machine-translate body+title, leave the trailing space/prefix structure intact. Comment in PR description: "Translations machine-generated; flagged for native-speaker review".

3. **Split `tray::notify`** (`src/tray/mod.rs:85-94`) into:
   - rename existing fn → `notify_warning` (preserves current `NIIF_WARNING` semantics)
   - extract shared `fn notify_inner(owner, kind, title, body, flags: NOTIFY_ICON_INFOTIP_FLAGS)`
   - add new `pub fn notify_info(owner, kind, title, body)` that calls `notify_inner(..., NIIF_INFO)`
   Update both existing call sites (`src/app.rs:831,859`) to call `notify_warning` instead of `notify`.

4. **Fill in `cleanup_stale_old_exes`** per architecture section.

5. **Parse `--updated-to` in main.rs**, thread it into `app::run`. Two options:
   - Pass as a new arg to `pub fn run(updated_to: Option<String>)`.
   - Store in a `OnceLock<Option<String>>` inside `update::handoff`, set in main, read in app.
   Pick the simpler one — direct function arg is preferred unless `run`'s signature is already heavily used elsewhere.

6. **In `app::run`** after tray icons register and the main window message loop is about to enter:
   ```rust
   if let Some(v) = updated_to.as_ref() {
       let strings = i18n.strings();
       let title = strings.update_applied_title.clone();
       let body = format!("{}{}", strings.update_applied_body, v);
       tray::notify_info(msg_hwnd, IconKind::ClaudeCode, &title, &body);
   }
   update::handoff::cleanup_stale_old_exes(&exe);
   ```
   Use `ClaudeCode` IconKind because it's always present when Claude is enabled (default). If user disabled Claude and enabled only Codex, fall back to Codex kind. Cheapest: try ClaudeCode first; if `tray::notify_info` fails silently, no harm.

7. **Compile check**: `cargo build --release`.

8. **Manual test cleanup**:
   - Create a file `claude-code-usage-bubble.exe.old.1234` next to the running binary.
   - Launch the app.
   - Verify the file is gone after launch.

9. **Manual test notification**:
   - Launch with `--updated-to 9.9.9` flag.
   - Verify Windows notification appears with the title + version body.
   - Verify it uses the blue info icon, not yellow warning.

## Success Criteria

- [ ] `cargo build --release` clean.
- [ ] Stale `.old.<pid>` files in install dir are removed on every startup (idempotent).
- [ ] Launching with `--updated-to vX.Y.Z` shows a tray balloon with localized title and version body.
- [ ] Balloon icon is blue info (NIIF_INFO), not yellow warning.
- [ ] Launching WITHOUT `--updated-to` shows no balloon (existing behavior preserved).
- [ ] All 8 locale TOML files (`en, nl, es, fr, de, ja, ko, zh-TW`) contain the 3 new keys (`update_applied_title`, `update_applied_body`, `update_rollback_failed_body`); `cargo build --release` would fail to deserialize otherwise.

## Risk Assessment

| Risk | Mitigation |
|---|---|
| Cleanup removes a `.old.<pid>` file that another running instance still depends on | Impossible by construction: only the spawning instance writes `.old.<pid>` and it has already exited by the time the new instance runs cleanup |
| Glob false-positive (e.g. a user-created file matching the pattern) | Pattern requires `.old.` literal AND a numeric pid-like suffix is implied; we don't pattern-match the suffix strictly. Risk is theoretical; user-created files matching `claude-code-usage-bubble.exe.old.*` is extremely unlikely |
| Tray balloon doesn't show because NIM_MODIFY runs before NIM_ADD completes | Defer the `notify_info` call by one tick (PostMessage to message loop) if testing shows races. Likely unnecessary because tray icons register synchronously |
| i18n loader is struct-field based, no template substitution at the loader level | Confirmed by Validation Session 1: append/prepend the version via Rust `format!` at the call site. TOML strings are static text fragments only |
| Translations diverge from idiomatic expression in non-English locales | Machine-translate for first cut, mark "FIXME: review by native speaker" in commit message. Subsequent crowd-sourced fixes are out of this plan's scope |
