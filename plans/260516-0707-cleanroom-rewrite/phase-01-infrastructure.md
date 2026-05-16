---
phase: 1
status: pending
estimated_hours: 9
---

# Phase 1 — Infrastructure

## Context links

- Brainstorm: [`../reports/brainstorm-260516-0707-cleanroom-reimplementation.md`](../reports/brainstorm-260516-0707-cleanroom-reimplementation.md) (axes 1, 2, 9, 10 + os/)
- Source files to be REPLACED later: `src/diagnose.rs`, `src/theme.rs`, `src/native_interop.rs` (do not delete this phase — Phase 2+ depends on them until then)

## Overview

- **Priority:** Critical (every other phase depends on these primitives)
- **Status:** pending
- **Brief:** Stand up the foundation modules — logging, Win32 helpers, WinHTTP client, and the build-script swap — without touching any business logic. End of this phase: `cargo build --release` succeeds with the new modules compiled in but not yet referenced by `app.rs` (so behavior is unchanged).

## Key insights from brainstorm

- WinHTTP via `windows-rs::Win32::Networking::WinHttp` removes `ureq` + `native-tls` (~600 KB binary savings) and respects the system proxy automatically.
- WinHTTP is verbose. Encapsulate in a single ~150 LOC `net::winhttp::Client` so the rest of the codebase stays clean.
- `log` + `simplelog` replaces the bespoke `OnceLock<Mutex<File>>` logger. Standard ecosystem, same one-line ergonomics.
- `embed-resource` replaces `winres`. Same purpose, different file shape (`res/icon.rc` instead of builder API).
- `os/` directory consolidates color, wide-string, DPI, registry, theme helpers (currently scattered across `theme.rs` and `native_interop.rs`).

## Requirements

### Functional

- `diag::init(enabled: bool)` writes a log file at `%TEMP%\claude-code-usage-bubble.log` when called with `true`; no-op otherwise.
- `log::info!` / `log::warn!` / `log::error!` macros work and route to the file.
- `os::wide_str(&str) -> Vec<u16>` returns a NUL-terminated UTF-16 vector.
- `os::Color` provides hex parsing + COLORREF conversion.
- `os::dpi::for_window(hwnd)` returns u32 DPI ≥ 96.
- `os::registry::read_string(hkey, path, name)` returns Option<String>.
- `os::registry::write_string(hkey, path, name, value)` returns Result.
- `os::registry::delete_value(hkey, path, name)` returns Result.
- `os::theme::is_dark()` returns bool from registry.
- `net::winhttp::Client::new()` constructs a client with a user-agent string.
- `client.get(url).header(k, v).send()` returns `Result<Response, net::Error>`.
- `client.post(url).header(k, v).json_body(value).send()` returns `Result<Response, net::Error>`.
- `Response::status() -> u32`, `Response::header(&str) -> Option<&str>`, `Response::text() -> Result<String, net::Error>`, `Response::json<T>() -> Result<T, net::Error>`.
- Build script (`build.rs`) embeds `res/icon.ico` and version info via `embed-resource`.

### Non-functional

- Binary size after this phase: ≤ current (we add `tiny-skia`/`embed-resource`/`simplelog`/`thiserror` deps in later phases; this phase should not balloon).
- No new behavior changes vs current build.
- All new modules pass `cargo clippy -- -W clippy::all` with zero new warnings.

## Architecture

### `diag/mod.rs` (~30 LOC)

```rust
use std::path::PathBuf;
use simplelog::{Config, LevelFilter, WriteLogger};
use std::fs::File;

pub fn init(enabled: bool) -> Result<Option<PathBuf>, std::io::Error> {
    if !enabled { return Ok(None); }
    let path = std::env::temp_dir().join("claude-code-usage-bubble.log");
    WriteLogger::init(LevelFilter::Debug, Config::default(), File::create(&path)?)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    log::info!("diagnostic logging enabled");
    Ok(Some(path))
}
```

Callers use `log::info!` etc directly — no `diagnose::log(...)` indirection.

### `os/` directory

