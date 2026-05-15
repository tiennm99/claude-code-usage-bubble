# Phase 3: Build Floating Bubble Window

## Context Links

- Source `window.rs` painting + drag logic: `/config/workspace/CodeZeno/Claude-Code-Usage-Monitor/src/window.rs` (lines around `UpdateLayeredWindow`, `WM_LBUTTONDOWN`, `SetCapture`)
- Reference UX: 360 Security floating ball, IObit Advanced SystemCare RAM-boost ball — both are circular, top-most, draggable-anywhere with edge snap.

## Overview

- **Priority:** Critical — this is the heart of the new UX
- **Status:** pending
- **Description:** Build a circular floating bubble window that floats on top of everything, can be dragged anywhere, snaps to monitor edges, and shows usage percentage in the center over a colored progress ring. Replaces the 2847-line `window.rs` taskbar embedding code.

## Key Insights

- Use `WS_POPUP | WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` — `TOOLWINDOW` keeps it out of Alt+Tab; `NOACTIVATE` prevents focus theft.
- Drag-anywhere = handle `WM_NCHITTEST`, return `HTCAPTION` for the entire bubble area. The OS handles drag automatically, including with proper cursor and Win+drag behavior. No need for `SetCapture`.
- Circular alpha mask: render to a DIB section with per-pixel alpha. Pixels outside the circle = `0x00000000` (fully transparent). Then `UpdateLayeredWindow` with `ULW_ALPHA`. Click-through outside the circle happens automatically because alpha=0 doesn't hit-test (default behavior of layered windows with `WS_EX_LAYERED` + per-pixel alpha; verify via test).
- Snap to edge: in `WM_EXITSIZEMOVE`, query current position via `GetWindowRect`, find nearest monitor via `MonitorFromPoint`, get its work area via `GetMonitorInfo`. If center is within 12px of any work-area edge, snap that edge.
- HiDPI: use `SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)` early in `main`. Query `GetDpiForWindow` on `WM_DPICHANGED` and rescale bubble size + font.
- GDI ring drawing: parametric — sweep angle proportional to percentage. Use `Polygon` filled with brush, or `AngleArc` with thick pen. For clean anti-aliased look on layered window, draw into DIB section manually with alpha-weighted line algorithm. Simpler path: use GDI+ via `gdiplus` crate, or accept GDI-aliased look for v1.
- Bubble size default: **56×56 px** at 100% DPI (matches reference apps). Allow user to tweak in `Cargo.toml` constant for v1.

## Requirements

### Functional
- Window appears as a circular bubble, can be dragged anywhere on any monitor
- Bubble shows percentage text (e.g. "73%") in center
- Colored progress ring around the percentage; color matches the source app's color stops (orange → red gradient from 50% to 100%)
- Top-most: stays visible over other windows
- No taskbar entry, no Alt-Tab entry
- Left-click → posts `WM_APP_PANEL_TOGGLE` to self (panel implementation in phase 4)
- Right-click → context menu (phase 4 owns menu items; bubble owns the right-click detection)
- Drag releases → snap to nearest monitor edge if within 12 px of it
- Repaints when percentage / theme / DPI changes

### Non-functional
- 60 FPS not required; redraws on data update only (every 60s poll cycle plus countdown ticks)
- Per-monitor DPI aware
- Visible on dark and light Windows themes (use `theme::is_dark_mode` for ring background tint)

## Architecture

```
src/
├── bubble.rs              NEW — Window class, message loop owner, GDI painting,
│                          drag + snap, DPI, hit-testing
└── (other modules unchanged from phase 2)
```

Bubble owns:
- HWND lifecycle (`RegisterClassExW` + `CreateWindowExW`)
- DIB section + `UpdateLayeredWindow` call
- Percentage state (`Option<f64>` for each enabled model)
- Drag state (managed by OS via `HTCAPTION`)
- Snap math
- DPI scale factor cache

Bubble delegates:
- Polling → `poller::poll` (phase 4 wires the background thread)
- Panel toggle → `app::on_panel_toggle` (phase 4)
- Right-click menu → `app::on_show_context_menu` (phase 4)
- Settings → `settings` module (phase 4)

## Related Code Files

**To create:**
- `src/bubble.rs` (target: 400–700 lines)

**To modify:**
- `src/main.rs` — eventually call `bubble::run()` (wired in phase 4)
- `src/native_interop.rs` — may add helpers if hit-testing geometry math gets gnarly

**To delete:** none.

## Implementation Steps

1. **Window class registration:**
   - Class name: `ClaudeCodeUsageBubble`
   - Style: `CS_DBLCLKS` (allow `WM_LBUTTONDBLCLK` if we want double-click later)
   - WndProc: `bubble_wnd_proc`
2. **Window creation:**
   - `WS_POPUP`, ext `WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`
   - Initial position: load from settings (phase 4); fall back to "near bottom-right corner of primary monitor"
   - Size: 56×56 logical px scaled by current DPI
