---
phase: 3
status: pending
estimated_hours: 3
---

# Phase 3 — `creds/` module (credential discovery)

## Context links

- Brainstorm: axis 4 (credential discovery) + 5 (refresh — only the discovery part lives here; orchestrator lives in Phase 4)
- Source file to be REPLACED: parts of `src/poller.rs` (credential reading/discovery), `src/native_interop.rs` (WSL command execution)

## Overview

- **Priority:** Medium — Phase 4 providers depend on this.
- **Status:** pending
- **Brief:** Introduce a `trait CredentialSource` with three impls (local Claude, WSL Claude, local Codex). Replace the source's `enum CredentialSource { Windows, Wsl }` + serial fallback with a registry-pattern + iterator.

## Key insights from brainstorm

- Trait-based discovery is structurally different from source's enum + match dispatch.
- `Vec<Box<dyn CredentialSource>>` ordered by priority lets future additions (e.g. an environment-variable-based source) drop in with no changes to the locator.
- Change-detection signatures (used by app's "watch for re-auth" loop) become a trait method.

## Requirements

### Functional

- `creds::Token { access_token: String, expires_at_unix_ms: Option<i64>, account_id: Option<String> }`.
- `trait creds::CredentialSource: Send + Sync`:
  - `fn id(&self) -> &str` — stable identifier ("local-claude", "wsl:Ubuntu-22", "codex").
  - `fn read(&self) -> Result<Token, Error>`.
  - `fn signature(&self) -> Option<String>` — opaque hash/key for change detection.
  - `fn refresh_hint(&self) -> RefreshHint` — what command to spawn for refresh.
- `creds::CredentialLocator::default_claude()` builds a locator with local Windows path first, then all installed WSL distros.
- `creds::CredentialLocator::default_codex()` builds a locator with the local Codex path.
- `locator.first_available() -> Option<&dyn CredentialSource>`.
- `locator.signatures() -> Vec<String>`.

### Non-functional

- WSL probe (which spawns `wsl.exe -l -q`) must complete in ≤ 5s or be timed out.
- WSL token-read must complete in ≤ 5s or be timed out.
- No blocking work in `signature()` (it's called frequently from the poll loop) — only stat/file-size, not file-read.

## Architecture

### `src/creds/mod.rs`

```rust
use std::time::Duration;

pub mod local_fs;
pub mod wsl_bridge;
pub mod codex_auth;

#[derive(Debug, Clone)]
pub struct Token {
    pub access_token: String,
    pub expires_at_unix_ms: Option<i64>,
    pub account_id: Option<String>,
}

#[derive(Debug, Clone)]
pub enum RefreshHint {
    LocalCliCommand { exe: &'static str },
    WslCliCommand { distro: String },
    Codex,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("credential file not found at {path}")]
    NotFound { path: String },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("missing field in credential JSON: {0}")]
    MissingField(&'static str),
    #[error("WSL command failed: {0}")]
    WslCommand(String),
    #[error("timeout waiting for WSL command")]
    WslTimeout,
}

pub trait CredentialSource: Send + Sync {
    fn id(&self) -> &str;
    fn read(&self) -> Result<Token, Error>;
    fn signature(&self) -> Option<String>;
    fn refresh_hint(&self) -> RefreshHint;
}

pub struct CredentialLocator {
    sources: Vec<Box<dyn CredentialSource>>,
}

impl CredentialLocator {
    pub fn new(sources: Vec<Box<dyn CredentialSource>>) -> Self {
        Self { sources }
    }

    pub fn default_claude() -> Self {
        let mut sources: Vec<Box<dyn CredentialSource>> = Vec::new();
        if let Some(local) = local_fs::LocalClaudeCreds::detect() {
            sources.push(Box::new(local));
        }
        for distro in wsl_bridge::list_distros() {
            sources.push(Box::new(wsl_bridge::WslClaudeCreds::new(distro)));
        }
        Self { sources }
    }

    pub fn default_codex() -> Self {
        let mut sources: Vec<Box<dyn CredentialSource>> = Vec::new();
        if let Some(codex) = codex_auth::LocalCodexCreds::detect() {
            sources.push(Box::new(codex));
        }
        Self { sources }
    }

    pub fn first_available(&self) -> Option<&dyn CredentialSource> {
        self.sources.iter().find(|s| s.signature().is_some()).map(Box::as_ref)
    }

    pub fn signatures(&self) -> Vec<String> {
        self.sources.iter().filter_map(|s| s.signature()).collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn CredentialSource> {
        self.sources.iter().map(Box::as_ref)
    }
}
```

### `src/creds/local_fs.rs`

```rust
use std::path::PathBuf;

pub struct LocalClaudeCreds {
    path: PathBuf,
    id: String,
}

impl LocalClaudeCreds {
    pub fn detect() -> Option<Self> {
        let home = dirs::home_dir()?;
        let path = home.join(".claude").join(".credentials.json");
        Some(Self { id: format!("local:{}", path.display()), path })
    }
}

impl super::CredentialSource for LocalClaudeCreds {
    fn id(&self) -> &str { &self.id }
    fn read(&self) -> Result<super::Token, super::Error> {
        let content = std::fs::read_to_string(&self.path)?;
        parse_claude_json(&content)
    }
    fn signature(&self) -> Option<String> {
        let meta = std::fs::metadata(&self.path).ok()?;
        let modified = meta.modified().ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs()).unwrap_or(0);
        Some(format!("{}|{}|{}", self.id, meta.len(), modified))
    }
    fn refresh_hint(&self) -> super::RefreshHint {
        super::RefreshHint::LocalCliCommand { exe: "claude" }
    }
}

pub fn parse_claude_json(content: &str) -> Result<super::Token, super::Error> {
    let json: serde_json::Value = serde_json::from_str(content)?;
    let oauth = json.get("claudeAiOauth")
        .ok_or(super::Error::MissingField("claudeAiOauth"))?;
    let access_token = oauth.get("accessToken")
        .and_then(|v| v.as_str())
        .ok_or(super::Error::MissingField("accessToken"))?
        .to_string();
    let expires_at_unix_ms = oauth.get("expiresAt").and_then(|v| v.as_i64());
    Ok(super::Token { access_token, expires_at_unix_ms, account_id: None })
}
```

### `src/creds/wsl_bridge.rs`

Spawns `wsl.exe -l -q` to enumerate distros, then per-distro spawns `wsl.exe -d <distro> -- sh -lc 'cat ~/.claude/.credentials.json'`. Includes UTF-16LE-aware text decoder for `wsl.exe -l -q` output. Uses `CREATE_NO_WINDOW` flag. 5s timeout per command via a `run_with_timeout` helper.

Key public types:
- `pub fn list_distros() -> Vec<String>` — empty Vec if WSL not installed.
- `pub struct WslClaudeCreds { distro: String, id: String }` implementing `CredentialSource`.

### `src/creds/codex_auth.rs`

Reads `$CODEX_HOME/auth.json` or `~/.codex/auth.json`. Token includes `account_id` from `tokens.account_id`. Mirrors `local_fs.rs` pattern.

## Related code files

**To create:**
- `src/creds/mod.rs`
- `src/creds/local_fs.rs`
- `src/creds/wsl_bridge.rs`
- `src/creds/codex_auth.rs`

**To modify:**
- `src/main.rs` — `mod creds;`

**To delete:** nothing (Phase 4 deletes `src/poller.rs` once `usage::*` providers go live).

## Implementation steps

1. Create `src/creds/mod.rs` with trait + `Token` + `Error` + `RefreshHint` + `CredentialLocator`.
2. Create `local_fs.rs` with `LocalClaudeCreds`.
3. Create `wsl_bridge.rs` with distro enumeration + per-distro creds source. Note `decode_wsl_text` handles the UTF-16LE encoding quirk on `wsl.exe -l -q`.
4. Create `codex_auth.rs` with `LocalCodexCreds`.
5. Wire `mod creds;` into `main.rs`.
6. `cargo build --release` clean.

## Todo checklist

- [ ] `creds/mod.rs`
- [ ] `creds/local_fs.rs`
- [ ] `creds/wsl_bridge.rs`
- [ ] `creds/codex_auth.rs`
- [ ] `main.rs` declares module
- [ ] `cargo build --release` clean
- [ ] Manual test (Windows): `CredentialLocator::default_claude().first_available()` finds a real credential file

## Success criteria

- Trait dispatch works; locator returns the right source based on priority.
- WSL probe doesn't hang the process when no WSL is installed.
- `signature()` is fast (<1 ms for local, <100 ms for WSL).

## Risks + mitigations

| Risk | Likelihood | Mitigation |
|---|---|---|
| WSL probe blocks for full 5s when WSL is uninstalled | Low | Test on a WSL-free VM; verify timeout works |
| UTF-16LE detection heuristic produces false positives | Low | Source has same heuristic and ships in production |
| `wsl.exe` not on PATH | Negligible on Win10+ | Return empty list silently |
| `dirs::home_dir()` returns None | Negligible on Windows | Return `None` from `detect()` and let locator skip |

## Security considerations

- Tokens stored as `String` in memory; not logged.
- WSL `sh -lc` arg is a constant — no user-controlled input → no shell injection.
- Don't `log::debug!` the token; log only `token len=N`.

## Next steps

→ Phase 4: providers + refresh orchestrator that USES this locator.
