---
phase: 2
status: pending
estimated_hours: 6
---

# Phase 2 — Types & i18n

## Context links

- Brainstorm: axes 3 (provider types) + 6 (localization)
- Source files to be REPLACED: `src/models.rs`, `src/localization/*` (9 files)

## Overview

- **Priority:** High (Phase 4 providers depend on these types; bubble/panel/app render using them)
- **Status:** pending
- **Brief:** Define the new provider-result data types and replace the 9 hand-coded localization Rust files with one Rust loader + 9 TOML files. Refactor consumer imports (`bubble.rs`, `panel.rs`, `app.rs`, `settings.rs`) to the new shape. End of phase: source's `models.rs` and `localization/*` deleted.

## Key insights from brainstorm

- Source's `UsageData { session, weekly }` is one shape; `UsageWindows { primary, secondary }` (or a `HashMap<Window, Reset>`) is structurally different and works for both Anthropic (5h/7d) and Codex (which already uses "primary/secondary" terminology in its API response).
- Source dispatches localization via `enum LanguageId` + matching const tables. Embedded TOML via `include_str!` + parsed-at-startup HashMap is structurally different and easier for translators.
- `LocaleStrings` becomes a `serde::Deserialize` struct keyed by TOML section.

## Requirements

### Functional

- `usage::types::UsageWindows` carries `primary: Window`, `secondary: Window`, both `pub`.
- `usage::types::Window { utilization: f64, resets_at: Option<SystemTime> }`.
- `usage::types::ProviderId` enum: `Claude`, `ChatGpt` (note: renamed from "Codex" internally; menu label stays "Codex" via i18n).
- `usage::types::ProviderSnapshot { id: ProviderId, windows: Result<UsageWindows, usage::Error> }` for app-level results.
- `i18n::I18n::load(active_code: Option<&str>) -> Self` parses all embedded TOMLs at startup.
- `i18n::I18n::strings() -> &LocaleStrings` returns the active language's strings.
- `i18n::LocaleStrings` is a single struct with all UI strings as named fields (matches what bubble/panel/app need).
- `i18n::detect::detect_system_locale() -> Option<String>` mirrors source's `GetUserPreferredUILanguages` chain.

### Non-functional

- Adding a new language = drop a new TOML file in `src/i18n/locales/` + add one line in `i18n/mod.rs` `include_str!` map.
- TOML parsing happens once at startup; ~9 small files combined < 10 KB; parse time < 5 ms.

## Architecture

### `src/usage/types.rs`

```rust
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum ProviderId {
    Claude,
    ChatGpt,
}

impl ProviderId {
    pub fn as_str(self) -> &'static str {
        match self { Self::Claude => "claude", Self::ChatGpt => "chatgpt" }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Window {
    pub utilization: f64,         // 0.0–100.0
    pub resets_at: Option<SystemTime>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UsageWindows {
    pub primary: Window,          // 5h for Claude / primary_window for ChatGPT
    pub secondary: Window,        // 7d for Claude / secondary_window for ChatGPT
}

#[derive(Clone, Debug)]
pub struct ProviderSnapshot {
    pub id: ProviderId,
    pub windows: UsageWindows,
}
```

### `src/usage/mod.rs` (Phase 2 portion — provider trait stub goes here, real impls in Phase 4)

```rust
pub mod types;
pub use types::{ProviderId, Window, UsageWindows, ProviderSnapshot};

// Provider trait lives here; impls (anthropic.rs, chatgpt.rs) come in Phase 4.
pub trait UsageProvider: Send {
    fn id(&self) -> ProviderId;
    fn poll(&mut self, http: &crate::net::Client) -> Result<UsageWindows, Error>;
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("authentication required")]
    AuthRequired,
    #[error("no credentials configured")]
    NoCredentials,
    #[error("token expired and refresh failed")]
    TokenExpired,
    #[error("network: {0}")]
    Network(#[from] crate::net::Error),
    #[error("response shape mismatch: {0}")]
    BadResponse(String),
    #[error("credential read: {0}")]
    Creds(#[from] crate::creds::Error),  // forward-declared, real type in Phase 3
}
```

