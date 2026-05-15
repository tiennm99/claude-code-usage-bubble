# Phase 4: Expanded Panel + Settings Persistence + Orchestration

## Context Links

- Source `window.rs` — borrow message-loop dispatch, polling-thread orchestration, settings persistence pattern
- Source `poller.rs` — `poll`, `credential_watch_snapshot`, `format_line`, `time_until_display_change`
- Source `tray_icon.rs` — `add/update/remove/sync`, `handle_message`

## Overview

- **Priority:** High
- **Status:** pending
- **Description:** Build (a) the expanded panel that appears on bubble click and shows both 5h and 7d bars with countdowns, (b) settings persistence to `%APPDATA%\ClaudeCodeUsageBubble\settings.json`, (c) the orchestrating `app.rs` module that owns polling, message routing, context menus, and dual-bubble lifecycle.

## Key Insights

- Panel is a separate window: `WS_POPUP | WS_EX_LAYERED | WS_EX_TOPMOST`, opaque background, shown adjacent to bubble. Source's draw code for the horizontal bars can be ported almost directly (it already draws progress bars + countdown text via GDI).
- Settings file location matches source pattern: `%APPDATA%\ClaudeCodeUsageBubble\settings.json` (renamed dir). Use `dirs::config_dir()`.
- Polling: background `std::thread` spawned in `app::run`. Posts `WM_APP_USAGE_UPDATED` to each bubble window when data refreshes. Source's poll-loop logic is copy-friendly.
- Context menu: built via `CreatePopupMenu` + `AppendMenuW` + `TrackPopupMenu`. Source has the full menu structure — port it but remove "Reset position" → rename to "Reset bubble position" (per model).
- Dual-bubble: each enabled model gets its own HWND + tray icon + bubble window. Settings stores `bubble_positions: { claude: {x, y}, codex: {x, y} }`.

## Requirements

### Functional
- Settings persist across restarts: window positions, polling frequency, enabled models, language, "Start with Windows" state, last update check
- Expanded panel: shows session bar + weekly bar + countdowns + reset times for the model whose bubble was clicked
- Panel auto-closes on focus loss or after a brief timeout (optional)
- Right-click menu mirrors source's menu structure: Refresh, Models, Update frequency, Language, Start with Windows, Reset position, Updates, Exit
- Single-instance enforced via named mutex `Global\ClaudeCodeUsageBubble`
- Polling runs in background, posts updates via `PostMessageW(WM_APP_USAGE_UPDATED, ...)`
- Countdown timer adapts to display granularity (`time_until_display_change`)

### Non-functional
- Settings file is atomically written (write to `.tmp`, rename)
- Polling thread cannot block UI thread
- Mutex released on clean shutdown

## Architecture

```
src/
├── app.rs                  NEW — orchestrator: spawns bubbles, polls, routes messages,
│                                 owns tray icons, owns context menu builder
├── panel.rs                NEW — expanded panel window (one per model on demand)
├── settings.rs             NEW — load/save settings.json, schema
└── main.rs                 modified — calls app::run
```

Message flow:

```
poll thread ────PostMessage(WM_APP_USAGE_UPDATED)──▶ bubble HWND
                                                       └─▶ updates percentage, redraws
bubble click ──PostMessage(WM_APP_PANEL_TOGGLE)─▶ app handler (in bubble wndproc)
                                                       └─▶ panel::show_for(model)
right-click ──app::show_context_menu(hwnd)──▶ TrackPopupMenu ─▶ WM_COMMAND
                                                       └─▶ menu action dispatch
tray icon ────WM_APP_TRAY───────────▶ tray_icon::handle_message ─▶ TrayAction
                                                       └─▶ toggle/ shutdown / refresh
```

## Related Code Files

**To create:**
- `src/app.rs` (target: 500–800 lines)
- `src/panel.rs` (target: 300–500 lines)
- `src/settings.rs` (target: 150–250 lines)

**To modify:**
- `src/main.rs`:
  ```rust
  #![windows_subsystem = "windows"]
  mod app; mod bubble; mod diagnose; mod localization; mod models;
  mod native_interop; mod panel; mod poller; mod settings; mod theme;
  mod tray_icon; mod updater;

  fn main() {
      let args: Vec<String> = std::env::args().collect();
      if args.iter().any(|a| a == "--diagnose") {
          if let Ok(path) = diagnose::init() {
              diagnose::log(format!("startup args={args:?} log_path={}", path.display()));
          }
      }
      if let Some(exit_code) = updater::handle_cli_mode(&args) {
          std::process::exit(exit_code);
      }
      app::run();
  }
  ```

**To delete:** none.

## Implementation Steps

