---
phase: 3
title: "Data layer on ureq"
status: pending
priority: P1
effort: "4h"
dependencies: [2]
---

# Phase 03: Data Layer On ureq

## Context

The providers (`src/usage/anthropic.rs`, `chatgpt.rs`, `headers.rs`,
`registry.rs`, `refresh.rs`) and creds (`src/creds/*`) are portable. They have
only two Win32 couplings: `src/net/winhttp.rs`, and the `CommandExt` import in
`src/usage/refresh.rs`. None of these modules has tests.

## Requirements

- `src/net/mod.rs` keeps the surface the providers use: `Client::new(ua)`,
  `get`/`post`, `header`, `json_body`, `send`, and
  `Response::{status, header, body, text, json}`. Swap the `Win` error variant
  for `Transport(String)`.
- The ureq agent:
  - `http_status_as_error(false)`.
  - Timeouts: connect 60 s, receive 30 s, global 120 s.
  - gzip enabled.
  - TLS is rustls with the platform verifier, so corporate roots stay trusted.
  - User-Agent `claude-code-usage-bubble/<version>`.
  - A per-request User-Agent replaces the agent's, as the Codex provider requires.
- Proxy:
  - Env proxies come from ureq.
  - `cfg(windows)`, only when no env proxy is set: `platform::windows::system_proxy()` reads `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings` (`ProxyEnable`, `ProxyServer`, `ProxyOverride`). Nothing when `ProxyEnable` is not 1.
  - Parsing, a pure function in `core` with tests: a `ProxyServer` of the form `http=h:p;https=h:p;…` uses the `https=` entry, else the `http=` entry; a bare `h:p` applies to all schemes. `ProxyOverride` is split on `;`; each entry is a case-insensitive glob (`*` wildcard) against the host, and `<local>` matches dotless hosts.
  - `net::Client` holds a proxied agent and a direct agent and picks per request: direct when the request host matches a `ProxyOverride` entry. The parsed proxy URL is also handed to the updater builder in Phase 09.
- `refresh.rs`:
  - `cfg(windows)` keeps today's candidate list and `CREATE_NO_WINDOW`.
  - Unix tries `claude -p .` and `codex exec .`, searching `PATH` plus `~/.local/bin`, `~/.claude/local`, `/opt/homebrew/bin` and `/usr/local/bin`.
  - The child is reaped on a detached thread, so no zombies are left.
- Keep `OpenCodeGo` non-live. `unreachable!()` must stay unreachable, so add a test asserting that `LIVE_USAGE` excludes it.
- Never log tokens or request headers.

## Files

- Modify: `src/net/mod.rs`, `src/usage/mod.rs` (restore), `src/usage/refresh.rs`, `src/usage/types.rs` (serde already added), `src/lib.rs` (`pub mod net; pub mod creds; pub mod platform;`), `src/core/mod.rs` (`proxy` parsing)
- Create: `src/platform/mod.rs`, `src/platform/windows.rs` (only `system_proxy` for now; later phases append to it), `tests/fixtures/*.json`
- Delete: `src/net/winhttp.rs`

## Steps

1. Implement the ureq-backed `net`, then compile the providers unchanged.
2. Add fixture tests:
   - OAuth usage JSON: full, partial, null buckets, and null utilization (which goes to the fallback).
   - Rate-limit headers: fractions, missing values, negative resets.
   - The ChatGPT envelope: null `rate_limit`, null windows, negative `reset_at`.
   - ISO 8601 edge cases: `+0000` rejected, lowercase `t` rejected, years before 1970.
   - Credential JSON: missing fields, a float `expiresAt`, null `tokens`, and an empty `CODEX_HOME`.
   - Proxy strings: per-scheme with and without `https=`, bare `h:p`, and `ProxyOverride` matches for `*.anthropic.com`, `chatgpt.com` and `<local>`.
3. Add a `#[ignore]` live smoke test that polls the real providers when credentials exist. Run it by hand.

## Validation

- Host: `cargo test usage:: creds:: net::` is green.
- Host: `cargo test -- --ignored live_poll` returns data or `NoCredentials`, and never panics.
- CI Windows: compiles with the proxy reader.

## Risks

| Risk | L x I | Mitigation |
|------|-------|------------|
| ureq returns 4xx/5xx as errors, which breaks the 401/403 handling and the Messages fallback | H x H | `http_status_as_error(false)`, plus a test against a fixture 429 response that carries headers. |
| Users behind PAC lose data | L x M | Intentional change, documented in the README. The static proxy, its bypass list and env proxies still work. |
| macOS has no credential file | H x L | Out of scope (keyring is deferred). macOS shows `…`. |

## Rollback

Revert the commits. The WinHTTP file is recoverable from git.
