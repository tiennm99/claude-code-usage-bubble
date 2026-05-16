# Full-Project Code Review — claude-code-usage-bubble v0.1.2

Scope: full src/ tree (~6.2k LOC across 35 files). Recent shipped releases 0.1.0-0.1.2; v0.1.2 fixed cmd.exe arg escaping in the self-updater.

## Severity counts

- **P0**: 3
- **P1**: 9
- **P2**: 7

---

## P0

### P0-1. Poll thread holds global state mutex during blocking HTTPS
`src/app.rs:415-423`

`do_poll()` acquires `lock_state()`, calls `s.registry.poll_enabled(&s.http, &settings)` which dispatches to `ClaudeProvider::poll` / `ChatGptProvider::poll` — each issues a synchronous WinHTTP request inside the locked critical section. Lock is held for the full RTT (can be seconds, hang on dead network = whole poll cycle).

While locked, every UI-thread path that touches `lock_state()` stalls:
- left/right click on bubble (`on_bubble_click`, `on_bubble_right_click` → `build_panel_data`, `show_context_menu`)
- countdown timer (`refresh_countdowns`)
- WM_APP_USAGE_UPDATED dispatch (`propagate_to_ui`)
- menu commands (toggle, set_poll_interval, version_action, etc.)
- bubble move/resize callbacks

Symptom: bubble appears frozen / right-click menu won't open whenever the network is slow.

Fix: clone the data needed (settings + a separate `Mutex<Registry>` or split state) before issuing HTTP. Build a snapshot in one short lock, do HTTP outside the lock, take the lock again only to write results.

### P0-2. `attempt_refresh` holds global lock across up to 8s sleep loop
`src/app.rs:470-482`, `src/usage/refresh.rs:36-54`

`attempt_refresh` calls `s.registry.try_refresh(...)` while holding `lock_state()`. `try_refresh → Orchestrator::refresh` spawns the CLI then loops `thread::sleep(500ms)` for up to `REFRESH_TIMEOUT = 8s` per failed provider. The UI thread is unresponsive for the full duration.

Fix: same pattern as P0-1 — release the lock before calling `orchestrator.refresh(src)`. Re-acquire only for the balloon decision.

### P0-3. Synchronous update download blocks UI thread inside WM_COMMAND
`src/app.rs:1098-1104`, `src/update/install.rs:24,41-50`

`version_action` runs on the UI thread (called from `msg_wnd_proc → WM_COMMAND → on_menu_command`). On "Apply update" it calls `update::install::begin(&c, &release)` which downloads the .exe synchronously (`http.get().send()` + `fs::write`). For a multi-MB asset on a slow link the bubble + every other window message handler is blocked.

Fix: spawn a thread for the download; on completion post a custom message back to the UI thread that triggers `spawn_handoff` + `PostQuitMessage`.

---

## P1

### P1-1. GDI font handles deleted while still selected into DC
`src/bubble.rs:1293-1320`

In `paint_text_layer` the pattern is `SelectObject(hdc, label_font) → SelectObject(hdc, bold_font) → SelectObject(hdc, main_font) → DeleteObject(main_font); DeleteObject(bold_font); DeleteObject(label_font);`. The original font is never saved/restored, and `main_font` is still the currently selected object when `DeleteObject(main_font)` runs.

GDI rule: deleting a GDI object that is selected into a DC is an error; the call returns FALSE and the object is not freed. This leaks ~3 HFONT slots per `render()` call — and `render` fires on every poll, every WM_TIMER countdown tick, and on every TIMER_PULSE tick (every 80 ms while a bar is ≥95%). Process will eventually exhaust the GDI handle quota (10000 per process by default) under prolonged alarm conditions.

Fix: save first SelectObject return, restore it before deleting all three fonts. Same fix used correctly in `measure_text_w` at lines 1009-1013.

### P1-2. Update handoff: `cmd.exe` percent expansion + cmd-metachar exposure on usernames
`src/update/install.rs:53-74`

`src_str`/`tgt_str` strip `"` but not `%`, `&`, `^`, `(`, `)`, `|`. These come from `dirs::data_local_dir()` and `std::env::current_exe()`. Both can include the username segment.

Real-world risk is low (most usernames are alphanumeric), but a username containing `%VAR%` would expand inside the inner `"..."` quotes (cmd.exe percent-expansion happens inside quotes too) and a username containing `&` or `^` bypasses the inner quoting in known cmd.exe corner cases.