3. **DPI awareness:**
   - In `bubble::run`, call `SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)`.
   - On `WM_CREATE`, cache DPI via `GetDpiForWindow`.
   - On `WM_DPICHANGED`, update scale and resize.
4. **Painting (the hard part):**
   - On every state change (percentage, DPI, theme), call `redraw()`.
   - `redraw()`:
     - Create DIB section sized to bubble pixel dimensions (`CreateDIBSection` with `BI_RGB` and 32bpp).
     - Clear to fully transparent (`0x00000000`).
     - For each pixel inside the circle radius, write background fill (theme-adjusted: dark theme → semi-opaque dark with high alpha; light theme → semi-opaque white).
     - Stroke progress ring: for the sweep angle proportional to current percentage, draw a thick arc using either GDI `AngleArc` with rounded `Pen`, or manual pixel writes (4 px ring thickness at 100% DPI).
     - Draw percentage text in center via `DrawTextW` with `DT_CENTER | DT_VCENTER | DT_SINGLELINE`. Font: bold 14 pt at 100% DPI, scaled by DPI factor.
     - Call `UpdateLayeredWindow` with `ULW_ALPHA` and the DIB.
5. **Drag-anywhere via `WM_NCHITTEST`:**
   ```rust
   WM_NCHITTEST => {
       // Convert lparam (screen coords) to client coords
       let p = screen_to_client(hwnd, lparam);
       if inside_circle(p, radius) { LRESULT(HTCAPTION as isize) }
       else { LRESULT(HTTRANSPARENT as isize) }
   }
   ```
   OS handles drag + cursor. `HTTRANSPARENT` outside the circle ensures clicks pass through.
6. **Snap on drag release:**
   - `WM_EXITSIZEMOVE` → snap logic.
   - `MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST)` → `GetMonitorInfo` → work area rect.
   - Compare bubble's center to each edge of work area. If distance < 12 logical px (scaled by DPI), adjust window position to snap.
   - Persist new position via `app::on_bubble_moved(model, x, y)` (phase 4).
7. **Click handling:**
   - `WM_LBUTTONUP` → if no drag occurred (compare with `WM_LBUTTONDOWN` position), `PostMessageW(WM_APP_PANEL_TOGGLE)`.
   - `WM_RBUTTONUP` → call into `app::show_context_menu(hwnd, screen_pos)` (phase 4 implements).
8. **Public API:**
   ```rust
   pub fn run(initial: BubbleConfig) -> ! { /* never returns; spins message loop */ }
   pub struct BubbleConfig {
       pub model: TrayIconKind,    // Claude or Codex
       pub initial_position: Option<(i32, i32)>,
       pub initial_percentage: Option<f64>,
   }
   pub fn update_percentage(hwnd: HWND, percentage: Option<f64>);  // called from poll thread via PostMessage
   ```
   For dual-bubble mode, phase 4 spawns one `bubble::run` per enabled model on separate threads (each with its own message loop) — simpler than juggling two HWNDs in one thread.

## Todo List

- [ ] Window class + creation with correct styles
- [ ] Per-monitor DPI awareness on entry
- [ ] DIB section + layered window painting pipeline
- [ ] Circle fill with theme-aware background
- [ ] Progress ring painted at correct sweep angle, correct color stop
- [ ] Percentage text drawn centered
- [ ] `WM_NCHITTEST` returns `HTCAPTION` inside circle, `HTTRANSPARENT` outside
- [ ] Drag works smoothly across monitors
- [ ] Snap on release within 12px of work-area edge
- [ ] Left-click (no drag) posts panel-toggle message
- [ ] Right-click posts context-menu request
- [ ] Public API `run`, `update_percentage`
- [ ] Manual test: bubble visible, draggable, snaps, percentage updates

## Success Criteria

- Bubble appears on Windows 10/11 with a Visual Studio-clean cargo build
- Drag works smoothly with no flicker
- Edge snap engages reliably from 12 px
- Bubble survives display reconnection (laptop → external monitor → unplug)
- Percentage text remains crisp on 100%, 125%, 150%, 175% DPI

## Risk Assessment

- **High** — this is novel code with no exact analog in source.
- Risk: ClearType sub-pixel text rendering on a per-pixel-alpha layered window looks bad. Source repo's `window.rs` solved this with a black background-pixel hack (`alpha = 0x01` so it's nearly transparent but still gets ClearType). Apply the same trick for the circle's interior fill region.
- Risk: GDI `AngleArc` doesn't anti-alias. Mitigation: either accept aliased v1, or render to a 2x supersampled DIB and downsample.
- Risk: Snap math wrong on rotated taskbar or unusual DPI configurations. Mitigation: clamp to monitor work area only, ignore taskbar position.

## Security Considerations

- N/A in this phase; bubble does not handle user input beyond mouse position and clicks.

## Next Steps

→ Phase 4: expanded panel, settings persistence, polling thread, orchestration
