# Phase 5: Polish & Finishing

## Context Links

- All prior phases
- Source README for attribution / language list

## Overview

- **Priority:** Required before any release
- **Status:** pending
- **Description:** Multi-monitor and HiDPI verification, accessibility checks, README, license attribution, version 0.1.0 tag, optional CI workflow.

## Requirements

### Functional
- Bubble renders crisply at 100% / 125% / 150% / 175% / 200% DPI
- Bubble survives display add/remove (laptop dock/undock)
- Bubble respects monitor work area when snapping (does not overlap taskbar)
- README has install + run + uninstall sections
- LICENSE has CodeZeno attribution paragraph
- `--diagnose` flag works (writes `%TEMP%\claude-code-usage-bubble.log`)

### Non-functional
- Cargo build is fully reproducible
- No clippy warnings on `cargo clippy -- -D warnings` (or document the ones you keep)

## Architecture

No new modules.

## Related Code Files

**To modify:**
- `README.md` — fill out final content
- `LICENSE` — final attribution paragraph
- Maybe `.github/workflows/build.yml` for CI Windows build

**To delete:** none.

## Implementation Steps

1. **HiDPI manual test matrix:**
   - Windows 10 at 100% DPI: bubble visible, text readable, ring smooth
   - Windows 11 at 150%: same
   - 4K monitor at 200%: same
   - Mixed-DPI dual monitor: drag bubble between monitors → verify rescale on `WM_DPICHANGED`
2. **Multi-monitor edge tests:**
   - Snap bubble to right edge of secondary monitor → settings saved with correct coords
   - Disconnect monitor → bubble should reposition to primary monitor's work area on next start
   - Test with taskbar on top / left / right (not just default bottom)
3. **README content checklist:**
   - One-paragraph what-it-is + screenshot/gif placeholder
   - **Attribution section** (required by source MIT license):
     > This project is a derivative of [CodeZeno/Claude-Code-Usage-Monitor](https://github.com/CodeZeno/Claude-Code-Usage-Monitor) (MIT, © 2026 Code Zeno Pty Ltd). The usage-polling, updater, tray-icon, and localization modules are ported from that codebase with minor adaptations; the floating-bubble UI is original to this project.
   - Install: cargo build instructions; future winget block
   - Use: bubble + panel + tray icon described
   - Models: same content as source
   - Diagnostics: `--diagnose` flag, log path
   - Privacy: same content as source (credentials read locally, GitHub for updates)
   - License: MIT
4. **LICENSE file** — include both:
   ```
   MIT License

   Copyright (c) 2026 <your name>

   Portions of this software are derived from Claude Code Usage Monitor,
   Copyright (c) 2026 Code Zeno Pty Ltd, licensed under the MIT License.

   <rest of MIT license text>
   ```
5. **Optional CI** (`.github/workflows/build.yml`):
   - Runs `cargo fmt --check`, `cargo clippy`, `cargo build --release` on `windows-latest`
   - Uploads artifact on tag
6. **Smoke test before tagging:**
   - Run `claude-code-usage-bubble.exe`
   - Verify bubble appears with placeholder data (or real data if Claude CLI signed in)
   - Drag, snap, expand, menu, exit — all work
   - Re-launch → second instance exits silently
   - `claude-code-usage-bubble.exe --diagnose` → log file populated
7. **Tag v0.1.0** (only after the above passes):
   - `git tag v0.1.0`
   - Push to GitHub
   - Create release with the .exe artifact attached (so `updater.rs` works for future versions)

## Todo List

- [ ] HiDPI matrix tested
- [ ] Multi-monitor edge tests done
- [ ] README.md final
- [ ] LICENSE attribution finalized
- [ ] Diagnostic log verified
- [ ] Clippy clean
- [ ] CI workflow (optional)
- [ ] Smoke test green
- [ ] v0.1.0 tagged

## Success Criteria

- App can be downloaded fresh, built once, and used end-to-end
- Source repo attribution is unambiguous
- No regressions vs phases 1-4

## Risk Assessment

- **Low.** Polish phase.
- Possible regression: HiDPI bug discovered late — fix in `bubble.rs` painting code.

## Security Considerations

- Verify `updater.rs` `current_install_channel()` still returns `Portable`. Re-enabling winget detection is a future task — not part of v0.1.0.

## Next Steps

→ v0.1.0 release; future tasks (out of scope for this port):
- Winget package submission (when ready)
- Custom bubble art per model (Claude orange, Codex green) replacing inherited icons
- Optional bubble-size setting (S/M/L) in right-click menu
- Auto-hide when fullscreen apps active