Fix: validate `current_exe()` / `stage_path()` against `[A-Za-z0-9 \\:.\-_/]` before substitution, or use the helper-exe pattern the comments dismiss. At minimum, reject paths containing any of `%&^|<>`.

### P1-3. Downloaded binary is not verified before swap
`src/update/install.rs:24,41-50`

`begin` downloads the asset URL and immediately stages it for `move /y` replacement. No SHA256, signature, or even file-size sanity check. If GitHub's release CDN delivers a truncated/corrupted asset (network blip), the next launch is broken with no rollback. If an attacker controls the release-publishing pipeline (compromised PAT, repo takeover) every existing install gets RCE.

Fix: ship the release with a `claude-code-usage-bubble.exe.sha256` sidecar (or use the GH API's `digest` field — populated in newer releases), download both, compare before `spawn_handoff`. Also `fsync` the staged file.

### P1-4. Token-expiry balloon picks wrong provider when both are enabled
`src/app.rs:715-744`

`show_token_expired_balloon` ignores which provider actually failed: `if s.settings.show_claude_code { (Claude, claude title, claude body) } else { (ChatGpt, ...) }`. If both providers are enabled and only Codex's token expired, the balloon claims the Claude token expired. Sent through `attempt_refresh` the `failures: Vec<ProviderId>` already knows the real victim.

Fix: pass `failures` (or one chosen provider) into `show_token_expired_balloon` and pick the kind + strings from it.

### P1-5. Multiple Refresh clicks pile up concurrent poll threads
`src/app.rs:351,397-413,353-356,963-1000`

`spawn_poll_thread()` is called unconditionally from `IDM_REFRESH`, `IDM_FREQ_*`, `toggle_model`, and timer callbacks. There's no in-flight flag. Each thread serializes on `lock_state()` (so HTTP is sequential per P0-1) but the threads themselves accumulate and the UI fires N redundant `WM_APP_USAGE_UPDATED` posts.

Fix: an `AtomicBool` poll-in-flight gate, or coalesce by skipping the spawn when one is already running.

### P1-6. CJK suffix overflows the bubble's countdown column
`src/bubble.rs:891`, `src/i18n/locales/{ja,ko,zh-TW}.toml`

`COUNTDOWN_TEMPLATE = "999d"` measures column width against 4 ASCII glyphs. Korean uses `시간` and `분` (multi-codepoint, wider full-width characters); Japanese/Chinese use `日 時 分`. The right-side text column is sized to the ASCII template and gets clipped or runs into the percent column on these locales.

Fix: measure the template using the actual active locale's suffix strings (e.g. `format!("999{}", strings.hour_suffix)`) and pick the longest among day/hour/minute/second so all variants fit.

### P1-7. Panel `place_near` ignores monitor origin on multi-monitor
`src/panel.rs:503-522`

Comparing `y < 0` and `x + panel_w > virtual_screen_w` only works when the primary monitor is at origin (0,0). With a left-side secondary monitor at `(-1920, 0)`, the bubble may sit at `x = -1500`; `x < 0` triggers and the panel jumps to `x = 8` on the primary monitor — far from the anchor. Same for negative Y.

Fix: use `MonitorFromWindow(anchor_hwnd, MONITOR_DEFAULTTONEAREST)` + `GetMonitorInfoW` to clamp into the anchor's monitor work area, mirroring what `clamp_into_work_area` already does in `bubble.rs:767-806`.

### P1-8. `CreatePopupMenu().unwrap()` x5 panics UI thread on low-resource failure
`src/app.rs:790,806,821,835,870`

GDI/USER object limits or session lockup can cause `CreatePopupMenu` to return null. The unwrap propagates to the message loop and kills the app rather than just declining to show the menu.

Fix: `let Ok(freq) = CreatePopupMenu() else { let _ = DestroyMenu(menu); return; };` (and propagate cleanup). Use the existing `DestroyMenu(menu)` pattern already in place.

### P1-9. Dead `ureq` + `native-tls` deps in shipped binary
`Cargo.toml:11-14`

Comment claims poller.rs / updater.rs keep ureq alive; both files are gone (`Glob` confirms). `ureq`, `native-tls`, and their transitive deps (foreign-types, core-foundation, etc.) are still linked into the release binary, increasing exe size and attack surface for no behavioral reason.

Fix: drop `ureq` and `native-tls` from `[dependencies]`; let `cargo build` confirm nothing breaks.

---

## P2

### P2-1. Bubble window WM_SETICON leaks HICONs at exit
`src/bubble.rs:170-198`

`ExtractIconExW` returns two HICONs that are sent via `WM_SETICON`. The window manager does not take ownership — application is expected to `DestroyIcon` them when the window is destroyed (or before replacing). Both leak for the process lifetime.

Fix: on `WM_DESTROY`, send WM_SETICON with null and DestroyIcon the previous ones.

### P2-2. `kind_to_provider` is dead identity code
`src/app.rs:566-571`, type alias `TrayIconKind = ProviderId` in app.rs:29

`TrayIconKind` is just `ProviderId`. The match arm-by-arm conversion is identity. Whole function and all call sites can be deleted (or replaced by direct passing).

Fix: inline; remove the `TrayIconKind` alias too — it's confusing scaffolding from an earlier refactor.

### P2-3. Per-frame DC measure: `compute_layout` creates+destroys 4 HFONTs per render
`src/bubble.rs:945-948, 988-1015`

`measure_text_w` is called 4× from `compute_layout`, each making a `CreateFontW + DeleteObject`. For static templates ("999d", "100%", "5h", "7d") at fixed DPI/breakpoint, this could be cached. Hot path during pulse (every 80ms).

Fix: cache layout per `(size_logical, dpi, label_font_px, font_px)` key. Invalidate on WM_DPICHANGED.

### P2-4. `apply_alpha_mask` re-runs `point_in_rounded_rect` for every pixel
`src/bubble.rs:1411-1422`, also `paint_background` 1149-1159, `paint_accent_stripe` 1173-1180

Three full-canvas passes each rechecking the same rounded-rect predicate. For a 360x140 bubble that's 3×50k = 150k branchy point-in-rect tests per frame. Painful at 12.5fps pulse.

Fix: precompute a row span (`x_min..x_max`) per scanline once, share across the three passes. Or set alpha in `paint_background` directly and drop the separate mask pass.

### P2-5. Tray icon loses registration after explorer.exe restart
`src/tray/mod.rs:52-82`

No handler for the `TaskbarCreated` registered shell message. If Explorer restarts (crash or DPI/theme change), every NIM_MODIFY for our tray icon silently fails and the icon vanishes for the rest of the session.

Fix: register the `RegisterWindowMessageW("TaskbarCreated")` message in `msg_wnd_proc`, and on receipt clear `registered` set and let the next `sync()` re-issue NIM_ADD.

### P2-6. `parse_iso8601` has unreachable shadow + custom calendar math
`src/usage/anthropic.rs:168-218`

`let trimmed = ...; let _ = trimmed;` discards the work; the parser re-splits on 'T' and rebuilds. Custom leap/days math is brittle — works for current decade but invites subtle bugs.

Fix: small adjustment now — remove the dead `trimmed` shadow. Long-term: import `time` (already pulled in transitively) and use `OffsetDateTime::parse`.

### P2-7. `bubble.rs` is 1496 lines — past file-size threshold
`src/bubble.rs`

Project rule (CLAUDE.md) targets <200 LOC/file for context manageability. Bubble has accreted hit-testing, snap geometry, fullscreen detection, GDI painting, layout math, and pulse animation in one file. Splitting (`bubble/wnd_proc.rs`, `bubble/snap.rs`, `bubble/paint.rs`, `bubble/layout.rs`) would localize future changes.

Fix: low priority — refactor only when next painting/layout pass is needed.

---

## Unresolved questions

1. **Update channel auto-detect**: `update::current_channel()` always returns `Portable`. Is that intended for v0.1.x ship, or did the winget probe get cut and forgotten? If winget is on the roadmap, P1-3 (binary verification) becomes optional for that channel since winget signs.

2. **GitHub release asset digest field**: as of late-2025 GitHub returns a `digest` ("sha256:…") on release assets via the REST API. Worth confirming whether to consume that (P1-3 fix) or ship a sidecar.

3. **Provider-aware balloon**: P1-4's fix assumes one balloon per failed provider is the desired UX. Alternative: aggregate ("Claude + Codex tokens expired"). Which does the product owner prefer?

---

**Status:** DONE
**Summary:** Reviewed full 6.2k LOC tree. Found 3 P0 (all are lock-while-blocking-IO patterns hanging the UI), 9 P1 (GDI font leak on every paint, update handoff still has minor injection surface + no binary verification, wrong-provider balloon, dead deps, multi-monitor + CJK layout bugs), 7 P2. No mutations applied.
**Concerns/Blockers:** none.
