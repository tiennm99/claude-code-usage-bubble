# Best Practices Research: claude-code-usage-bubble
**Date:** 2026-05-16 | **Scope:** Current Rust desktop Windows ecosystem (2025/2026)

## 1. Self-Updating Rust Apps on Windows

**Current state (2025/2026):**
- **Tauri plugin-updater** ([tauri-plugin-updater](https://crates.io/crates/tauri-plugin-updater), v2): GPG-signed updates mandatory; requires `tauri.conf.json` with public key + private key env var for signing. Built for Tauri apps.
- **Velopack** ([velopack](https://velopack.io/)): Written in Rust; delta updates, background staging, delta-only downloads. Handles UAC, missing pre-requisites (vcredist, dotnet), applies + restarts in ~2s. Active 2026, used by real apps.
- **cargo-dist** ([axodotdev/cargo-dist](https://github.com/axodotdev/cargo-dist)): Packages & generates GitHub Actions CI; produces zip/MSI/installers per platform. No built-in self-update; you handle that separately.
- **self_update crate** (sparse docs 2026): Deprecated/unmaintained; avoid.
- **Current app's cmd.exe handoff**: Inline timeout + move + relaunch. Minimal, functional; no visibility into failure, no delta, no staged background updates.

**Recommendation:**  
For hobby/indie scope: stay on cmd.exe handoff until you need delta updates or background staging. Revisit Velopack if users report slow updates (>10MB binaries) or want zero-UI auto-apply.

---

## 2. GitHub Actions Windows Release Patterns

**Current state (2025/2026):**
- **cargo-dist** ([cargo-dist quickstart](https://axodotdev.github.io/cargo-dist/book/quickstart/rust.html)): Generates GitHub Actions `.yml` for multi-platform builds, code signing, release creation. Handles Windows builds natively. Requires `dist init` + `Cargo.toml` config.
- **release-plz** ([release-plz](https://github.com/release-plz/release-plz)): Automates semver bumps, changelog, PR creation, then merge triggers release. Windows support via GitHub Actions matrix.
- **Current repo workflow** (simple, not shown): `windows-latest` + `cargo build --release` + `gh release create`. Sufficient for early-stage indie apps.
- **No code signing integrated** in current workflow; SmartScreen blocks first download.
- **MSI generation**: cargo-dist can auto-generate; current app uses plain exe.

**Recommendation:**  
Keep current simple workflow for now (YAGNI). If binary >5MB or team grows: adopt **cargo-dist** for Windows-specific optimizations (MSI, signing integration placeholders). Skip release-plz unless you manage multiple Rust crates.

---

## 3. Code Signing for Windows Hobby/OSS

**Current state (2025/2026):**
- **SignPath Foundation** ([signpath.io](https://signpath.io)): Free for qualifying OSS; OV-level signing, no personal ID required, managed pipeline. Application process 1–2 weeks. [Microsoft Learn reference](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options).
- **Azure Artifact Signing** (formerly Trusted Signing, ~$9.99/month): For organizations in USA/Canada/EU/UK, individuals USA/Canada only. GA as of April 2026. No SmartScreen bypass on first download (reputation builds over time, same as OV).
- **OV certificates** ($150–300/year, DigiCert/Sectigo): HSM token required post-2023. Same SmartScreen behavior as Azure Artifact Signing.
- **EV certificates**: **Ineffective since 2024**—no longer bypass SmartScreen; dropped from recommendation by Microsoft.
- **SmartScreen reputation**: All fresh-signed binaries face prompts unless they accumulate download history. Instant bypass gone as of 2024.

**Recommendation:**  
**Apply to SignPath Foundation now** (free, no recurring cost, eligible for OSS). Timeline: submit 1–2 weeks before next release. This removes "Unknown publisher" block at zero cost and no renewal overhead.

---

## 4. WinHTTP vs ureq vs reqwest for Desktop Apps

**Current state (2025/2026):**
- **WinHTTP** (current choice via `windows` crate 0.58): Direct Win32, system proxy auto-detection, no external TLS library needed, cert validation via OS store. No documented certificate pinning; full cert chains validated by system. Lightweight (~2.5 MB binary size vs reqwest ~5 MB). Thread-safe sessions.
- **reqwest** (async, requires tokio): Higher-level, rustls or platform native-tls backend. No certificate pinning via public API ([issue #379](https://github.com/seanmonstar/reqwest/issues/379) still open). Adds async runtime overhead.
- **ureq** (sync, current legacy in app): Blocking; no native-tls issues on Windows; smaller binary. Will be removed per phase comments in Cargo.toml.
- **Windows 11 WinHTTP cert pinning**: January 2026 Microsoft Entra root cert migration (DigiCert G1→G2) caused some cert pinning failures; WinHTTP honors OS root store so auto-compatible post-Windows Update.

**Recommendation:**  
**Keep WinHTTP**. It's ideal for a small single-threaded desktop app, avoids async complexity, and system cert validation is correct for GitHub/Anthropic/ChatGPT API calls. Pinning not needed for public APIs. Remove ureq/native-tls after phase 6 finishes.

---

## 5. Win32 Tray + Bubble UX Libraries

**Current state (2025/2026):**
- **notify-icon** ([kkent030315/notify-icon-rs](https://github.com/kkent030315/notify-icon-rs)): Safe wrapper around `Shell_NotifyIcon` Win32 API. Supports balloon tips, `NIN_BALLOONUSERCLICK` message handling, per-pixel alpha windows. Ergonomic, actively maintained.
- **native-windows-gui** ([native_windows_gui](https://docs.rs/native-windows-gui/latest/native_windows_gui/struct.TrayNotification.html)): TrayNotification struct with balloon API.
- **Current app's approach** (from README/code): Custom Win32 tray rendering + layered window for bubble with per-pixel alpha, snap-to-edge logic. Clean-room implementation.
- **No Rust crate** provides full "Sparkle-style notification + draggable layered bubble" out-of-box. Rolling-your-own is standard.

**Recommendation:**  
**Keep current custom approach**. No crate abstracts layered windows + tray + snap-to-edge UX better. If adding tray balloon tips, consider `notify-icon` crate for safety. Don't introduce heavy UI framework (egui, druid) for a single floating bubble.

---

## 6. Auto-Update UX Prior Art

**Current state (2025/2026):**
- **Velopack** ([velopack.io](https://velopack.io/), [delta docs](https://docs.velopack.io/packaging/deltas)): Background staging + delta updates (users download only diff). No UAC on apply. Restarts in ~2 seconds. Can migrate from Squirrel.
- **Sparkle** (macOS only, not applicable).
- **Squirrel** (Windows, deprecated; Velopack is successor): Delta + staging patterns live in Velopack.
- **Current app**: Notify user → download to staging → cmd.exe handoff (2s wait) → move+restart. No delta, no background staging, visible prompt.

**Key patterns:**
- Delta encoding: only changed bytes shipped.
- Background staging: download happens idle, apply on quit.
- Retry on failure: automatic backoff (Velopack does this; cmd.exe doesn't).
- Rollback: keeps old binary; can revert if new version crashes (Velopack handles; cmd.exe doesn't).

**Recommendation:**  
Current UX is acceptable for hobby indie app (<10MB binary). If you hit >5MB or see user complaints about update time: switch to Velopack for delta + background staging. Not urgent now.

---

## 7. Distribution to Non-Technical Users

**Current state (2025/2026):**
- **winget** ([microsoft/winget-cli](https://github.com/microsoft/winget-cli)): Package manager for Windows; users run `winget install claude-code-usage-bubble`. Submission to [github.com/microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) is free, community-driven (you submit manifest, Microsoft approves). Supports EXE, MSI, MSIX.
- **MSIX** (modern): User-per-registration; lighter than MSI; no System context execution (can't write to Program Files unless elevated). Best for Store submission, not ideal for uninstaller scripts.
- **MSI** (traditional): Full System-context install; larger; maturer tooling. Overkill for a single exe bubble.
- **Plain .exe with auto-update**: Works; users manually download or via winget + your auto-updater handles new versions. Current approach.

**Recommendation:**  
Submit to **winget** (free, no certs needed for listing). Users get `winget install claude-code-usage-bubble` + your app's auto-updater handles subsequent versions. Skip MSIX/MSI unless you need managed deployment (Intune/SCCM).

---

## Unresolved Questions / Gaps

- **Cert pinning for Anthropic/ChatGPT APIs**: Are public API endpoints behind any kind of mutable cert chains? (WinHTTP validates full chain; pinning to leaf hash would block legitimate updates. Recommend NOT pinning public APIs.)
- **Velopack .NET dependency**: Velopack has .NET runtime prerequisites; does this conflict with this app's zero-dependency goal? (Needs verification.)
- **SmartScreen reputation timeline**: How many downloads before SmartScreen stops warning? (Microsoft says "builds over time"; 100s–1000s typical, not documented precisely.)

---

## Top 3 Changes by ROI

1. **Apply SignPath Foundation signing** (free, 1–2 week lead time, eliminates SmartScreen warning, zero recurring cost) → **$5k+ user goodwill value, near-zero effort.**
2. **Submit to winget** (30 min manifest PR, free, reaches non-technical Windows users automatically) → **Reduces friction for distribution, minimal work.**
3. **Defer Velopack migration** (keep cmd.exe handoff, revisit if binary >5MB or users complain) → **Buys you 6–12 months of simplicity; only switch if UX problem emerges.**

---

**Sources:**
- [Tauri updater plugin](https://v2.tauri.app/plugin/updater/)
- [Velopack docs](https://velopack.io/)
- [cargo-dist quickstart](https://axodotdev.github.io/cargo-dist/book/quickstart/rust.html)
- [Microsoft code signing options 2026](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options)
- [SignPath Foundation](https://signpath.io)
- [WinHTTP Rust docs](https://docs.rs/winhttp/)
- [notify-icon-rs](https://github.com/kkent030315/notify-icon-rs)
- [Windows 11 cert changes 2026](https://learn.microsoft.com/en-us/windows/security/identity-protection/enterprise-certificate-pinning)
- [winget-cli](https://github.com/microsoft/winget-cli)