| File | Responsibility | LOC |
|---|---|---|
| `mod.rs` | `pub use` re-exports + module declarations | ~10 |
| `color.rs` | `Color { r, g, b }`, `from_hex`, `to_colorref()` | ~30 |
| `string.rs` | `wide_str(&str) -> Vec<u16>` | ~5 |
| `dpi.rs` | `for_window(hwnd)`, `for_system()`, `scale(logical, dpi)` | ~20 |
| `registry.rs` | typed wrapper over `RegOpenKeyExW`/`RegQueryValueExW`/`RegSetValueExW`/`RegDeleteValueW` | ~80 |
| `theme.rs` | `is_dark()` → reads `SystemUsesLightTheme` via `os::registry` | ~15 |

### `net/winhttp.rs` (~150 LOC)

```rust
pub struct Client {
    session: HINTERNET,
    user_agent: Vec<u16>,
}

pub struct RequestBuilder<'a> {
    client: &'a Client,
    method: Method,
    url: Url,
    headers: Vec<(Vec<u16>, Vec<u16>)>,
    body: Option<Vec<u8>>,
}

pub struct Response {
    status: u32,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("WinHTTP error {code}: {context}")]
    Win(u32, String),
    #[error("HTTP {status}")]
    Status(u32),
    #[error("JSON parse: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid URL: {0}")]
    Url(String),
    #[error("UTF-8 conversion failed")]
    Utf8,
}
```

Internally chains: `WinHttpOpen` → `WinHttpCrackUrl` → `WinHttpConnect` → `WinHttpOpenRequest` → `WinHttpSendRequest` → `WinHttpReceiveResponse` → `WinHttpQueryHeaders` → `WinHttpReadData`.

### `Cargo.toml` updates

```toml
[dependencies]
# REMOVED: ureq, native-tls
serde = { version = "1", features = ["derive"] }
serde_json = "1"
dirs = "6"
log = "0.4"
simplelog = "0.12"
thiserror = "2"

[dependencies.windows]
version = "0.58"
features = [
    # existing features +
    "Win32_Networking_WinHttp",
]

[build-dependencies]
# REMOVED: winres
embed-resource = "3"
```

### `build.rs`

```rust
fn main() {
    embed_resource::compile("res/icon.rc", embed_resource::NONE);
}
```

### `res/icon.rc` (new file)

```
#include <winver.h>
1 ICON "..\\src\\icons\\icon.ico"
1 VERSIONINFO
  FILEVERSION 0,1,0,0
  PRODUCTVERSION 0,1,0,0
  FILEOS 0x40004L
  FILETYPE 0x1L
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904E4"
    BEGIN
      VALUE "ProductName", "Claude Code Usage Bubble\0"
      VALUE "FileDescription", "Claude Code Usage Bubble\0"
      VALUE "OriginalFilename", "claude-code-usage-bubble.exe\0"
      VALUE "InternalName", "ClaudeCodeUsageBubble\0"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1252
  END
END
```

## Related code files

**To create:**
- `src/diag/mod.rs`
- `src/os/mod.rs`
- `src/os/color.rs`
- `src/os/string.rs`
- `src/os/dpi.rs`
- `src/os/registry.rs`
- `src/os/theme.rs`
- `src/net/mod.rs`
- `src/net/winhttp.rs`
- `res/icon.rc`

**To modify:**
- `Cargo.toml` (add new deps, remove `ureq`+`native-tls`+`winres`, add `Win32_Networking_WinHttp`)
- `build.rs` (replace winres with embed-resource)
- `src/main.rs` (add `mod diag; mod os; mod net;` declarations — do NOT remove old `mod diagnose; mod theme; mod native_interop;` yet)

**To delete:** nothing in this phase (Phase 2 removes `theme.rs`, etc.)

## Implementation steps

