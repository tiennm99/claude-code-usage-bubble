# UI Rendering Code Review

Scope: `src/bubble.rs` (renderer), `src/usage_color.rs`, `src/os/color.rs`,
drawing portions of `src/panel.rs`, `src/tray/badge.rs`.

## Findings

1. **`src/bubble.rs:1178-1197` — severity: high.** Eight neutral surface
   colours (`#1F1F1F`, `#F3F3F3`, `#3A3A3A`, `#D6D6D6`, `#303030`, `#E0E0E0`,
   `#9A9A9A`, `#777777`) are hex literals inside `paint_bubble_pixmap` and
   duplicated near-verbatim in `panel.rs:279-293` (`#1F1F1F`, `#FAFAFA`,
   `#EAEAEA`, `#3A3A3A`, `#D6D6D6`) plus `bubble.rs:1590-1597` text colours.
   Three surfaces silently disagree (`#F3F3F3` bubble bg vs `#FAFAFA` panel
   bg) and a designer cannot retune the palette without grepping. Fix:
   centralise into a `palette` module alongside `usage_color.rs` exposing
   `bg(is_dark)`, `track(is_dark)`, `time_track(is_dark)`, `time_fill(is_dark)`,
   `text(is_dark)`, `muted(is_dark)`; have `panel.rs` consume the same helpers.

2. **`src/tray/badge.rs:53, 112, 114` — severity: high.** Badge colours go
   through `paint.set_color_rgba8(0x3a, 0x3a, 0x3a, 255)` and raw
   `[u8; 3]` arrays instead of `Rgb` / `os::color`. `#3A3A3A` already exists
   as the shared "track" colour in `bubble.rs:1184`, but the tray hard-codes
   it. The two Claude/Codex base tints (`#2A1F1C`, `#1A1F26`) also live only
   here. Fix: route through the same palette module, even if the badge keeps
   its own dark inner-disk variants (named constants beat magic byte arrays).

3. **`src/bubble.rs:1064-1117` — severity: high.** Padding/gap literals
   `scale_to_dpi(2|4|5|6|8|12, dpi)` appear 15+ times inside
   `compute_bubble_layout` with no naming. Same logical "edge padding"
   (`scale_to_dpi(4, dpi)`) is used for `head_pad`, head-label left/right,
   head-pct left/right (lines 1064, 1086, 1088, 1092, 1094); same "small
   nudge" (`scale_to_dpi(2, dpi)`) is the ring stroke clamp, label/pct
   row vertical breathing room, pct-reserve gap, and time-text padding. A
   designer tweaking head-text padding will touch four lines and miss the
   fifth. Fix: hoist named DPI-scaled constants at the top of the function
   (`HEAD_PAD`, `TEXT_VPAD`, `LANE_GAP`, `TAIL_PAD`, `RIGHT_INSET`,
   `BAR_MIN_W`) and reuse — same pattern `panel.rs` uses with its
   `*_LOGICAL` constants (line 24-30).

4. **`src/panel.rs:333-340` — severity: med.** Bar-x / bar-w / row-y math
   mixes `scaled(PADDING_LOGICAL)`, `scaled(LABEL_W_LOGICAL)`,
   `scaled(RIGHT_TEXT_W_LOGICAL)` with bare `scaled(4)`, `scaled(8)`,
   `scaled(24)`, `scaled(18)` — four un-named "small" values doing
   semantically distinct jobs (label-bar gap, bar-text gap, header height,
   row-1 offset). Fix: name them (`LABEL_BAR_GAP_LOGICAL`,
   `BAR_TEXT_GAP_LOGICAL`, `HEADER_H_LOGICAL`, `HEADER_OFFSET_LOGICAL`) so
   the row geometry is auditable in one place.

5. **`src/panel.rs:340` — severity: med.** `row2_y = row1_y +
   scale_to_dpi(BAR_HEIGHT_LOGICAL, dpi) + scale_to_dpi(ROW_GAP_LOGICAL, dpi)
   + scaled(8)`. The trailing `+ scaled(8)` is an unexplained extra gap on
   top of `ROW_GAP_LOGICAL`; this is exactly the inconsistency `ROW_GAP_LOGICAL`
   was created to prevent. Fix: fold into `ROW_GAP_LOGICAL` (16) or rename
   the extra into a labelled `ROW_TEXT_GAP_LOGICAL`.

