---
phase: 6
status: pending
estimated_hours: 7
---

# Phase 6 — Updater + remove NOTICE

## Context links

- Brainstorm: axis 8 (updater architecture)
- Source file to be REPLACED entirely: `src/updater.rs` (512 LOC)

## Overview

- **Priority:** Final — this phase removes the last copied module and drops the attribution.
- **Status:** pending
- **Brief:** Replace the source's helper-exe-handoff updater with an inline `cmd /c …` handoff (no duplicate-exe pattern). Replace `winres` legacy paths. Delete `NOTICE`, update README + LICENSE comment so the project no longer claims attribution.

## Key insights from brainstorm

- Source spawns a copy of itself as `updater-helper.exe`, which waits for the parent to exit and swaps the binary. Genuinely-different alternative: spawn `cmd.exe` directly with an inline command string that does the same dance.
- No temp `.bat` file needed — inline command via `cmd /c "..."` works.
- The `Portable` vs `Winget` channel split is kept (we may publish to winget later); the channel detection function returns `Portable` for now (already stubbed in current code).

## Requirements

### Functional

- `update::release::fetch_latest() -> Result<Option<Release>, Error>` — GitHub releases API call.
- `update::release::Release { version: Version, asset_url: String }` — parsed result.
- `update::install::begin(release: &Release) -> Result<(), Error>` — download + handoff.
- `update::install::run_cli(args: &[String]) -> Option<i32>` — handle `--apply-update` flag (still kept for parity if a user manually invokes it, even though we don't use the helper-exe path anymore).
- `update::channel::current() -> Channel` — returns `Channel::Portable` for now.
- `Version` type with parse + ordering.

### Non-functional

- Inline `cmd /c` invocation uses `CREATE_NO_WINDOW | DETACHED_PROCESS` — no console flash.
- Download timeout: 60 s. Total update apply time: < 30 s after the 2-second wait window.

## Architecture

### `src/update/mod.rs`

```rust
pub mod channel;
pub mod release;
pub mod install;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network: {0}")]
    Network(#[from] crate::net::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("no compatible release asset found")]
    NoAsset,
    #[error("install location not writable: {0}")]
    NotWritable(String),
    #[error("malformed version: {0}")]
    BadVersion(String),
}

pub use channel::{Channel, current as current_channel};
pub use release::{Release, fetch_latest};
pub use install::{begin, run_cli};
```

### `src/update/channel.rs`

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel { Portable, Winget }

pub fn current() -> Channel {
    // Until winget package exists, always Portable.
    // Future: detect by checking if current_exe path is under
    // %LOCALAPPDATA%\Microsoft\WinGet\Packages or %ProgramFiles%\WinGet\Packages.
    Channel::Portable
}
```

### `src/update/release.rs`

```rust
use crate::net::winhttp::Client;
use serde::Deserialize;

const ASSET_NAME: &str = "claude-code-usage-bubble.exe";

#[derive(Clone, Debug)]
pub struct Release {
    pub version: Version,
    pub asset_url: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version { pub major: u32, pub minor: u32, pub patch: u32 }

impl Version {
    pub fn current() -> Self { /* env!("CARGO_PKG_VERSION") */ }
    pub fn parse(s: &str) -> Option<Self> { /* … */ }
}

pub fn fetch_latest(http: &Client) -> Result<Option<Release>, super::Error> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", repo_path());
    let resp = http.get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", user_agent())
        .send()?;
    let body: GhRelease = resp.json()?;
    let candidate = Version::parse(body.tag_name.trim_start_matches('v'))
        .ok_or_else(|| super::Error::BadVersion(body.tag_name.clone()))?;
    if candidate <= Version::current() { return Ok(None); }
    let asset = body.assets.iter()
        .find(|a| a.name.eq_ignore_ascii_case(ASSET_NAME))
        .ok_or(super::Error::NoAsset)?;
    Ok(Some(Release { version: candidate, asset_url: asset.browser_download_url.clone() }))
}

#[derive(Deserialize)]
struct GhRelease { tag_name: String, assets: Vec<GhAsset> }
#[derive(Deserialize)]
struct GhAsset { name: String, browser_download_url: String }

fn repo_path() -> &'static str { "tiennm99/claude-code-usage-bubble" }
fn user_agent() -> &'static str { concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")) }
```

### `src/update/install.rs`

```rust
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x08000000;
const DETACHED_PROCESS: u32 = 0x00000008;

pub fn begin(http: &crate::net::winhttp::Client, release: &super::Release) -> Result<(), super::Error> {
    let current = std::env::current_exe()?;
    ensure_writable(&current)?;
    let staging = stage_path()?;
    std::fs::create_dir_all(staging.parent().unwrap())?;
    download(http, &release.asset_url, &staging)?;
    spawn_handoff(&staging, &current)?;
    Ok(())
}

fn download(http: &crate::net::winhttp::Client, url: &str, to: &std::path::Path) -> Result<(), super::Error> {
    let resp = http.get(url).header("User-Agent", super::release::user_agent()).send()?;
    std::fs::write(to, resp.body())?;  // assume Response exposes .body() -> &[u8]
    Ok(())
}

fn spawn_handoff(source: &std::path::Path, target: &std::path::Path) -> Result<(), super::Error> {
    let cmd = format!(
        r#"timeout /t 2 /nobreak >nul & move /y "{src}" "{tgt}" & start "" "{tgt}""#,
        src = source.to_string_lossy(),
        tgt = target.to_string_lossy(),
    );
    Command::new("cmd.exe")
        .args(["/c", &cmd])
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

pub fn run_cli(args: &[String]) -> Option<i32> {
    // Keep this for parity: if the user runs the binary with `--apply-update <target> <source> <pid>`
    // (the source's old helper signature), the inline-cmd handoff has already done the work;
    // we just exit 0.
    if args.len() >= 2 && args[1] == "--apply-update" { return Some(0); }
    None
}

fn stage_path() -> Result<PathBuf, super::Error> {
    let base = dirs::data_local_dir()
        .ok_or_else(|| super::Error::NotWritable("no data dir".into()))?;
    Ok(base.join("ClaudeCodeUsageBubble").join("updates").join("update.exe"))
}

fn ensure_writable(target: &std::path::Path) -> Result<(), super::Error> {
    let parent = target.parent().ok_or_else(|| super::Error::NotWritable("no parent".into()))?;
    let probe = parent.join(".probe");
    std::fs::write(&probe, b"").map_err(|e| super::Error::NotWritable(e.to_string()))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}
```

### Migration in `app.rs`

Replace `updater::*` imports:
- `updater::check_for_updates()` → `update::release::fetch_latest(&http_client)`
- `updater::begin_self_update(release)` → `update::install::begin(&http_client, release)`
- `updater::current_install_channel()` → `update::current_channel()`
- `updater::handle_cli_mode(args)` → `update::run_cli(args)`
- `updater::UpdateCheckResult` → `Option<Release>` (None = up to date, Some = available)
- `updater::ReleaseDescriptor` → `update::Release`
- `updater::InstallChannel` → `update::Channel`

### Drop NOTICE + final attribution cleanup

After the rewrite is complete and validated:

1. Delete `NOTICE` file.
2. Update `LICENSE`:
   - Remove the "Portions ported from …" paragraph (currently at the top of LICENSE).
   - Keep just the Apache-2.0 text with `Copyright 2026 tiennm99`.
3. Update `README.md`:
   - Replace "Differences vs upstream" section's "derivative of … with minor adaptations" wording with "inspired by [upstream link]".
   - Remove the "License" section mention of NOTICE.
4. Update `Cargo.toml`:
   - `license = "Apache-2.0"` (unchanged).

## Related code files

**To create:**
- `src/update/mod.rs`
- `src/update/channel.rs`
- `src/update/release.rs`
- `src/update/install.rs`

**To modify:**
- `src/main.rs` — declare `mod update;`; remove `mod updater;`
- `src/app.rs` — migrate `updater::*` call sites
- `LICENSE` — drop the upstream-attribution paragraph
- `README.md` — drop "derivative of" wording, replace with "inspired by"
- `Cargo.toml` — no functional changes

**To delete:**
- `src/updater.rs`
- `NOTICE`

## Implementation steps

1. **Create `update/channel.rs`** — trivial.
2. **Create `update/release.rs`** — Version type + fetch_latest. Test by hitting GitHub API.
3. **Create `update/install.rs`** — download + inline-cmd handoff. **Test on a throwaway VM** (the handoff replaces the binary, which is risky).
4. **Create `update/mod.rs`** — re-exports.
5. **Migrate `app.rs`** — replace all `updater::*` call sites.
6. **Delete `src/updater.rs`** + `mod updater;` line.
7. **`cargo build --release`** clean.
8. **End-to-end test on Windows**:
   - Stage a v0.1.1 GitHub release with a deliberately-different .exe.
   - Run v0.1.0 binary, trigger update → confirm new .exe replaces old, new app launches.
9. **AFTER end-to-end test succeeds:**
   - Delete `NOTICE`.
   - Edit `LICENSE` — drop the upstream-attribution paragraph at the top.
   - Edit `README.md` — drop "derivative of" paragraph; replace with one-line "Inspired by [CodeZeno/Claude-Code-Usage-Monitor]" (no attribution-required phrasing).
10. **Final repo audit:**
    - `grep -ri "CodeZeno" src/` → must return nothing.
    - `grep -ri "Claude-Code-Usage-Monitor" src/` → must return nothing.
    - File names: `find src -type f -name '*.rs' | xargs -I {} basename {} | sort` and compare against upstream's file list (`models.rs`, `diagnose.rs`, `theme.rs`, `poller.rs`, `updater.rs`, `tray_icon.rs`, `native_interop.rs`, `localization/*`). **No file name should match.**
    - `git log --oneline` shows the initial-port commit + 6 rewrite commits — transparent history.
11. **Commit and push:**
    - Commit message: `chore: complete clean-room rewrite; drop upstream attribution`
    - Push to GitHub.

## Todo checklist

- [ ] `update/channel.rs`
- [ ] `update/release.rs`
- [ ] `update/install.rs`
- [ ] `update/mod.rs`
- [ ] `app.rs` migrated to new updater API
- [ ] `updater.rs` deleted
- [ ] `cargo build --release` clean
- [ ] End-to-end update tested on Windows
- [ ] `NOTICE` deleted
- [ ] `LICENSE` upstream-attribution paragraph removed
- [ ] `README.md` updated to drop "derivative" wording
- [ ] Grep verifies no upstream references remain in `src/`
- [ ] File-name overlap with upstream = 0
- [ ] Final commit + push

## Success criteria

- App self-updates correctly using inline-cmd handoff.
- `NOTICE` file no longer exists in repo.
- Repo passes the "no upstream references" grep test.
- GitHub's auto-license detection still reports Apache-2.0.
- The README still credits inspiration but does not claim derivative status.

## Risks + mitigations

| Risk | Likelihood | Mitigation |
|---|---|---|
| Inline `cmd /c` flagged by antivirus | Medium | Most AVs allow `cmd.exe` execution; if flagged, fall back to a temp `.bat` |
| `move /y` fails if exe is still loaded by Windows | Medium-High | The 2s `timeout` gives parent time to exit, fully releasing file handle |
| User has unusual `cmd.exe` path | Negligible on Windows | Use full path `C:\Windows\System32\cmd.exe` if needed |
| Drop NOTICE prematurely (before phase done) | High if rushed | Phase order: rewrite first, then drop attribution. Never reorder. |
| Legal — is "inspired by" enough? | Low (we did rewrite everything) | This is the entire point of Phases 1-5. After full rewrite, no MIT code remains; attribution is courtesy, not required |

## Security considerations

- The inline `cmd /c` command string is built from `std::env::current_exe()` and `stage_path()` — both internal, no user-controlled input. No shell injection.
- The downloaded asset is over HTTPS to `api.github.com` → MITM-safe.
- Update fails closed: if `move /y` fails, the old exe is still in place; user can retry.

## Next steps

→ Project complete. Tag v0.2.0 with the clean-room rewrite as a milestone.

## Open questions

- Should we sign the binary with a code-signing certificate to satisfy AV heuristics around inline-cmd updates? Out of scope for this plan; future enhancement.
- After dropping NOTICE, should we add a small "Acknowledgements" section in README that mentions inspiration from CodeZeno's project without invoking MIT attribution language? Recommended: **yes**, that's the polite move and is legally untainting since we don't claim derivation.