1. **`settings.rs`** — define schema:
   ```rust
   #[derive(Serialize, Deserialize, Default)]
   pub struct Settings {
       pub show_claude_code: bool,    // default true
       pub show_codex: bool,          // default false
       pub bubble_positions: BubblePositions,
       pub poll_minutes: u32,         // default 5
       pub language: Option<String>,  // None = system
       pub start_with_windows: bool,
       pub last_update_check_unix: Option<i64>,
   }
   #[derive(Serialize, Deserialize, Default)]
   pub struct BubblePositions {
       pub claude: Option<(i32, i32)>,
       pub codex: Option<(i32, i32)>,
   }
   pub fn load() -> Settings { /* dirs::config_dir + read + serde + atomic */ }
   pub fn save(s: &Settings) { /* write .tmp + rename */ }
   ```
2. **`panel.rs`** — port the existing horizontal-bar painting code from source `window.rs`:
   - Window class `ClaudeCodeUsageBubblePanel`
   - On `WM_LBUTTONDOWN` outside → close
   - On `WM_KILLFOCUS` → close (with debounce so it doesn't close instantly when bubble is clicked again to toggle off)
   - Paints two rows (5h, 7d) for the model that was clicked; uses `poller::format_line` for the countdown text
   - Position: anchor next to bubble; flip side if would go off-screen
3. **`app.rs`** — orchestrator:
   - `pub fn run() -> !`:
     1. Acquire single-instance mutex; if already running, exit.
     2. `SetProcessDpiAwarenessContext`.
     3. Load settings.
     4. Resolve language (`localization::resolve_language`).
     5. Start polling thread.
     6. For each enabled model, spawn a bubble window thread.
     7. Run main message loop on UI thread (the bubble windows can be on the same thread — easier than multi-thread message pumps).
   - Polling thread:
     ```rust
     loop {
         match poller::poll(show_claude, show_codex) {
             Ok(data) => post_update_to_bubbles(data),
             Err(e) => post_error_to_app(e),
         }
         sleep(poll_interval);
     }
     ```
   - Context menu builder: replicate source's menu structure verbatim; localized strings via `Strings`. Map `WM_COMMAND` IDs to handlers (refresh, toggle model, change interval, etc.).
   - "Start with Windows": registry write to `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run\ClaudeCodeUsageBubble`.
   - Token-expired flow: if `PollError::TokenExpired`, show tray balloon via `tray_icon::notify_balloon` with localized title/body.
4. **Wire `bubble.rs` events back to `app.rs`:**
   - `WM_APP_PANEL_TOGGLE` handler in bubble wndproc → call `panel::show_for_model(model)` (registered via callback or via `app::handle_panel_toggle`).
   - `WM_RBUTTONUP` → call `app::show_context_menu_at(point, model)`.
   - `WM_EXITSIZEMOVE` → call `app::on_bubble_moved(model, x, y)` which updates settings.

5. **Reuse `tray_icon.rs`** as the source-app's secondary indicator:
   - In `app::run`, after creating bubbles, call `tray_icon::sync(hwnd, &[TrayIconData{kind: Claude, percent: …, tooltip: …}, …])`.
   - Tray icon clicked → `tray_icon::handle_message` → if `ToggleWidget`, toggle bubble visibility (set `WS_VISIBLE` style); if `ShowContextMenu`, dispatch to `app::show_context_menu_at`.

## Todo List

- [ ] `settings.rs` with atomic save
- [ ] `panel.rs` with bar painting + auto-close
- [ ] `app.rs` orchestrator
- [ ] Single-instance mutex acquisition + release
- [ ] Polling thread spawning + `PostMessage` updates
- [ ] Context menu localized + dispatchable
- [ ] Start-with-Windows registry roundtrip
- [ ] Tray icons synced from polling updates
- [ ] Bubble-to-app event callbacks wired
- [ ] Dual-bubble mode tested (both Claude + Codex enabled)
- [ ] `cargo build --release` → working binary

## Success Criteria

- Launching the app twice: second launch exits silently
- Bubble + panel + tray icon all show correct usage after first poll
- Right-click menu functional for all items
- Toggle Claude/Codex via menu: bubbles appear/disappear; settings persist
- Restart app: bubble reappears at last saved position
- Token-expired triggers tray balloon
- Poll frequency change takes effect within one poll cycle

## Risk Assessment

- **Medium.** Most logic is structural — message routing and state management. Source repo has all the patterns.
- Risk: dual-bubble on same UI thread with two HWNDs — should work, but verify message routing keys off `hwnd` parameter.
- Risk: panel auto-close races with bubble re-click. Mitigation: 200 ms debounce on `WM_KILLFOCUS` before destroying panel; if bubble clicked within that window, cancel close.

## Security Considerations

- Settings file written to `%APPDATA%`, user-scoped, no privileged ops.
- Single-instance mutex name `Global\ClaudeCodeUsageBubble` — distinct from source app to allow coexistence.

## Next Steps

→ Phase 5: polish (HiDPI testing, multi-monitor, README, attribution, etc.)