6. **`src/bubble.rs:1099, 1126` — severity: med.** `tail_right = width_px -
   scale_to_dpi(12, dpi)` and `bar_right = (text_left - pad).max(bar_left +
   bar_min)`. The `12` is the right-edge inset to clear the stadium's right
   end-cap; this is conceptually `corner_radius / 2`-ish but encoded as a
   constant that won't track if aspect ratio changes. Fix: derive from
   `layout.corner_radius` or hoist a `TAIL_RIGHT_INSET` constant with a
   comment tying it to the end-cap curvature.

7. **`src/bubble.rs:1360-1377` and `src/tray/badge.rs:86-106` — severity:
   med.** `build_arc` is duplicated verbatim between the bubble renderer
   and the tray badge — same 64-segment sampling, same `FRAC_PI_2` start,
   same edge-case `.max(1)` segment count. Fix: lift into a small
   `geometry` / `tiny_skia_helpers` module shared by both call sites;
   change neither call site to keep behaviour identical.

8. **`src/bubble.rs:1339-1352` — severity: med.** `paint_pill` is a perfect
   helper candidate for `panel.rs`'s bar drawing — `panel.rs` uses
   `FillRect` rectangles with hard corners (`draw_row` lines 406-428),
   visually inconsistent with the bubble's rounded pill caps. Fix: extract
   `paint_pill` to a shared rendering helper module and have panel use it
   so the two surfaces have matching bar geometry. (Cross-surface
   consistency was the stated reason for `usage_color.rs` existing — same
   logic applies to bar shape.)

9. **`src/bubble.rs:1080-1083, 1111-1112` — severity: med.** `head_label_h`,
   `head_pct_h`, `time_text_h`, `usage_pct_h` all add `scale_to_dpi(2, dpi)`
   of "breathing room" to a font height, but never call it that — and the
   computed rect height is then used by `DrawTextW` with `DT_VCENTER` so a
   too-tight value would clip ascenders/descenders. Currently safe because
   2 px (logical) ≈ font leading, but the magic `2` is load-bearing. Fix:
   `const FONT_VPAD_LOGICAL: i32 = 2;` with a one-line comment "ascender/
   descender slack for DT_VCENTER".

10. **`src/bubble.rs:1141-1146, 1153-1158` — severity: low.** The vertical
    centring expression `usage_bar_top + (usage_bar_h - usage_pct_h) / 2`
    is computed twice for `tail_usage_pct_rect` (top + bottom). Tiny but
    if a designer asks "where does the % text sit relative to the bar?"
    they have to mentally simplify. Fix: compute `pct_text_top` /
    `time_text_top` as named locals before the struct literal.

11. **`src/bubble.rs:1076, 1077` — severity: low.** `big_font_px =
    head_diameter * 26 / 100`; `small_font_px = big_font_px * 55 / 100`.
    The 26 % and 55 % ratios are the core typographic scale of the head
    text — promote to `BIG_FONT_RATIO_PCT`, `SMALL_TO_BIG_FONT_PCT`
    constants with a "tweak these to retune head proportions" comment.

12. **`src/bubble.rs:1078` — severity: low (dead-code adjacent).**
    `main_font_px = small_font_px;` — `main_font_px` is identical to
    `small_font_px` but kept as a separate field on `BubbleLayout`
    (line 1056) and used for the countdown (line 1604, 1658). If the
    intent is "may diverge in future", document it; otherwise drop the
    duplicate field and use `small_font_px` directly.

13. **`src/bubble.rs:1085-1096` — severity: low.** `head_label_rect` and
    `head_pct_rect` both use `left: scale_to_dpi(4, dpi)` and `right:
    head_diameter - scale_to_dpi(4, dpi)` — identical horizontal extents.
    Could share a single `head_text_left`/`head_text_right` pair to make
    "head text is centered in the head circle" structurally visible.

