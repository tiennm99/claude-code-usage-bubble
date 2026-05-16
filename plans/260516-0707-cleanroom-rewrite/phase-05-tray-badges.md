---
phase: 5
status: pending
estimated_hours: 5
---

# Phase 5 — Tray badges with tiny-skia

## Context links

- Brainstorm: axis 7 (tray icon drawing)
- Source file to be REPLACED entirely: `src/tray_icon.rs` (441 LOC)

## Overview

- **Priority:** Medium — replaces a non-critical-path file but visibly improves UX (anti-aliased badges).
- **Status:** pending
- **Brief:** Replace GDI-drawn tray icons with `tiny-skia` path-rendered, anti-aliased badges. Wrap the Win32 tray-notification calls (`Shell_NotifyIconW`) in a new `tray/` module with cleaner add/update/remove semantics.

## Key insights from brainstorm

- Source's GDI rendering uses primitive rectangles + text. Output looks aliased on HiDPI.
- `tiny-skia` renders vector paths with anti-aliasing in pure Rust. ~200 KB added to binary; UX clearly better.
- Tray icons are 16×16 / 24×24 / 32×32 depending on DPI. Render at largest size, downsample.
- The tray-icon "add vs update" inefficiency flagged in Phase 4 code review (R5) is fixed here by tracking which icons are registered in module state.

## Requirements

### Functional

- `tray::Manager::new(owner_hwnd: HWND) -> Self`.
- `manager.sync(state: &[TrayIcon])` adds/updates/removes icons to match the given state.
- `manager.notify(id: TrayIconId, title: &str, body: &str)` shows a balloon for an existing icon.
- `tray::badge::render(percent: Option<f64>, kind: BadgeKind, dpi: u32) -> HICON` produces an anti-aliased HICON.
- `tray::callback::handle(lparam: LPARAM) -> TrayAction` dispatches WM_APP_TRAY messages.
- `TrayIcon { id: TrayIconId, percent: Option<f64>, tooltip: String, kind: BadgeKind }`.

### Non-functional

- Badge render must complete in < 5 ms per icon (called on every poll cycle, ~1× per minute typically).
- Memory: each cached badge HICON is ~4 KB; we cache by `(percent_bucket, kind, dpi)` — at most ~100 entries × 4 KB = 400 KB cache size.

## Architecture

### `src/tray/mod.rs`

```rust
use windows::Win32::Foundation::*;

pub mod badge;
pub mod callback;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BadgeKind {
    Claude,
    ChatGpt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TrayIconId(pub u32);

pub const ID_CLAUDE: TrayIconId = TrayIconId(1);
pub const ID_CHATGPT: TrayIconId = TrayIconId(2);

#[derive(Clone, Debug)]
pub struct TrayIcon {
    pub id: TrayIconId,
    pub percent: Option<f64>,
    pub tooltip: String,
    pub kind: BadgeKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    None,
    LeftClick(TrayIconId),
    RightClick(TrayIconId),
}

pub struct Manager {
    owner: HWND,
    registered: std::collections::HashSet<TrayIconId>,
}

impl Manager {
    pub fn new(owner: HWND) -> Self {
        Self { owner, registered: Default::default() }
    }

    pub fn sync(&mut self, state: &[TrayIcon]) {
        let target_ids: std::collections::HashSet<_> = state.iter().map(|i| i.id).collect();
        // Remove icons not in state
        let to_remove: Vec<_> = self.registered.difference(&target_ids).copied().collect();
        for id in to_remove { self.remove(id); }
        // Add or update
        for icon in state {
            if self.registered.contains(&icon.id) {
                self.update(icon);
            } else {
                self.add(icon);
            }
        }
    }

    pub fn notify(&self, id: TrayIconId, title: &str, body: &str) { /* NIM_MODIFY with NIF_INFO */ }

    fn add(&mut self, icon: &TrayIcon) { /* NIM_ADD + Shell_NotifyIconW */ }
    fn update(&mut self, icon: &TrayIcon) { /* NIM_MODIFY */ }
    fn remove(&mut self, id: TrayIconId) { /* NIM_DELETE */ }
}
```

### `src/tray/badge.rs`

```rust
use windows::Win32::UI::WindowsAndMessaging::HICON;
use tiny_skia::*;

pub fn render(percent: Option<f64>, kind: super::BadgeKind, dpi: u32) -> Option<HICON> {
    let size = match dpi {
        d if d >= 192 => 32,
        d if d >= 144 => 24,
        _ => 16,
    };
    let mut pixmap = Pixmap::new(size, size)?;

    // 1. Background fill (gradient from kind's tint colors)
    let bg_color = base_color_for(kind, percent);
    fill_circle(&mut pixmap, size, bg_color);

    // 2. Ring sweep for percent
    if let Some(p) = percent {
        draw_arc(&mut pixmap, size, p, kind);
    }

    // 3. Center text "%" with size auto-fit
    if let Some(p) = percent {
        draw_percent_text(&mut pixmap, size, p);
    }

    // 4. Convert BGRA pixmap to HICON via CreateIconIndirect
    pixmap_to_hicon(&pixmap)
}

fn fill_circle(pixmap: &mut Pixmap, size: u32, color: Color) { /* … */ }
fn draw_arc(pixmap: &mut Pixmap, size: u32, percent: f64, kind: super::BadgeKind) { /* … */ }
fn draw_percent_text(pixmap: &mut Pixmap, size: u32, percent: f64) {
    // tiny-skia doesn't render text natively. Two options:
    //   a) Use cosmic-text (heavy) or ab_glyph (lighter).
    //   b) Pre-rasterize digits 0-9 + % glyph at build time into tiny PNGs and embed.
    //   c) Skip text — just use the ring sweep for usage indication.
    // Choose (c) for simplicity; the bubble shows the exact percent already.
}

fn pixmap_to_hicon(pixmap: &Pixmap) -> Option<HICON> {
    // Win32 ICONINFO with mask + color bitmaps; bitmaps from CreateDIBSection.
    // BGRA layout matches what tiny-skia produces (premultiplied alpha).
    // …
}
```