In Phase 2 we leave `creds::Error` and the impls as `todo!()` stubs that won't link until Phase 3/4.

### `src/i18n/mod.rs`

```rust
use std::collections::HashMap;
use serde::Deserialize;

pub mod detect;

#[derive(Clone, Deserialize)]
pub struct LocaleStrings {
    pub window_title: String,
    pub refresh: String,
    pub update_frequency: String,
    pub one_minute: String,
    pub five_minutes: String,
    pub fifteen_minutes: String,
    pub one_hour: String,
    pub models: String,
    pub claude_label: String,           // was claude_code_model
    pub chatgpt_label: String,          // was codex_model
    pub settings: String,
    pub start_with_windows: String,
    pub reset_position: String,
    pub language: String,
    pub system_default: String,
    pub check_for_updates: String,
    pub checking_for_updates: String,
    pub up_to_date: String,
    pub update_failed: String,
    pub applying_update: String,
    pub update_available: String,
    pub update_via_winget: String,      // was update_via_winget_label
    pub exit: String,
    pub show_widget: String,
    pub session_window: String,
    pub weekly_window: String,
    pub now: String,
    pub day_suffix: String,
    pub hour_suffix: String,
    pub minute_suffix: String,
    pub second_suffix: String,
    pub token_expired_title: String,
    pub token_expired_body: String,
    pub chatgpt_token_expired_title: String,
    pub chatgpt_token_expired_body: String,
}

#[derive(Deserialize)]
struct LocaleFile {
    code: String,
    native_name: String,
    #[serde(flatten)]
    strings: LocaleStrings,
}

pub struct I18n {
    available: HashMap<String, (String, LocaleStrings)>, // code → (native_name, strings)
    active: String,
}

impl I18n {
    pub fn load(active_code: Option<&str>) -> Self {
        let raw = [
            ("en", include_str!("locales/en.toml")),
            ("nl", include_str!("locales/nl.toml")),
            ("es", include_str!("locales/es.toml")),
            ("fr", include_str!("locales/fr.toml")),
            ("de", include_str!("locales/de.toml")),
            ("ja", include_str!("locales/ja.toml")),
            ("ko", include_str!("locales/ko.toml")),
            ("zh-TW", include_str!("locales/zh-TW.toml")),
        ];
        let mut available = HashMap::new();
        for (code, body) in raw {
            if let Ok(file) = toml::from_str::<LocaleFile>(body) {
                available.insert(code.to_string(), (file.native_name, file.strings));
            }
        }
        let active = match active_code {
            Some(c) if available.contains_key(c) => c.to_string(),
            _ => detect::detect_system_locale()
                .and_then(|s| Self::normalize(&s, &available))
                .unwrap_or_else(|| "en".to_string()),
        };
        Self { available, active }
    }

    pub fn strings(&self) -> &LocaleStrings {
        &self.available[&self.active].1
    }

    pub fn active_code(&self) -> &str { &self.active }

    pub fn available(&self) -> impl Iterator<Item = (&str, &str)> {
        self.available.iter().map(|(code, (name, _))| (code.as_str(), name.as_str()))
    }

    fn normalize(code: &str, available: &HashMap<String, (String, LocaleStrings)>) -> Option<String> {
        // "en-US" → "en", "zh-Hant-TW" → "zh-TW", etc.
        // Exact match first, then prefix.
        let lower = code.to_ascii_lowercase().replace('_', "-");
        if available.contains_key(&lower) { return Some(lower); }
        let prefix = lower.split('-').next().unwrap_or("");
        if prefix == "zh" && (lower.contains("tw") || lower.contains("hant")) {
            return Some("zh-TW".into());
        }
        available.keys()
            .find(|k| k.split('-').next() == Some(prefix))
            .cloned()
    }
}
```

### `src/i18n/detect.rs`

Mirrors source's `preferred_ui_languages` + `default_ui_locale` + `default_locale_name` chain via Win32 globalization APIs, but in one function:

```rust
pub fn detect_system_locale() -> Option<String> {
    preferred().or_else(default_ui).or_else(default_user)
}
fn preferred() -> Option<String> { /* GetUserPreferredUILanguages */ }
fn default_ui() -> Option<String> { /* GetUserDefaultUILanguage + LCIDToLocaleName */ }
fn default_user() -> Option<String> { /* GetUserDefaultLocaleName */ }
```

### `src/i18n/locales/en.toml`

```toml
code = "en"
native_name = "English"

window_title = "Claude Code Usage Bubble"
refresh = "Refresh"
update_frequency = "Update frequency"
one_minute = "1 minute"
five_minutes = "5 minutes"
fifteen_minutes = "15 minutes"
one_hour = "1 hour"
models = "Models"
claude_label = "Claude Code"
chatgpt_label = "Codex"
settings = "Settings"
start_with_windows = "Start with Windows"
reset_position = "Reset position"
language = "Language"
system_default = "System default"
check_for_updates = "Check for updates"
checking_for_updates = "Checking for updates…"
up_to_date = "Up to date"
update_failed = "Update failed"
applying_update = "Applying update…"
update_available = "Update available"
update_via_winget = "via WinGet"
exit = "Exit"
show_widget = "Show widget"
session_window = "5h"
weekly_window = "7d"
now = "now"
day_suffix = "d"
hour_suffix = "h"
minute_suffix = "m"
second_suffix = "s"
token_expired_title = "Claude Code session expired"
token_expired_body = "Sign in again to keep usage reporting."
chatgpt_token_expired_title = "Codex session expired"
chatgpt_token_expired_body = "Sign in again to keep usage reporting."
```

The 8 other locale files mirror this shape with translated strings. **Important:** copy the translations from `src/localization/*.rs` content (the strings themselves are utilitarian/factual translations and not copyright-eligible the way code is — but for safety, re-translate the most unique strings using your own phrasing).

### Consumer refactors (in this phase)

**`src/app.rs`:**
- `use crate::localization::{LanguageId, Strings, resolve_language}` → `use crate::i18n::{I18n, LocaleStrings}`
- `s.language.strings()` → `s.i18n.strings()`
- `LanguageId::ALL.iter()` → `s.i18n.available()`
- All field renames: `claude_code_model` → `claude_label`, `codex_model` → `chatgpt_label`, etc.

**`src/bubble.rs`:** no localization access; only depends on bubble-specific data. Unaffected.

**`src/panel.rs`:**
- `data.strings.session_window` works unchanged (field name preserved).
- `data.strings.claude_code_model` → `data.strings.claude_label`.

**`src/settings.rs`:** unchanged (it stores `language: Option<String>` already, which holds a locale code).

**`src/models.rs`:** delete. Replace `crate::models::{AppUsageData, UsageData, UsageSection}` consumers:
- `AppUsageData` → `Vec<ProviderSnapshot>`
- `UsageData` → `UsageWindows`
- `UsageSection` → `Window`

Migration map for `app.rs`:
- `s.data.claude_code.as_ref()` → `s.snapshots.iter().find(|sn| sn.id == ProviderId::Claude)`
- `c.session.percentage` → `sn.windows.primary.utilization`
- `c.weekly.percentage` → `sn.windows.secondary.utilization`

## Related code files

**To create:**
- `src/usage/mod.rs`
- `src/usage/types.rs`
- `src/i18n/mod.rs`
- `src/i18n/detect.rs`
- `src/i18n/locales/en.toml`
- `src/i18n/locales/nl.toml`
- `src/i18n/locales/es.toml`
- `src/i18n/locales/fr.toml`
- `src/i18n/locales/de.toml`
- `src/i18n/locales/ja.toml`
- `src/i18n/locales/ko.toml`
- `src/i18n/locales/zh-TW.toml`

