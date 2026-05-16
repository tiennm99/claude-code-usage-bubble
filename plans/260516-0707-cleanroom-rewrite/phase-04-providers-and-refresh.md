---
phase: 4
status: pending
estimated_hours: 8
---

# Phase 4 — Providers & refresh orchestrator

## Context links

- Brainstorm: axes 3 (provider trait) + 5 (refresh)
- Source file to be REPLACED entirely: `src/poller.rs` (~1100 LOC)
- Phase deps: 1 (`net::winhttp`), 2 (`usage::types`), 3 (`creds`)

## Overview

- **Priority:** Critical — replaces the largest single source file.
- **Status:** pending
- **Brief:** Implement `ClaudeProvider` + `ChatGptProvider` against the trait from Phase 2, plus the `RefreshOrchestrator` that spawns local CLIs to refresh expired tokens. Replace `crate::poller::*` calls in `app.rs` with `usage::registry::poll_all`. End of phase: `src/poller.rs` is gone.

## Key insights from brainstorm

- The two providers share rate-limit-header parsing logic — extract to `usage::headers`.
- Anthropic's primary endpoint returns the dedicated `oauth/usage` JSON; fallback is the Messages API with rate-limit headers. Both code paths go in `ClaudeProvider`.
- ChatGPT's `wham/usage` endpoint is shaped differently (`rate_limit.primary_window.used_percent`) — separate parser.
- `RefreshOrchestrator::refresh(source)` uses `RefreshHint` to know which CLI to spawn. 8-second timeout, not 30 — UX wins.

## Requirements

### Functional

- `ClaudeProvider::new(locator: CredentialLocator) -> Self`.
- `ClaudeProvider::poll(http) -> Result<UsageWindows, usage::Error>`:
  - Try `GET https://api.anthropic.com/api/oauth/usage` with `Authorization: Bearer …` + `anthropic-beta: oauth-2025-04-20`.
  - If primary returns 401/403 → `usage::Error::AuthRequired`.
  - If primary returns 2xx but data is incomplete → fall back to Messages API.
  - Messages-API fallback: `POST https://api.anthropic.com/v1/messages` with minimal payload; parse `anthropic-ratelimit-unified-{5h,7d}-utilization` headers + reset timestamps.
- `ChatGptProvider::new(locator: CredentialLocator) -> Self`.
- `ChatGptProvider::poll(http) -> Result<UsageWindows, usage::Error>`:
  - `GET https://chatgpt.com/backend-api/wham/usage` with `Authorization: Bearer …` + `User-Agent: codex-cli` + optional `ChatGPT-Account-Id`.
  - Parse `rate_limit.{primary_window,secondary_window}.used_percent` + `.reset_at` (Unix seconds).
- `RefreshOrchestrator::new(timeout: Duration) -> Self`.
- `RefreshOrchestrator::refresh(source: &dyn CredentialSource) -> RefreshOutcome` — spawns appropriate CLI, waits up to timeout, returns outcome.
- `usage::registry::Registry`:
  - `Registry::new()` builds with default providers.
  - `registry.enabled_providers(settings) -> Vec<ProviderId>`.
  - `registry.poll_one(id, http) -> Result<UsageWindows, usage::Error>`.

### Non-functional

