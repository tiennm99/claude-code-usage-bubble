# Phase 1: Bootstrap Repo

## Context Links

- Source `Cargo.toml`: `/config/workspace/CodeZeno/Claude-Code-Usage-Monitor/Cargo.toml`
- Source `build.rs`: `/config/workspace/CodeZeno/Claude-Code-Usage-Monitor/build.rs`
- Source icons: `/config/workspace/CodeZeno/Claude-Code-Usage-Monitor/src/icons/`

## Overview

- **Priority:** Must-first (every other phase depends on this)
- **Status:** pending
- **Description:** Create the new repo's foundation: `Cargo.toml` with the right windows-rs features, `build.rs` for icon embedding, LICENSE (MIT, dual attribution), README with attribution to source, fresh icon assets, project layout.

## Key Insights

- Source uses `windows = 0.58` with 12 feature flags. New repo can **drop** `Win32_UI_Accessibility` (no more `SetWinEventHook` on TrayNotifyWnd) and **drop** `Win32_UI_Input_KeyboardAndMouse` if drag is handled via `WM_NCHITTEST` + `HTCAPTION` instead of manual `SetCapture`.
- Source uses `ureq` + `native-tls` + `serde` + `dirs` — all keep verbatim.
- Source release profile: `opt-level="z"`, `lto=true`, `strip=true`, `codegen-units=1`, `panic="abort"` — keep all (produces ~2MB binary).
- Package name change: `claude-code-usage-bubble` (binary name `claude-code-usage-bubble.exe`).

## Requirements

### Functional
- `cargo build --release` produces a single-file `.exe`
- `winres` embeds icon resource so the .exe has a proper Windows icon
- README clearly attributes original repo + MIT license
- Project compiles with no implementation yet (empty `main.rs` returning `()`)

### Non-functional
- Binary size target: < 3 MB stripped
- No Linux/Mac build (Windows-only `#![windows_subsystem]` in main)

## Architecture

```
claude-code-usage-bubble/
├── Cargo.toml
├── build.rs
├── LICENSE                 # MIT, with attribution clause
├── README.md               # Attribution + usage
├── src/
│   ├── main.rs             # entry, mod declarations only
│   └── icons/
│       ├── icon.ico
│       ├── 16x16.png, 32x32.png, 48x48.png, 256x256.png
│       └── *.svg sources
└── plans/                  # this directory
```

## Related Code Files

**To create:**
- `Cargo.toml`
- `build.rs`
- `LICENSE`
- `README.md`
- `src/main.rs` (stub)
- `src/icons/*` (placeholder copies from source; can be replaced with bubble-specific art later)
- `.gitignore`

**To modify:** none (greenfield repo)

## Implementation Steps

1. **Create `Cargo.toml`** with package metadata, the same `windows-rs` features minus accessibility + keyboard/mouse:
   ```toml
   [package]
   name = "claude-code-usage-bubble"
   version = "0.1.0"
   edition = "2021"
   license = "MIT"
   description = "Floating bubble showing Claude Code / Codex usage on Windows"
   repository = "<set this>"
   ```
   Features to include: `Win32_Foundation`, `Win32_Globalization`, `Win32_Graphics_Gdi`, `Win32_System_LibraryLoader`, `Win32_UI_Shell`, `Win32_UI_WindowsAndMessaging`, `Win32_System_Registry`, `Win32_System_Threading`, `Win32_Security`, `Win32_UI_HiDpi`.
2. **Create `build.rs`** mirroring source `build.rs`; embed `src/icons/icon.ico` via `winres`.
3. **Copy `src/icons/*`** from source verbatim (placeholder; designer can replace).
4. **Write `LICENSE`** — MIT text with a header line crediting CodeZeno/Claude-Code-Usage-Monitor.
5. **Write `README.md`** — short, includes:
   - One-paragraph what-it-is
   - Attribution: "This project ports usage-polling, updater, and tray-icon code from [CodeZeno/Claude-Code-Usage-Monitor](https://github.com/CodeZeno/Claude-Code-Usage-Monitor) (MIT)."
   - Install/run section (placeholder)
6. **Stub `src/main.rs`**:
   ```rust
   #![windows_subsystem = "windows"]
   fn main() {}
   ```
7. **Run `cargo build`** — must compile clean.
8. **Run `cargo build --release`** — confirm binary produced, check size.

## Todo List

- [ ] Cargo.toml with correct features
- [ ] build.rs with winres
- [ ] Icons copied from source
- [ ] LICENSE with attribution
- [ ] README.md with attribution
- [ ] src/main.rs stub
- [ ] .gitignore (target/, Cargo.lock for libs only — keep Cargo.lock for binaries)
- [ ] `cargo build` succeeds
- [ ] `cargo build --release` succeeds, binary < 3 MB

## Success Criteria

- `cargo build --release` on Windows produces a runnable .exe with embedded icon
- README links source repo
- LICENSE includes attribution clause

## Risk Assessment

- **Low.** Pure configuration; no logic.
- Cross-compile note: if developer is on Linux/Mac, will need MinGW or Windows machine for `winres` step. Document this in README.

## Security Considerations

- N/A in this phase.

## Next Steps

→ Phase 2: port portable modules into `src/`
