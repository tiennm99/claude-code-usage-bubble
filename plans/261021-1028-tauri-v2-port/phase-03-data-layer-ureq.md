---
phase: 3
title: "Data layer on ureq"
status: completed
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
  - Env proxies: `net::env_proxy()` walks the variables ureq reads (`ALL_PROXY`, `HTTPS_PROXY`, `HTTP_PROXY`, either case) and keeps the first valid one, in ureq's order. ureq's `socks-proxy` feature is on, so a SOCKS entry such as Clash's `ALL_PROXY=socks5h://…` is used, not skipped. `NO_PROXY` applies to the chosen proxy.
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

## Todo

- [x] `src/net/mod.rs` on ureq: same `Client`/`RequestBuilder`/`Response` surface, `Transport(String)` replaces `Win`, `http_status_as_error(false)`, connect 60 s / receive 30 s / global 120 s, gzip, rustls with the platform verifier, `claude-code-usage-bubble/<version>` User-Agent with per-request override
- [x] Proxy: env proxies from ureq; `cfg(windows)` `platform::windows::system_proxy()` reads `ProxyEnable`/`ProxyServer`/`ProxyOverride` only when no env proxy is set; `core::proxy` parses per-scheme and bare servers, `;`-split override globs and `<local>`; `Client` holds a proxied and a direct agent and picks per request
- [x] `refresh.rs`: Windows candidates and `CREATE_NO_WINDOW` behind `cfg(windows)`; Unix `claude -p .` / `codex exec .` searched in `PATH`, `~/.local/bin`, `~/.claude/local`, `/opt/homebrew/bin`, `/usr/local/bin`; child reaped on a detached thread
- [x] `usage/mod.rs` restored (providers, registry, headers, refresh, `Error`, `UsageProvider`); `src/lib.rs` declares `creds`, `net` and `platform`
- [x] Test asserting `LIVE_USAGE` excludes OpenCode Go
- [x] Deleted `src/net/winhttp.rs`
- [x] Fixture tests in `tests/fixtures/*.json`: OAuth usage (full, partial, null buckets, null utilization), rate-limit headers (fractions, missing, negative, a 429 over loopback), ChatGPT envelope (null `rate_limit`, null windows, negative `reset_at`), ISO 8601 (`+0000`, lowercase `t`, before 1970), credential JSON (missing fields, float `expiresAt`, null `tokens`, empty `CODEX_HOME`), proxy strings
- [x] `#[ignore]` `live_poll` smoke test, run by hand on the host: both providers returned data
- [x] Host: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (112 passed, 1 ignored), `npm run lint`, `npm test`, `npm run tauri build -- --no-bundle`
- [x] CI Windows, Linux and macOS rows green, including the Windows proxy reader

## Implementation notes

- The registry reader uses `windows-registry` 0.6, the crate and major that
  `auto-launch` already pulls, so no new crate is compiled and no `unsafe`
  registry code is needed.
- ureq always tunnels through an HTTP proxy with `CONNECT`, also for `http://`
  targets. The loopback routing test asserts the `CONNECT` line instead of a
  proxied reply.
- `net::Error::Status` is kept so the providers compile unchanged; the client
  never builds it, because every status comes back as a `Response`.
- `net::Client::new` consults `net::system_proxy()` only when
  `net::env_proxy()` finds no usable proxy. SOCKS env proxies go through
  ureq's `socks-proxy` connector; without that feature ureq fails every
  request for `socks5h` and silently connects directly for `socks5`. The
  system proxy stays HTTP/HTTPS only. `Client::system_proxy()`
  exposes the system proxy in use for the updater
  builder in Phase 09. Proxy URLs are never logged (they may hold
  credentials).
- An empty `CODEX_HOME` now counts as unset, as in the Codex CLI. Before, it
  resolved to `auth.json` in the working directory.
- A float `expiresAt` is truncated to whole milliseconds instead of being
  treated as "no expiry".
- `anthropic::parse_usage_body` and `codex_auth::{auth_path, parse_codex_json}`
  were split out of the I/O paths so the fixtures test them directly; the
  behaviour is unchanged.
- The README note that PAC/WPAD is no longer evaluated belongs to the README
  rewrite in Phase 11; this phase does not own the README.

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