14. **`src/bubble.rs:1216 vs 1339-1352` — severity: low.** Stadium
    background uses inline two-circle-plus-rect path; the pill helper
    does the same shape. The stadium could call `paint_pill(pixmap, 0.0,
    0.0, w, h, h/2.0, bg)` and shed ~15 lines. Worth doing once
    `paint_pill` moves to a shared module (finding 8).

15. **`src/bubble.rs:1653` — severity: low (text layout).**
    `draw_tail_text_in_rect` is called with `DT_RIGHT | DT_VCENTER |
    DT_SINGLELINE | DT_END_ELLIPSIS`. The `DT_END_ELLIPSIS` on a
    right-aligned 3-char string ("100%") inside a tight rect will produce
    `1…` if the rect collapses by even a pixel — fine, but worth
    confirming the `pct_reserve_w` (line 1103) leaves a 1-px AA safety
    margin. Current `+ scale_to_dpi(2, dpi)` looks adequate. No fix
    needed; flag for future locale changes.

16. **`src/bubble.rs:1674-1685` — severity: low.** `draw_text_in_rect`
    always uses `DT_NOCLIP`; `draw_tail_text_in_rect` uses
    `DT_END_ELLIPSIS` (no `DT_NOCLIP`). The two helpers diverge silently
    on whether text may escape its rect. Document the contract on each
    helper ("head text trusts layout, tail text fits-or-ellipsises").

17. **`src/panel.rs:447-501` — severity: low.** `draw_text` creates and
    destroys a font on every call (4× per `paint`). Same anti-pattern in
    `bubble.rs:paint_bubble_text` is amortised by caching `big_font`,
    `small_font`, `main_font` for the whole paint. Not a correctness
    issue but means the panel allocates 4 GDI fonts on every
    InvalidateRect. Cache by `(size, bold)` keyed on the HDC.

## Quick wins

1. **Centralise the neutral palette** (findings 1, 2). One new module
   `palette.rs` exposing `bg / track / time_track / time_fill / text /
   muted` plus tray-specific tints. Replace all `Color::from_hex(...)`
   calls in `bubble.rs:1178-1197`, `bubble.rs:1589-1598`,
   `panel.rs:279-293`, and the byte arrays in `tray/badge.rs`. ~20-line
   diff, kills cross-surface drift.

2. **Name the padding constants in `compute_bubble_layout`** (finding 3,
   9). Add ~6 `const` declarations at the top of the function — keeps
   them locally scoped, matches `panel.rs` style. Designer can retune
   metrics from one block.

3. **Lift `build_arc` to a shared `tiny_skia_helpers` module** (finding
   7). Two-file delete-and-import. Identical behaviour.

4. **Promote `paint_pill` and reuse in `panel.rs` rows + stadium bg**
   (findings 8, 14). Makes bar visual style consistent between bubble
   and panel; bonus simplification of the stadium fill.

5. **Drop / rename `main_font_px`** (finding 12). Either delete the field
   and use `small_font_px` directly, or split the constants (`MAIN_FONT_RATIO`)
   so future divergence is intentional.

## Defer

- Finding 4-6 (panel/bubble padding naming, derived `TAIL_RIGHT_INSET`):
  worth doing alongside the palette refactor but not blocking.
- Finding 10, 13 (local temporaries for centring math, shared
  `head_text_*` extents): pure readability, low payoff.
- Finding 15 (ellipsis safety margin on locale changes): keep an eye on
  this when adding a locale wider than `999시간`.
- Finding 17 (panel GDI font caching): only matters if the panel starts
  refreshing more often than once per poll cycle.

Out-of-scope sighting (one-line flag as instructed): `compute_bubble_layout`
takes an `HDC` purely to measure text — couples geometry calc to a live GDI
device. Not a render bug, but it makes the function untestable without a
window and is worth refactoring when convenient.

**Status:** DONE
**Summary:** Renderer is functionally solid; the dominant problem is
duplicated/un-named geometry and colour literals scattered across bubble /
panel / badge that drift independently and resist designer-led tuning.
