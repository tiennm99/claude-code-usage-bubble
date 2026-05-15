# Phase 2: Port Portable Modules

## Context Links

- Source: `/config/workspace/CodeZeno/Claude-Code-Usage-Monitor/src/`
- Modules to port verbatim (or near-verbatim): `models.rs`, `diagnose.rs`, `theme.rs`, `poller.rs`, `updater.rs`, `tray_icon.rs`, `localization/*`
- Module to trim: `native_interop.rs`

## Overview

- **Priority:** High (must precede phases 3-4)
- **Status:** pending
- **Description:** Bring over the portable subsystems from the source repo with minimal changes. These represent ~2,700 lines of working, tested code — the goal is to keep them intact and only edit what's required for the new project name and the simplified Win32 surface.

## Key Insights

- `poller.rs` (1099 lines) is fully self-contained — depends only on `serde`, `ureq`, `native-tls`, `dirs`, plus `diagnose` and `models` from the same crate.
- `updater.rs` (510 lines) embeds `env!("CARGO_PKG_REPOSITORY")` to resolve GitHub owner/repo automatically — no hardcoded references.
- `tray_icon.rs` uses `WM_APP_TRAY` (`WM_APP + 3`) and `IDM_TOGGLE_WIDGET = 50`. Keep the constants; `app.rs` (phase 4) will own dispatch.
- `native_interop.rs` is the trimming target: drop `find_taskbar`, `find_child_window`, `get_taskbar_rect`, `embed_in_taskbar`, `set_tray_event_hook`, `get_window_thread_id`, `unhook_win_event`. Keep `wide_str`, `colorref`, `Color`, timer-ID constants, custom-message constants.

## Requirements

### Functional
- `mod models; mod diagnose; mod theme; mod poller; mod updater; mod tray_icon; mod localization; mod native_interop;` all compile against current `main.rs`
- `cargo check` passes with zero warnings beyond unused-symbol warnings (which will resolve in phases 3-4)
- All `pub` symbols documented above remain reachable

### Non-functional
- No behavioral changes vs source — diffs limited to module boundaries

## Architecture

```
src/
├── main.rs                 (stub from phase 1)
├── models.rs               COPIED
├── diagnose.rs             COPIED
├── theme.rs                COPIED
├── poller.rs               COPIED
├── updater.rs              COPIED + 1-line stub for current_install_channel
├── tray_icon.rs            COPIED
├── native_interop.rs       TRIMMED (~80 lines vs source 179)
└── localization/
    ├── mod.rs              COPIED
    ├── english.rs          COPIED
    ├── dutch.rs            COPIED
    ├── french.rs           COPIED
    ├── german.rs           COPIED
    ├── japanese.rs         COPIED
    ├── korean.rs           COPIED
    ├── spanish.rs          COPIED
    └── traditional_chinese.rs  COPIED
```

## Related Code Files

**To create (copy from source):**
- All files listed in Architecture section above.

**To modify after copying:**
- `src/updater.rs` — replace `current_install_channel()` body with `InstallChannel::Portable` while keeping the rest of the function intact (preserves code for future winget enablement).
- `src/native_interop.rs` — delete taskbar/WinEvent functions and their imports.
- `src/main.rs` — declare modules; do NOT yet call `window::run` (window module doesn't exist yet).

**To delete:** none.

## Implementation Steps

1. **Copy modules verbatim:**
   ```
   cp -r ../Claude-Code-Usage-Monitor/src/{models,diagnose,theme,poller,updater,tray_icon}.rs src/
   cp -r ../Claude-Code-Usage-Monitor/src/localization src/
   ```
2. **Copy & trim `native_interop.rs`:**
   - Keep: `wide_str`, `colorref`, `Color`, `TIMER_*` constants, `WM_APP_*` constants, `get_window_rect_safe`, `move_window`
   - Delete: `WS_POPUP_STYLE`, `WS_CHILD_STYLE`, `WS_CLIPSIBLINGS_STYLE` (bubble uses standard windows-rs constants), `EVENT_OBJECT_LOCATIONCHANGE`, `WINEVENT_OUTOFCONTEXT`, `find_taskbar`, `find_child_window`, `get_taskbar_rect`, `embed_in_taskbar`, `set_tray_event_hook`, `get_window_thread_id`, `unhook_win_event`
   - Drop imports for `Accessibility`, `Shell::SHAppBarMessage`/`APPBARDATA`, `Foundation::RECT` (if no longer used after trim)
3. **Stub winget detection in `updater.rs`:**
   ```rust
   pub fn current_install_channel() -> InstallChannel {
       // Bubble repo is not yet published to winget; once it is, restore the
       // is_winget_install_path probe by reading the source repo's logic.
       InstallChannel::Portable
   }
   ```
   Keep `is_winget_install_path` + `winget_install_roots` + `normalize_path` as `#[allow(dead_code)]` to preserve the code path.
4. **Update `src/main.rs`** to declare the modules:
   ```rust
   #![windows_subsystem = "windows"]
   mod diagnose; mod localization; mod models; mod native_interop;
   mod poller; mod theme; mod tray_icon; mod updater;
   fn main() { /* phase-04 will wire this */ }
   ```
5. **Run `cargo check`** — expect warnings about unused public items; should be zero errors.

## Todo List

- [ ] Copy 7 source modules + localization directory
- [ ] Trim `native_interop.rs` to ~80 lines (drop taskbar/WinEvent helpers)
- [ ] Stub `updater::current_install_channel`
- [ ] Wire `mod` declarations in `main.rs`
- [ ] `cargo check` clean (only dead-code warnings)
- [ ] `cargo build --release` still produces a binary

## Success Criteria

- All ported modules compile in the new crate without modification beyond what is listed above
- No warnings about missing imports
- Source code license headers (if any) are preserved

## Risk Assessment

- **Low.** Copy-with-rename operation; the trimming of `native_interop.rs` is the only judgment call.
- Edge case: `tray_icon.rs` imports `crate::native_interop::WM_APP_TRAY` — verify constant survives the trim.

## Security Considerations

- `poller.rs` reads OAuth credentials from `~/.claude/.credentials.json` and (optionally) WSL distros. No new attack surface vs source.
- `updater.rs` downloads .exe from GitHub. Same trust model as source. Until the new repo has releases published, this code is dormant.

## Next Steps

→ Phase 3: build the bubble window (replaces 2847 lines of `window.rs`)