**Decision:** drop text from tray badges entirely. The bubble shows the exact percentage; tray badge is a coarse indicator (color + ring fill). This sidesteps the tiny-skia text-rendering hassle and keeps the badge image clearer at 16×16.

### `src/tray/callback.rs`

```rust
use windows::Win32::Foundation::LPARAM;
use super::{TrayAction, TrayIconId};

const WM_LBUTTONUP: u32 = 0x0202;
const WM_RBUTTONUP: u32 = 0x0205;

pub fn handle(lparam: LPARAM) -> TrayAction {
    let raw = lparam.0 as u32;
    let event = raw & 0xFFFF;
    let id_lo = (raw >> 16) & 0xFFFF;
    let id = TrayIconId(id_lo);
    match event {
        WM_LBUTTONUP => TrayAction::LeftClick(id),
        WM_RBUTTONUP => TrayAction::RightClick(id),
        _ => TrayAction::None,
    }
}
```

### Cargo.toml addition

```toml
tiny-skia = "0.11"
```

(~250 KB added to binary; no text dependency since we dropped text from badges.)

## Related code files

**To create:**
- `src/tray/mod.rs`
- `src/tray/badge.rs`
- `src/tray/callback.rs`

**To modify:**
- `Cargo.toml` — add `tiny-skia`
- `src/main.rs` — `mod tray;`; remove `mod tray_icon;`
- `src/app.rs` — replace `crate::tray_icon::{sync, add, update, remove, notify_balloon, handle_message, ...}` with `crate::tray::{Manager, TrayIcon, BadgeKind, TrayAction, ID_CLAUDE, ID_CHATGPT}`. Store `Manager` in `AppState`. Update all call sites.

**To delete:**
- `src/tray_icon.rs`

## Implementation steps

1. **Add `tiny-skia` dep**.
2. **Implement `badge.rs`** — start with `fill_circle` (one path), verify pixmap saves to PNG correctly for visual debugging.
3. **Add `draw_arc`** — `PathBuilder::move_to + arc_to`. Use 0° = top (12 o'clock).
4. **Implement `pixmap_to_hicon`** — this is the trickiest part. Create AND/XOR DIB sections, populate from pixmap pixels (premultiplied BGRA), build `ICONINFO`, call `CreateIconIndirect`.
5. **Test badge rendering** — save 10 sample HICONs at different percents to disk and inspect.
6. **Implement `Manager`** with add/update/remove/sync.
7. **Implement `callback.rs`**.
8. **Migrate `app.rs`** to new API; replace `tray_icon::sync(...)` with `state.tray.sync(&icons)`.
9. **Delete `src/tray_icon.rs`** and remove from `main.rs`.
10. **`cargo build --release`** clean.
11. **Windows e2e**: run app, see tray icons appear with anti-aliased ring. Hover for tooltip. Left-click toggles bubble. Right-click opens menu.

## Todo checklist

- [ ] `tiny-skia` added to Cargo.toml
- [ ] `badge.rs::fill_circle` works (PNG inspection)
- [ ] `badge.rs::draw_arc` works (PNG inspection)
- [ ] `badge.rs::pixmap_to_hicon` produces valid HICON
- [ ] `tray/mod.rs::Manager` with add/update/remove/sync
- [ ] `callback.rs::handle` returns correct TrayAction
- [ ] `app.rs` migrated to new tray API
- [ ] `tray_icon.rs` deleted
- [ ] `cargo build --release` clean
- [ ] Tray icons appear with anti-aliased visuals on Windows

## Success criteria

- Badge looks visibly smoother than source's GDI version (anti-aliased ring).
- Add/update/remove is idempotent (no duplicate icons after `sync`).
- Tray callbacks fire correctly for left/right click.
- No `src/tray_icon.rs` remains.

## Risks + mitigations

| Risk | Likelihood | Mitigation |
|---|---|---|
| `pixmap_to_hicon` produces wrong-format icon (alpha channel issues) | High | Test by saving the source pixmap as PNG, then comparing to the rendered icon; iterate on BGRA channel order |
| `CreateIconIndirect` requires monochrome mask bitmap; we only have color | Medium | Pass `hbmMask = NULL` to let Windows auto-generate from alpha (works on Win10+) |
| 16×16 looks bad even with AA | Medium | Render at 32×32 then downsample with high-quality lanczos (tiny-skia doesn't include downsampling — use `image` crate's resize) |
| `tiny-skia` adds too much binary size | Low | Measured ~250 KB; acceptable for the UX win |

## Security considerations

- No external input drives badge rendering — all params are internal (`percent`, `kind`, `dpi`). No injection surface.
- HICON handles must be `DestroyIcon`'d when cache evicts (avoid handle leak — Windows limit is ~10,000 per process).

## Next steps

→ Phase 6: replace `updater.rs` and drop `NOTICE`.

## Open questions

- Keep the cached HICONs alive for the process lifetime, or destroy aggressively on each `update`? Source destroys + recreates each cycle (wasteful but simple). Recommend: cache by `(percent_rounded_to_5pct, kind, dpi)` and let the cache grow naturally. Max size ~100 entries × 4 KB = 400 KB.
- Need an icon for "no data" state (percent = None). Current spec says "fill_circle + no ring". Verify the visual reads correctly.