1. **Add new dependencies** to `Cargo.toml`: `log`, `simplelog`, `thiserror`, `embed-resource`. Add `Win32_Networking_WinHttp` to `windows` features. Leave `ureq`, `native-tls`, `winres` in place for now.
2. **Create `res/icon.rc`** referencing `src/icons/icon.ico`.
3. **Replace `build.rs`** with `embed-resource::compile`.
4. **`cargo build --release`** — verify icon embedding works (PE has icon resource).
5. **Create `src/os/string.rs`** with `wide_str()`. Trivial.
6. **Create `src/os/color.rs`** with `Color` struct + `from_hex` + `to_colorref`.
7. **Create `src/os/registry.rs`** with `read_string`, `read_u32`, `write_string`, `delete_value` — `unsafe` wrappers over Win32 registry APIs returning `Result<T, RegistryError>`.
8. **Create `src/os/theme.rs`** calling `registry::read_u32(HKEY_CURRENT_USER, "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize", "SystemUsesLightTheme")` and inverting.
9. **Create `src/os/dpi.rs`** with `for_window`, `for_system`, `scale`.
10. **Create `src/os/mod.rs`** with `pub mod color; pub mod string; pub mod dpi; pub mod registry; pub mod theme;` + re-exports of common items (`Color`, `wide_str`).
11. **Create `src/diag/mod.rs`** with `init(bool)` that initialises simplelog. Add `log::set_max_level` if not handled by simplelog.
12. **Create `src/net/winhttp.rs`** in 4 commits:
    - 12a. `Client::new`, `Drop` for `WinHttpCloseHandle`.
    - 12b. `RequestBuilder` + GET path.
    - 12c. POST + JSON body.
    - 12d. `Response::header` + `Response::text` + `Response::json`.
13. **Create `src/net/mod.rs`** with `pub mod winhttp;` + `pub use winhttp::{Client, Error, Response};`.
14. **Wire modules into `src/main.rs`**:
    ```rust
    mod diag;
    mod net;
    mod os;
    // OLD ones still present for now:
    mod diagnose;
    mod theme;
    mod native_interop;
    ```
15. **`cargo build --release`** — must compile clean. Binary should run identically to current build (we haven't replaced anything yet).
16. **`cargo clippy --release`** — fix any clippy warnings on new modules.
17. **Manual smoke test on Windows** (or note as deferred to Phase 4 testing):
    - Run `--diagnose`, confirm log file exists at `%TEMP%\claude-code-usage-bubble.log` (but it's empty for now since nothing calls `log::info!` yet — that's OK).
    - Run a tiny dev-only test binary or `cargo test` that exercises `net::winhttp::Client.get("https://api.github.com").send()` to validate the HTTP wrapper end-to-end.

## Todo checklist

- [ ] Cargo.toml deps updated
- [ ] `res/icon.rc` created
- [ ] `build.rs` swapped to embed-resource
- [ ] Build produces .exe with embedded icon
- [ ] `src/os/string.rs`
- [ ] `src/os/color.rs`
- [ ] `src/os/registry.rs`
- [ ] `src/os/theme.rs`
- [ ] `src/os/dpi.rs`
- [ ] `src/os/mod.rs`
- [ ] `src/diag/mod.rs`
- [ ] `src/net/winhttp.rs` (incremental, 4 sub-commits)
- [ ] `src/net/mod.rs`
- [ ] `src/main.rs` declares new modules
- [ ] `cargo build --release` clean
- [ ] `cargo clippy` clean
- [ ] WinHTTP smoke test passes against `api.github.com`

## Success criteria

- Phase ends with a runnable binary that behaves exactly like the previous version (no business logic changes).
- `net::winhttp::Client` can successfully GET `https://api.github.com/repos/tiennm99/claude-code-usage-bubble/releases/latest` and parse JSON.
- `log::info!("test")` from anywhere writes to `%TEMP%\claude-code-usage-bubble.log` when `--diagnose` is passed.
- Binary size: not larger than current (we've removed `ureq`+`native-tls`, added smaller crates).

## Risks + mitigations

| Risk | Likelihood | Mitigation |
|---|---|---|
| WinHTTP TLS handshake fails on older Windows | Low | Test on Win10/Win11; WinHTTP supports TLS 1.2+ since Win10 1607 |
| `WinHttpCrackUrl` is awkward; URL parsing has edge cases | Medium | Use `url` crate (~50 KB) for parsing, then pass components to WinHTTP |
| `embed-resource` doesn't match `winres`'s VERSIONINFO output exactly | Low | Verify with `mt /inspect output.exe` |
| `log` + `simplelog` collide with another logger init | None | App owns the only init |
| Chunked transfer / compression auto-decode disabled | Medium | Set `WINHTTP_OPTION_DECOMPRESSION` flag |

## Security considerations

- WinHTTP enforces certificate validation by default — don't disable.
- `registry` module writes only to `HKEY_CURRENT_USER` (user-scoped). No admin escalation.
- Log file path is `%TEMP%` — user-scoped. No secrets logged (verify in Phase 4 when adding token-handling logs).

## Next steps

→ Phase 2: replace `models.rs` + `localization/*` with `usage/types.rs` + `i18n/` directory.