- Total poll time (both providers) must stay under 60s even with refresh attempts.
- Refresh timeout is 8s (down from source's 30s) — verify UX feels snappy.
- HTTP retries are NOT done at this layer (app retains the retry/backoff loop).

## Architecture

### `src/usage/mod.rs` (expanded)

```rust
pub mod types;
pub mod headers;
pub mod anthropic;
pub mod chatgpt;
pub mod refresh;
pub mod registry;

pub use types::{ProviderId, Window, UsageWindows, ProviderSnapshot};

pub trait UsageProvider: Send {
    fn id(&self) -> ProviderId;
    fn poll(&mut self, http: &crate::net::winhttp::Client) -> Result<UsageWindows, Error>;
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("authentication required")]
    AuthRequired,
    #[error("no credentials configured")]
    NoCredentials,
    #[error("token expired after refresh")]
    TokenExpired,
    #[error("network: {0}")]
    Network(#[from] crate::net::Error),
    #[error("response shape mismatch: {0}")]
    BadResponse(String),
    #[error("credential: {0}")]
    Creds(#[from] crate::creds::Error),
}
```

### `src/usage/headers.rs`

```rust
use super::{Window, UsageWindows};
use crate::net::winhttp::Response;

pub fn parse_anthropic_rate_limit(resp: &Response) -> UsageWindows {
    let primary = Window {
        utilization: header_f64(resp, "anthropic-ratelimit-unified-5h-utilization") * 100.0,
        resets_at: unix_to_system_time(header_i64(resp, "anthropic-ratelimit-unified-5h-reset")),
    };
    let secondary = Window {
        utilization: header_f64(resp, "anthropic-ratelimit-unified-7d-utilization") * 100.0,
        resets_at: unix_to_system_time(header_i64(resp, "anthropic-ratelimit-unified-7d-reset")),
    };
    UsageWindows { primary, secondary }
}

fn header_f64(resp: &Response, name: &str) -> f64 {
    resp.header(name).and_then(|s| s.parse().ok()).unwrap_or(0.0)
}
fn header_i64(resp: &Response, name: &str) -> Option<i64> {
    resp.header(name).and_then(|s| s.parse().ok())
}
fn unix_to_system_time(secs: Option<i64>) -> Option<std::time::SystemTime> {
    let s = secs?;
    if s < 0 { return None; }
    Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(s as u64))
}
```

### `src/usage/anthropic.rs`

```rust
use crate::creds::CredentialLocator;
use crate::net::winhttp::Client;
use super::{UsageProvider, UsageWindows, Window, Error, ProviderId};
use serde::Deserialize;

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";

pub struct ClaudeProvider {
    locator: CredentialLocator,
}

impl ClaudeProvider {
    pub fn new(locator: CredentialLocator) -> Self { Self { locator } }
}

impl UsageProvider for ClaudeProvider {
    fn id(&self) -> ProviderId { ProviderId::Claude }

    fn poll(&mut self, http: &Client) -> Result<UsageWindows, Error> {
        let source = self.locator.first_available().ok_or(Error::NoCredentials)?;
        let token = source.read()?;
        // … (try usage endpoint; fall back to messages; parse rate-limit headers)
    }
}

#[derive(Deserialize)]
struct OauthUsageResponse {
    five_hour: Option<Bucket>,
    seven_day: Option<Bucket>,
}
#[derive(Deserialize)]
struct Bucket {
    utilization: f64,
    resets_at: Option<String>,  // ISO 8601
}

fn try_usage_endpoint(http: &Client, token: &str) -> Result<Option<UsageWindows>, Error> { /* … */ }
fn try_messages_endpoint(http: &Client, token: &str) -> Result<UsageWindows, Error> { /* … */ }
fn parse_iso8601(s: &str) -> Option<std::time::SystemTime> { /* … minimal date parser, same as source's */ }
```

### `src/usage/chatgpt.rs`

Mirrors anthropic shape; parses Codex JSON.

### `src/usage/refresh.rs`

```rust
use crate::creds::{CredentialSource, RefreshHint};
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub enum RefreshOutcome { Refreshed, StillExpired, CliMissing, Timeout }

pub struct RefreshOrchestrator { timeout: Duration }

impl RefreshOrchestrator {
    pub fn new(timeout: Duration) -> Self { Self { timeout } }

    pub fn refresh(&self, source: &dyn CredentialSource) -> RefreshOutcome {
        let signature_before = source.signature();
        let hint = source.refresh_hint();
        let spawn_ok = match hint {
            RefreshHint::LocalCliCommand { exe } => self.spawn_local(exe),
            RefreshHint::WslCliCommand { distro } => self.spawn_wsl(&distro),
            RefreshHint::Codex => self.spawn_codex(),
        };
        if !spawn_ok { return RefreshOutcome::CliMissing; }

        let start = std::time::Instant::now();
        loop {
            if start.elapsed() > self.timeout { return RefreshOutcome::Timeout; }
            std::thread::sleep(Duration::from_millis(500));
            if source.signature() != signature_before {
                return RefreshOutcome::Refreshed;
            }
        }
    }

    fn spawn_local(&self, exe: &str) -> bool { /* spawn `<exe>.cmd -p .` or `<exe> -p .` */ }
    fn spawn_wsl(&self, distro: &str) -> bool { /* wsl.exe -d <d> -- bash -lic 'claude -p .' */ }
    fn spawn_codex(&self) -> bool { /* spawn codex exec . */ }
}
```

### `src/usage/registry.rs`

```rust
use super::{UsageProvider, ProviderId, UsageWindows, Error};
use crate::net::winhttp::Client;
use crate::settings::Settings;

pub struct Registry {
    providers: Vec<Box<dyn UsageProvider>>,
}

impl Registry {
    pub fn with_defaults() -> Self {
        let claude_locator = crate::creds::CredentialLocator::default_claude();
        let codex_locator = crate::creds::CredentialLocator::default_codex();
        Self {
            providers: vec![
                Box::new(super::anthropic::ClaudeProvider::new(claude_locator)),
                Box::new(super::chatgpt::ChatGptProvider::new(codex_locator)),
            ],
        }
    }

    pub fn poll_enabled(&mut self, http: &Client, settings: &Settings) -> Vec<(ProviderId, Result<UsageWindows, Error>)> {
        let mut results = Vec::new();
        for p in self.providers.iter_mut() {
            let enabled = match p.id() {
                ProviderId::Claude => settings.show_claude_code,
                ProviderId::ChatGpt => settings.show_codex,
            };
            if !enabled { continue; }
            results.push((p.id(), p.poll(http)));
        }
        results
    }
}
```

### `app.rs` migration

Replace `poller::poll(show_claude, show_codex)` with `registry.poll_enabled(&http_client, &settings)`. App now holds:
- `http_client: net::winhttp::Client`
- `registry: usage::registry::Registry`
- `refresh: usage::refresh::RefreshOrchestrator`

Polling thread flow:
1. `let results = registry.poll_enabled(http, settings);`
2. For each `(id, Err(AuthRequired))`, call `refresh.refresh(source)` — needs locator access; expose via provider trait `fn try_refresh(orchestrator: &Orchestrator) -> RefreshOutcome` OR pass locator to app.
3. Post `WM_APP_USAGE_UPDATED`.

(Detail: simplest is for the provider to expose its locator: `fn locator(&self) -> &CredentialLocator;` but that's leaky. Alternative: provider has an internal `fn refresh(&self, orch) -> RefreshOutcome` that owns the locator-access. Implement option B.)

## Related code files

**To create:**
- `src/usage/headers.rs`
- `src/usage/anthropic.rs`
- `src/usage/chatgpt.rs`
- `src/usage/refresh.rs`
- `src/usage/registry.rs`

**To modify:**
- `src/usage/mod.rs` — expand with new module declarations
- `src/app.rs` — migrate poll-thread logic from `poller::*` to `registry::*` + `refresh::*`; remove `crate::poller` import
- `src/main.rs` — remove `mod poller;`

**To delete:**
- `src/poller.rs` (1099 LOC removed)

## Implementation steps

1. **Implement `headers.rs`** — pure parsing, test in isolation.
2. **Implement `anthropic.rs`** in two parts:
   - 2a. `try_usage_endpoint` — full JSON parse path.
   - 2b. `try_messages_endpoint` — POST with model fallback chain + header parsing.
3. **Implement `chatgpt.rs`** — single endpoint, JSON parse.
4. **Implement `refresh.rs`** — orchestrator with 3 spawn paths.
5. **Implement `registry.rs`** — registry + `poll_enabled`.
6. **Migrate `app.rs::handle_poll_result`** to consume `Vec<(ProviderId, Result<UsageWindows, Error>)>` instead of `Result<AppUsageData, PollError>`.
7. **Migrate `app.rs::apply_data`** to update `Vec<ProviderSnapshot>` per provider.
8. **Add `fn try_refresh_for_provider(&mut self, id: ProviderId, orch: &Orchestrator) -> RefreshOutcome`** to `Registry`, so app can request refresh without touching internals.
9. **Wire `RefreshOrchestrator::new(Duration::from_secs(8))`** into app state.
10. **Delete `src/poller.rs`** + `mod poller;` line.
11. **`cargo build --release`** clean.
12. **End-to-end test on Windows**:
    - Run app, sign-in via existing Claude CLI session, see polling work.
    - Force token expiry (delete credentials file), see refresh succeed.
    - Disconnect network, see graceful degradation.

## Todo checklist

- [ ] `headers.rs`
- [ ] `anthropic.rs` (usage endpoint)
- [ ] `anthropic.rs` (messages fallback)
- [ ] `chatgpt.rs`
- [ ] `refresh.rs`
- [ ] `registry.rs`
- [ ] `app.rs` poll-thread + apply_data migration
- [ ] `poller.rs` deleted
- [ ] `main.rs` updated
- [ ] `cargo build --release` clean
- [ ] Manual Windows e2e: Claude polls, Codex polls, token-refresh works
- [ ] Manual Windows e2e: network down → "..." indicator; back online → recovers

## Success criteria

- `src/poller.rs` no longer exists.
- App polls both providers concurrently (in poll thread).
- Token-expired flow refreshes within 8 s (or shows "..." gracefully if CLI missing).
- All ISO 8601 + Unix timestamps parse correctly (test edge cases: end-of-day, leap years).

## Risks + mitigations

| Risk | Likelihood | Mitigation |
|---|---|---|
| Anthropic API shape changes between dev and ship | Low | Test against live API; pin `anthropic-version: 2023-06-01` |
| Codex endpoint changes auth header | Low | Match source's header set exactly: Bearer + User-Agent + optional ChatGPT-Account-Id |
| Refresh races multiple poll attempts | Medium | Single refresh per source per poll cycle; signature-based completion detection |
| `wsl.exe bash -lic 'claude -p .'` outputs to TTY when no -p flag is recognized in WSL claude version | Medium | Test against actual installed Claude CLI in WSL; consider `--no-prompt` alternative |
| Long-running Messages API request | Medium | 30 s HTTP timeout in `net::winhttp::Client` |

## Security considerations

- Bearer token is included in HTTPS request → WinHTTP encrypts with TLS.
- Token never logged at INFO level; only `len=N` at DEBUG.
- CLI refresh spawns process with `CREATE_NO_WINDOW` to avoid console flash.

## Next steps

→ Phase 5: replace `tray_icon.rs` with `tray/` directory and tiny-skia badges.

## Open questions

- Does the Anthropic OAuth usage endpoint return `seven_day.utilization` consistently or do we still need the messages fallback for 7d data? Source code says yes-fallback-sometimes-needed. Keep the fallback for safety.
- Should `ChatGptProvider` skip the request if it has no `account_id` to avoid wasting bandwidth on a guaranteed-401? Source includes the header conditionally; we mirror that.
