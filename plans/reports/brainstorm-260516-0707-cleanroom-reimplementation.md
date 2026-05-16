# Brainstorm: Clean-room reimplementation of copied modules

**Date:** 2026-05-16
**Skill:** `/ck:brainstorm`
**Goal:** Replace ~2,700 LOC of copied-from-upstream code in `claude-code-usage-bubble` with original implementations so attribution can be dropped honestly.

---

## TL;DR — brutal verdict

**Option 2 (rewrite) is worth doing IF AND ONLY IF you commit ~3–5 focused days of work.** The clean-room version is structurally better (trait-based providers, embedded TOML i18n, anti-aliased tray icons, WinHTTP instead of ureq, simpler updater) and lets you drop NOTICE. If you just want this app for personal use, Option 1 (keep NOTICE) is strictly cheaper and totally fine — MIT attribution is normal practice in OSS and nobody is going to sue you.

Recommendation if you proceed: **adopt all 10 axis recommendations below**. Mixing-and-matching produces a hybrid that still looks derivative.

---

## Scope reminder

**OUT of scope (kept as-is — original work):**
`bubble.rs`, `panel.rs`, `app.rs`, `settings.rs`, `main.rs` (~2,900 LOC original).

**IN scope (currently copied — to rewrite):**
`models.rs`, `diagnose.rs`, `theme.rs`, `poller.rs`, `updater.rs`, `tray_icon.rs`, `localization/*` (9 files), `native_interop.rs`, `build.rs`. Total ~2,700 LOC.

**External contracts that CANNOT change** (would break the app):
Anthropic + ChatGPT endpoint paths/headers, credential file paths/JSON shapes, WSL access via `wsl.exe`, CLI-driven token refresh, GitHub releases JSON, Windows `Run` registry path.

---

## Axis-by-axis recommendation

Each row gives the **top pick** and runners-up, with explicit trade-offs.