**To modify:**
- `Cargo.toml` — add `toml = "0.8"` (with default features)
- `src/main.rs` — declare `mod usage; mod i18n;`; remove `mod models; mod localization;`
- `src/app.rs` — migrate all `crate::models::*` and `crate::localization::*` imports
- `src/panel.rs` — field renames
- `src/bubble.rs` — only if it references `LanguageId` (it shouldn't)

**To delete:**
- `src/models.rs`
- `src/localization/mod.rs`
- `src/localization/english.rs`
- `src/localization/dutch.rs`
- `src/localization/spanish.rs`
- `src/localization/french.rs`
- `src/localization/german.rs`
- `src/localization/japanese.rs`
- `src/localization/korean.rs`
- `src/localization/traditional_chinese.rs`

## Implementation steps

1. **Add `toml = "0.8"`** to `Cargo.toml`.
2. **Create `src/usage/types.rs`** (struct definitions only — no impls yet).
3. **Create `src/usage/mod.rs`** with trait `UsageProvider` and `Error` enum. Leave it without any impls.
4. **Create `src/i18n/locales/en.toml`** first; verify TOML structure parses.
5. **Add `src/i18n/mod.rs` + `src/i18n/detect.rs`** with `I18n::load` reading only `en.toml`.
6. **Wire `mod i18n; mod usage;` into `main.rs`** and call `I18n::load(None)` from `app::run` (storing on `AppState`). Build should still compile (no usages downstream yet).
7. **Migrate `app.rs`** field-by-field from `Strings` to `LocaleStrings`. Run `cargo check` after each subsystem (menu, balloon, panel-data, tray-tooltip).
8. **Translate the other 8 locale TOMLs.** Use your own phrasings for the longer strings (e.g. `token_expired_body`) rather than direct copies of upstream's translations.
9. **Add the other 8 `include_str!` entries** to `i18n/mod.rs`.
10. **Migrate `panel.rs`** field renames (small).
11. **Migrate `app.rs` data model** from `AppUsageData` to `Vec<ProviderSnapshot>`. This is the biggest single edit. Update `apply_data`, `apply_usage_update`, `build_panel_data_from`, `refresh_tray_icons`, `refresh_text_fields`.
12. **Delete `src/models.rs` + `src/localization/*`** once nothing references them.
13. **`cargo build --release`** — clean.

## Todo checklist

- [ ] `usage/types.rs` written
- [ ] `usage/mod.rs` written (trait + Error stubs)
- [ ] `i18n/mod.rs` + `detect.rs` written
- [ ] 9 TOML locale files written (translations are your own paraphrasings)
- [ ] `Cargo.toml` adds `toml` dep
- [ ] `main.rs` declares new modules + removes old ones
- [ ] `app.rs` migrated to `LocaleStrings` + `Vec<ProviderSnapshot>`
- [ ] `panel.rs` field renames done
- [ ] `bubble.rs` confirmed unaffected
- [ ] Old `src/models.rs` + `src/localization/*` deleted
- [ ] `cargo build --release` clean
- [ ] App still runs (placeholder data since providers aren't wired yet)

## Success criteria

- TOML files parse cleanly at startup.
- App shows correct language strings based on Windows display language.
- No file in `src/` shares a name with upstream's `models.rs` or `localization/*`.
- Right-click → Language submenu lists 9 options (system default + 8 languages) and switching them updates UI immediately.

## Risks + mitigations

| Risk | Likelihood | Mitigation |
|---|---|---|
| TOML serde derive misalignment (typos in field names) | High | Use `#[serde(deny_unknown_fields)]` to catch typos at load time |
| Translations differ enough from upstream that meaning drifts | Medium | Compare meaning side-by-side before committing; ask a native speaker for the long strings if you can |
| Bubble/panel field references break in subtle places | Medium | `cargo check` after each consumer edit |
| App startup slows due to TOML parsing | Negligible | TOML files combined < 10 KB |

## Security considerations

- TOML strings are static, no eval. Parse failures fall back to English silently. No injection risk.
- No PII in locale files.

## Next steps

→ Phase 3: replace credential reading with `creds/` directory.

## Open questions

- **Translation copyright.** The upstream localization files contain ~30 short UI strings per language. These are utility translations of standard UI vocabulary and are unlikely to be copyrightable individually, but for full clean-room status, re-paraphrase the longest strings (`token_expired_body` and `chatgpt_token_expired_body`). Recommended: write your own phrasing for those two.