| # | Axis | Top pick | Runner-up | Why |
|---|---|---|---|---|
| 1 | HTTP / async | **WinHTTP via `windows-rs`** | attohttpc | OS-native, no TLS crate needed, respects Windows proxy, smaller binary, visibly different |
| 2 | Errors | **`thiserror` per-module enums** | anyhow | Typed errors per subsystem, no runtime cost, retry logic can match on variant |
| 3 | Providers | **`trait UsageProvider`** | enum + match | Structurally different from source's flat `poll_claude()`/`poll_codex()`; extensible for future providers |
| 4 | Credential discovery | **`trait CredentialSource` + Vec<Box<dyn ...>>** | flat fn chain | Pluggable, easy to add new locations |
| 5 | Token refresh | **`RefreshOrchestrator` type, brief-wait** | non-blocking spawn | State-machine in its own type; waits 5–8s then proceeds (vs source's 30s block) |
| 6 | Localization | **Embedded TOML via `include_str!`** | gettext | Each language a TOML file in `i18n/locales/*.toml`; parsed at startup; easy for translators |
| 7 | Tray badges | **`tiny-skia` for anti-aliased rendering** | pre-rendered PNG set | Pure-Rust 2D, visibly nicer than GDI primitives, +~200 KB binary |
| 8 | Updater handoff | **detached `.bat` script** | helper-exe (current) | One-off script writes, moves, restarts; no duplicate-exe pattern |
| 9 | Diagnose / logging | **`log` + `simplelog` file appender** | `tracing` | Standard ecosystem, level-aware, replaces hand-rolled OnceLock<Mutex<File>> |
| 10 | Build script | **`embed-resource`** | winres (current) | Modern replacement; identical capabilities; new Cargo.toml line |

### Detailed trade-offs

#### Axis 1 — WinHTTP vs ureq

| | ureq (source) | WinHTTP (recommended) |
|---|---|---|
| Binary size | +600 KB (ureq + rustls + native-tls) | 0 (Windows ships it) |
| Proxy handling | manual via env vars | automatic (system proxy) |
| TLS | native-tls (delegates to SChannel) | SChannel directly |
| Code shape | `agent.get(url).set(header, val).call()?` | `WinHttpOpen → WinHttpConnect → WinHttpOpenRequest → WinHttpSendRequest → WinHttpReceiveResponse` |
| Verbosity | low | high (but encapsulate in `net::winhttp::Client`) |
| Clean-room win | low (same crate as source) | **high** |

WinHTTP via `windows-rs`'s `Win32::Networking::WinHttp` module is the genuinely-different choice. Encapsulate the verbosity in one ~150-line `net::winhttp::Client` type and the rest of the code looks like normal HTTP.

#### Axis 2 — Error handling

Source uses `enum PollError { AuthRequired, NoCredentials, TokenExpired, RequestFailed }`. Visibly similar to thiserror enums, but: per-module thiserror is **structurally** different. Source has ONE error type for ALL of poller.rs. Clean-room splits into `usage::Error`, `creds::Error`, `update::Error`, `net::Error`, each with their own variants. This forces conversion at boundaries via `#[from]` — which is itself a different pattern.

#### Axis 3 — Provider trait

```rust
// New shape:
pub trait UsageProvider: Send {
    fn id(&self) -> ProviderId;
    fn poll(&mut self, http: &dyn HttpClient) -> Result<UsageWindows, usage::Error>;
}

pub struct ClaudeProvider { creds: Box<dyn CredentialSource>, ... }
pub struct ChatGptProvider { creds: ChatGptAuth, ... }
```

vs source:

```rust
// Source shape:
pub fn poll(show_claude_code: bool, show_codex: bool) -> Result<AppUsageData, PollError> {
    if show_claude_code { ... poll_claude_code()? }
    if show_codex { ... poll_codex()? }
}
```

The trait shape supports any number of providers, returns per-provider results, and `app.rs` becomes a registry instead of a switch. **Substantially different code shape.**

#### Axis 4 — Credential discovery

```rust
// New:
pub trait CredentialSource {
    fn id(&self) -> &str;
    fn read(&self) -> Result<Token, creds::Error>;
    fn refresh(&self) -> Result<(), creds::Error>;
    fn signature(&self) -> String; // for change-detection
}

pub struct LocalClaudeCreds { path: PathBuf }
pub struct WslClaudeCreds { distro: String }
pub struct CodexCreds { path: PathBuf }

pub struct CredentialLocator { sources: Vec<Box<dyn CredentialSource>> }
impl CredentialLocator {
    pub fn find_first(&self) -> Option<&dyn CredentialSource>;
    pub fn all_signatures(&self) -> Vec<String>;
}
```

#### Axis 5 — Token refresh

`RefreshOrchestrator` type:

```rust
pub struct RefreshOrchestrator {
    cli_resolver: CliResolver,
    timeout: Duration,
}

pub enum RefreshOutcome {
    Refreshed,
    StillExpired,
    CliMissing,
    Timeout,
}

impl RefreshOrchestrator {
    pub fn refresh(&self, source: &dyn CredentialSource) -> RefreshOutcome;
}
```

Default `timeout` is 8s (vs source's 30s). Falls through faster on failure; next poll cycle catches up. UX: user sees "..." for ~5 min after token expiry if refresh truly takes too long, vs 30 s of UI hang in source.

#### Axis 6 — Localization

```
i18n/locales/
  en.toml          # english
  vi.toml
  ja.toml
  ko.toml
  zh-tw.toml
  fr.toml
  de.toml
  es.toml
  nl.toml
```

Each TOML:

```toml
[ui]
window_title = "Claude Code Usage Bubble"
refresh = "Refresh"
update_frequency = "Update frequency"
# ...
[duration]
day_suffix = "d"
hour_suffix = "h"
# ...
```

At startup: `include_str!("locales/en.toml")` → parse via `toml::from_str::<I18nFile>` → cache. Locale detection unchanged (still Win32 `GetUserPreferredUILanguages`).

Public surface:
```rust
pub struct I18n {
    strings: HashMap<&'static str, LocaleStrings>,
    active: &'static str,
}
impl I18n {
    pub fn load(active_code: Option<&str>) -> Self;
    pub fn get(&self) -> &LocaleStrings;
}
```

vs source's `LanguageId::English.strings()` enum-dispatched const tables. Same outcome, completely different shape. Plus: adding a new language is `cp en.toml fr.toml && translate` — no Rust code changes.

#### Axis 7 — Tray badges with tiny-skia

```rust
pub fn render_badge(percent: Option<f64>, kind: BadgeKind, size: u32) -> Vec<u32> /* BGRA */ {
    let mut pixmap = Pixmap::new(size, size).unwrap();
    // Anti-aliased fill via tiny-skia Path + Paint
    // Drop into HICON via CreateIconFromResourceEx
}
```

Source draws DIB sections + GDI primitives (rectangles, text). tiny-skia uses path rendering with vector fills, anti-aliased. Result: smoother percentage badges. Trade-off: ~200 KB binary, but ergonomics for ring/arc drawing are massively better than GDI.

#### Axis 8 — Updater via .bat handoff

```rust
// Download new.exe to %TEMP%\bubble-update.exe
// Write %TEMP%\bubble-swap.bat:
//   @echo off
//   timeout /t 2 /nobreak >nul
//   move /y "%~dp0bubble-update.exe" "C:\Path\To\claude-code-usage-bubble.exe"
//   start "" "C:\Path\To\claude-code-usage-bubble.exe"
//   del "%~f0"
// Spawn cmd /c bubble-swap.bat with DETACHED_PROCESS | CREATE_NO_WINDOW
// Exit current process
```

Source: copies `current.exe → updater-helper.exe`, spawns helper with `--apply-update`, helper waits for parent to exit, copies new over, restarts.

New approach: shell script does the same dance, no duplicate exe. Simpler. Visibly different. Mild reliability risk if the user's PATH lacks `cmd.exe` (essentially impossible on Windows) or if antivirus blocks the .bat (more likely than .exe blocking, actually). Mitigation: use `cmd /c ...` directly with no temp .bat — pass commands inline.

Cleaner inline version:
```rust
Command::new("cmd.exe")
    .args(["/c", "timeout /t 2 /nobreak >nul & move /y \"new.exe\" \"target.exe\" & start \"\" \"target.exe\""])
    .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
    .spawn()?;
std::process::exit(0);
```

Even simpler — no .bat file at all.

#### Axis 9 — log + simplelog

```rust
// diag/mod.rs
pub fn init(enabled: bool) -> Result<PathBuf> {
    if !enabled { return Ok(PathBuf::new()); }
    let path = std::env::temp_dir().join("claude-code-usage-bubble.log");
    WriteLogger::init(LevelFilter::Debug, Config::default(), File::create(&path)?)?;
    Ok(path)
}
```

Then everywhere: `log::info!("...")`, `log::error!("...")`. Standard, idiomatic, visibly different from source's `diagnose::log(format!(...))`.

#### Axis 10 — embed-resource vs winres

```rust
// build.rs:
fn main() {
    embed_resource::compile("res/icon.rc", embed_resource::NONE);
}
```

Plus `res/icon.rc`:
```
1 ICON "icon.ico"
1 VERSIONINFO
  FILEVERSION 0,1,0,0
  PRODUCTVERSION 0,1,0,0
  ...
END
```

Trade-off: explicit .rc file instead of `WindowsResource` builder calls. Slightly more verbose. Visibly different. Same output.

---

## Proposed new module layout

```
src/
  main.rs                 (unchanged)
  app.rs                  (refactor imports only)
  bubble.rs               (refactor imports only)
  panel.rs                (refactor imports only)
  settings.rs             (refactor imports only)

  net/
    mod.rs                pub use winhttp::Client;
    winhttp.rs            WinHTTP wrapper (~150 LOC)

  usage/
    mod.rs                Provider trait + ProviderId enum
    types.rs              UsageWindows, Window, Reset, ProviderId
    anthropic.rs          ClaudeProvider impl
    chatgpt.rs            ChatGptProvider impl
    refresh.rs            RefreshOrchestrator
    headers.rs            anthropic rate-limit header parsing

  creds/
    mod.rs                CredentialSource trait + CredentialLocator
    local_fs.rs           LocalClaudeCreds (Windows path)
    wsl_bridge.rs         WslClaudeCreds (wsl.exe spawn)
    codex_auth.rs         CodexCreds (Codex auth.json)

  i18n/
    mod.rs                I18n loader, LocaleStrings struct
    detect.rs             Win32 locale detection
    locales/
      en.toml, vi.toml, ja.toml, ko.toml, zh-tw.toml, fr.toml, de.toml, es.toml, nl.toml

  update/
    mod.rs                pub use install::run;
    release.rs            GitHub releases fetch + Version cmp
    install.rs            Download + inline-cmd-handoff
    channel.rs            Portable/Winget detection

  tray/
    mod.rs                Add/Update/Remove/Sync
    badge.rs              tiny-skia badge renderer + DIB→HICON
    callback.rs           WM_APP_TRAY dispatch

  os/
    mod.rs                pub use everywhere
    color.rs              Color, ARGB helpers
    string.rs             wide_str
    dpi.rs                DPI helpers
    registry.rs           Registry read/write wrapper
    theme.rs              Dark mode detection

  diag/
    mod.rs                log facade + simplelog init
```

Each former flat-file module becomes a directory with clearer responsibility splits. **No file shares a name with the source repo.**

---

## Risk callout — what breaks?

1. **`app.rs` / `bubble.rs` / `panel.rs` imports** — they currently reference `crate::poller::*`, `crate::models::*`, `crate::tray_icon::*`, `crate::theme::*`, `crate::diagnose::*`, `crate::localization::*`, `crate::updater::*`. ALL of these change. Estimated: ~80 import-line edits + ~30 call-site refactors (e.g. `poller::poll(...)` → `provider_registry.poll_all(&http_client)`). Concentrated in `app.rs`.

2. **`AppUsageData` shape** — currently `{ claude_code: Option<UsageData>, codex: Option<UsageData> }`. New shape per provider trait: `HashMap<ProviderId, Result<UsageWindows, Error>>` or `Vec<(ProviderId, ProviderResult)>`. `app.rs` orchestrator needs reshape.

3. **WinHTTP learning curve** — first time you write WinHTTP code, expect 4–6 hours of stumbling on quirks (chunked transfer, automatic decompression, proxy bypass list, certificate validation flags). Mitigated by encapsulating in one `net::winhttp::Client` type.

4. **tiny-skia + HICON conversion** — need a small `pixmap_to_hicon()` helper that wraps `CreateIconFromResourceEx` or `CreateIconIndirect`. Expect 2 hours including pixel-format debugging.

5. **TOML escape edge cases** — strings with `"` or non-ASCII need proper escaping. `toml` crate handles this on read but you'll write 9 TOML files by hand. Half-day of careful copying.

6. **Updater inline-cmd race** — if user closes the app during the 2-second `timeout`, the move/start succeeds anyway because the file lock releases on exit. Edge case: AV may flag the inline cmd as suspicious. Less likely than .bat-file flagging.

7. **Behavior regressions** — source code is proven through real usage. Your new code is fresh. Expect first-launch bugs that the source didn't have: WSL probe order, ChatGPT-Account-Id header omission, rate-limit-header zero-default semantics, ClipBoardCheck on token-refresh.

---

## Time estimate

| Module | LOC | Hours |
|---|---|---|
| `os/` (color, string, dpi, registry, theme) | ~120 | 2 |
| `net/winhttp.rs` | ~150 | 6 |
| `usage/types.rs` + `usage/headers.rs` | ~100 | 2 |
| `usage/anthropic.rs` (+ ChatGptProvider) | ~250 | 4 |
| `usage/refresh.rs` | ~120 | 2 |
| `creds/{local_fs,wsl_bridge,codex_auth}` | ~200 | 3 |
| `i18n/` + 9 TOML files | ~250 | 4 |
| `tray/badge.rs` (tiny-skia) | ~150 | 3 |
| `tray/mod.rs` + callback | ~200 | 2 |
| `update/{release,install,channel}` | ~250 | 3 |
| `diag/` | ~30 | 0.5 |
| `app.rs` / `bubble.rs` / `panel.rs` refactor | — | 4 |
| `build.rs` + `res/icon.rc` | ~30 | 0.5 |
| Cargo.toml feature/dep updates | — | 0.5 |
| First-launch debugging on Windows | — | 4–8 |
| **Total** | ~1,850 | **40–48 hours** |

So: **about a week of focused work** to do this properly. Note this is LESS than the 2,700 LOC currently copied, because the new code is more idiomatic and avoids some of the source's verbosity.

---

## Brutal honesty: do or don't?

| If you want to … | Do option 1 (keep NOTICE) | Do option 2 (rewrite) |
|---|---|---|
| Ship today | ✅ | ❌ |
| Use it yourself privately | ✅ | overkill |
| Open-source publish under your name | ✅ legally fine | ✅ legally cleaner |
| Maintain long-term | tolerable | **better foundation** |
| Brag about it being "yours" | morally questionable | **honest** |
| Avoid any chance of an upstream complaint | overcautious | unnecessary |

My honest recommendation: **option 1 is fine for ~95% of users**. MIT-derivative-with-NOTICE is normal open source. The upstream's MIT license explicitly allows what you're doing. No reasonable person would object.

Option 2 is worth it if:
- You enjoy the architecture work
- You want to learn WinHTTP / tiny-skia / log-tracing
- You plan to make this a longer-term project with translators, contributors, etc.
- You'd rather not maintain the `NOTICE` file going forward

---

## If you approve option 2 — implementation order

I'd structure the rewrite as 6 phases (one PR each):

1. **Phase 1 — Infrastructure**: `diag/` + `os/` + `net/winhttp.rs` + Cargo.toml. No behavior changes. Compile-only.
2. **Phase 2 — Types & i18n**: `usage/types.rs`, `i18n/` + 9 TOML files. Replace source's `models.rs` + `localization/*`. Refactor consumers' imports.
3. **Phase 3 — Credentials**: `creds/`. Replace source's credential reading inside poller. Test on Windows.
4. **Phase 4 — Providers + refresh**: `usage/anthropic.rs`, `usage/chatgpt.rs`, `usage/refresh.rs`. Replace source's poller. Test full poll flow on Windows.
5. **Phase 5 — Tray badges**: `tray/badge.rs` + integration. Replace `tray_icon.rs`.
6. **Phase 6 — Updater + remove NOTICE**: `update/`, drop `NOTICE`, update README + LICENSE comment.

After phase 6: zero source-derivative code remains. Drop NOTICE legally.

---

## Decision needed

Pick one before I touch code:

1. **Stop here, keep NOTICE** → no further action. Repo stays as-is.
2. **Proceed with rewrite** → invoke `/ck:plan` to produce per-phase plan documents based on this brainstorm. Estimated 40–48h work, 6 PRs.
3. **Partial rewrite** → tell me which specific axes/modules you want and we draft a narrower plan.

---

## Open questions

- Do you want to bump the project to use `windows-rs 0.59+` while we're at it? Source uses 0.58. Newer versions have minor API differences. Slight extra risk but more current.
- Should the new repo's history retain the initial-port commit (so the rewrite shows up as a fresh series of commits), or should we squash-rewrite to claim authorship cleanly? I'd suggest **retain history** — it's transparent about what happened.
