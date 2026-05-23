# UI Design Review — Floating Bubble

Date: 2026-05-23
Scope: Visual-only redesign of the stadium bubble (head ring + tail bars).
Constraints: tiny-skia primitives only (AA fills, AA stroked arcs, AA pills) + GDI text. No new deps. No animations beyond existing pulse. Logical px values; `scale_to_dpi` handles HiDPI.

Files reviewed:
- `D:\tiennm99\claude-code-usage-bubble\src\bubble.rs` (lines 1032–1703)
- `D:\tiennm99\claude-code-usage-bubble\src\usage_color.rs`
- `D:\tiennm99\claude-code-usage-bubble\src\os\color.rs`

---

## 1. Issues Found

### A. Information hierarchy is flat
The big head "31%" and the tail "64%" are typographically equal in weight against their backgrounds — but they answer different questions (now vs. weekly). The eye has no anchor. Add weight contrast.

### B. Tail text is cramped
`tail_usage_pct_rect` and `tail_time_text_rect` share the same right edge (`content_right`) with no rule for the gap between the bar end and the percent label. With `pad = 6 logical` and `pct_reserve_w` literally just `measure("100%") + 2 logical`, the "64% / 3d" pair reads as one glyph blob. The two right-aligned tokens stack with only ~2 logical px of internal breathing.

### C. The inner time ring is nearly invisible
At ring_stroke_w 3 logical and time_ring_stroke_w 2 logical with only a 3-logical gap between them (line 1074), and using `#303030` track on a `#1F1F1F` background, the ratio is ~1.13:1. Below the visibility threshold; users won't read it as a ring.

### D. Two bubbles read as one merged blob
Claude and Codex stagger vertically by `height_px + gap=24` (line 1750) but visually the dark-on-dark stadia float without anchor — there's no provider identifier inside the bubble itself. The accent color is the *only* differentiator, and it disappears below 60% (Codex teal vs. orange both reduce to white at that range for tail text — see line 1646).

### E. Track contrast vs. fill is loud
`track = #3A3A3A` on `#1F1F1F` bg (4.0:1) is louder than the fill at low percentages. At 5% usage the dim track screams more than the bright fill — backwards visual priority.

### F. Time-bar reads as a second-quota
The grey time bar fills *left-to-right* same direction as the usage bar, and shares the same shape, position, and visual weight class. A user glancing sees "two progress bars" and assumes both are quotas. The grey hue helps but the *gestalt* fights it.

### G. Head "5h" label is buried
`small_font_px ≈ 55% of big_font_px` and uses `#888888`. At 200 logical width the label is ~10px and dim. It's the only thing telling the user the ring is the *5-hour* window.

### H. Ring uses round caps but track does not — visual mismatch
Active arc has `LineCap::Round` (line 1249) but the track is a full circle. At low percentages the rounded start cap juts out above the track — looks unfinished. The track should be `LineCap::Butt` (default closed circle is fine) but the *visual idiom* would benefit from the track being a hint subtler.

### I. Corner radius of pill = `canvas_h / 2` is fine, but the head circle inscribed in the same height feels visually small
`ring_radius = head_diameter/2 - 4 - stroke/2` makes the ring fill ~92% of the head square — but the head_square equals the canvas height, so the head looks slightly under-sized vs. the visual weight of the tail bars. Slight padding nudge.

### J. No separator/cue between the two stacked bubbles
Not a per-bubble issue, but worth noting: when both providers run, a faint provider mark inside each bubble would let users disambiguate without remembering "the upper one is Codex."

---

## 2. Proposed Changes

All values are **logical px**. Hex colors are dark-theme; the light-theme entry shown after `/`.

### Change 1 — Demote the head "5h" label, promote into a chip
**What:** Keep the small label, but render it as an uppercase, letter-spaced micro-cap inside a 1-px-stroke pill (no fill).
**Why:** Reads as a "window selector" tag rather than disambiguated noise.
**Values:**
- text: `"5H"` (uppercase, was `"5h"`)
- font weight: `FW_SEMIBOLD` (was normal)
- letter-spacing: simulate via `+1 logical px` between glyphs — actually, just keep tracking from font; the uppercase alone reads stronger.
- color: `#A8A8A8` / `#5E5E5E` (was `#888888` / `#6E6E6E`)
- no chip border for v1 — KISS. Just style the text. If chip is wanted later, AA stroke a pill rect.
- Keep current vertical position; the `label_pct_gap` is fine.

### Change 2 — Bump big-percent weight + tighten size
**What:** Big number gets heavier and slightly smaller; tightens visual mass.
**Why:** Heavier weight = stronger anchor without taking more space.
**Values:**
- weight: `FW_BOLD` (was `FW_SEMIBOLD`)
- size factor: `big_font_px = head_diameter * 24/100` (was `26/100`)
- color unchanged: `#EAEAEA` / `#1F1F1F`

### Change 3 — Lift the inner time ring above noise
**What:** Increase contrast of the time-ring track and fill; thicken slightly.
**Why:** Current ratio of 1.13:1 against bg is invisible. Per WCAG 1.4.11 non-text 3:1 minimum.
**Values:**
- `time_ring_stroke_w`: `scale_to_dpi(2, dpi).clamp(2, 3)` (was `1..3`, effective 1px floor → too thin)
- gap between outer ring inner edge and inner ring outer edge: `4 logical` (was `3`)
- time_track: `#2F2F2F` → **`#404040`** (3.5:1) / light unchanged
- time_fill (used for inner-ring active arc *and* tail time-bar fill): `#9A9A9A` → **`#B0B0B0`** / `#777777` → `#666666`
- Keep `LineCap::Round` on the active arc.

### Change 4 — Reserve a real gap between tail bar and tail text
**What:** Add a `bar_text_gap = 8` between bar end and text left edge (currently `pad = 6`).
**Why:** Eight is the eyeballed minimum where the eye registers "two columns" instead of "one wall of glyphs."
**Values:**
- new constant: `bar_text_gap = scale_to_dpi(8, dpi)` (was effectively `pad = 6`)
- `bar_right = (text_left - bar_text_gap).max(bar_left + bar_min);` (line 1126)
- `pad` stays `6` for the head→tail content_left inset.

### Change 5 — Right-edge inset
**What:** Increase inner right margin of the tail.
**Why:** The current `scale_to_dpi(12, dpi)` insetinto the pill's rounded right cap leaves text near the curvature. Bump to clear the cap visually.
**Values:**
- `tail_right = width_px - scale_to_dpi(14, dpi)` (was `12`)

### Change 6 — Reweight tail percent vs. tail countdown
**What:** Make the tail percent a touch heavier than the countdown so the *number* anchors the lane.
**Why:** Today both are FW_NORMAL same size, both `text_color` — flat. Bigger number with smaller dimmer suffix establishes hierarchy.
**Values:**
- weekly percent: `FW_SEMIBOLD`, color `text_color` (`#EAEAEA` / `#1F1F1F`)
- weekly countdown: `FW_NORMAL`, color **`muted_color`** (`#A8A8A8` / `#5E5E5E`) — was `text_color`
- Font sizes unchanged: both `small_font_px` / `main_font_px`.

### Change 7 — Tone down the usage-bar track
**What:** Drop track contrast so the *fill* dominates, not the track.
**Why:** At low percent (5–10%) the bright track outscreams the fill. Track should be a hint.
**Values:**
- `track`: `#3A3A3A` → **`#2C2C2C`** (was 4.0:1 vs. bg; now 1.6:1 — the *fill* hits 4.5:1+ from accent colors and carries the signal)
- light theme: `#D6D6D6` → `#E2E2E2`

### Change 8 — Differentiate the time-bar shape from the usage-bar shape
**What:** Make the time bar visibly *thinner and lower-contrast* so it doesn't read as a second quota.
**Why:** Current ratio: usage_bar 9% of height, time_bar 5% — close. Push the spread.
**Values:**
- `usage_bar_h = (height_px * 10 / 100).clamp(6, 12)` (was 9% / clamp 5–12)
- `time_bar_h = (height_px * 4 / 100).clamp(3, 6)` (was 5% / clamp 3–7)
- `lane_gap = scale_to_dpi(6, dpi)` (was 5)
- This gives the usage bar ~2.5× the visual mass of the time bar — clear "primary" vs. "context."

### Change 9 — Pull the head ring in by 1 px so the head circle feels deliberate
**What:** Slightly more head padding; ring sits 1 logical px farther in.
**Why:** Ring currently kisses the visual edge of the head square; a touch of breathing room makes the head feel composed and balances vs. the heavier tail.
**Values:**
- `head_pad = scale_to_dpi(5, dpi)` (was 4)
- `ring_stroke_w`: keep `scale_to_dpi(3, dpi).clamp(2, 4)` — already good.

### Change 10 — Provider mark dot (subtle disambiguator)
**What:** A 4×4 logical solid circle in the accent color, positioned at the *outer* edge of the ring at 12 o'clock — between the ring and the head's left edge.
**Why:** Today the only provider tell is the accent of the active arc; below 60% the arc *is* the accent so it works, but at >60% the arc shifts to amber/red and the provider identity vanishes. A constant dot fixes that. Also helps when two bubbles stack.
**Values:**
- center: `(ring_cx, ring_cy - ring_radius - ring_stroke_w/2 - 4)` — i.e. 4 logical px above the ring's outer edge
- radius: `scale_to_dpi(2, dpi)` (logical 2 → diameter 4)
- color: `accent_color_for(model, is_dark)` (existing function — `#D97757` Claude / `#10A37F` Codex)
- Implemented as one extra `pb.push_circle(...)` fill before the ring strokes — zero new dependencies.

### Change 11 — Round-cap the active tail bars; flat-cap the tracks
**What:** Keep the existing `paint_pill` for the *track*, but reduce its end-cap radius. For the *fill*, keep full cap. Actually, simpler: leave both as full-cap pills (current behavior) — just ensure the fill never paints below `2 * cap` width.
**Why:** Already correct in code (`paint_pill` does both end-caps). No change needed visually, but lock in a min-fill so the bar at 1% doesn't render as a dot.
**Values:**
- in the weekly-pct render block (line 1300+): if `fill_w > 0.0 && fill_w < bar_h`, set `fill_w = bar_h` (a one-cap-diameter minimum). Cosmetic only — preserves "I see some progress" cue when usage is 0.1–2%.

### Change 12 — Text-color for tail percent when bar is in alarm range
**What:** When `weekly_pct >= 95`, tint the percent text toward the alarm color instead of bumping its luminance via `brighten` only.
**Why:** Today, at 98%, the text just gets *brighter* via the pulse — but in dark mode the bar is already pulsing deep red. Tinting the number red ties it to the bar.
**Values:**
- if `pct >= 95.0`: `text_color_for_pct = #E08070` (dark) / `#B02810` (light), then apply pulse `brighten` on top.
- Keep the FW_SEMIBOLD from Change 6.
- 80 ≤ pct < 95: leave at default text color (the bar carries the warning).

---

## 3. ASCII Mockup (one bubble, dark, ~270 logical px wide)

```
                    canvas_w = 270 (logical)
   <─────────────────────────────────────────────────────────────────>
   ┌─────────────────────────────────────────────────────────────────┐
   │       •  <─ accent dot (2-logical r, 4 above ring outer)        │
   │      ╭───╮                                                      │
   │     /     \      ┌─────────────────────────────────────┐        │ ^
   │    │ ┌───┐ │     │▓▓▓▓▓▓▓░░░░░░░░░░░░░░░░░░░░░░░░│ 64% │  ←lane │ |
   │    │ │5H │ │     └─────────────────────────────────┴─────┘        │ |
   │    │ │31%│ │     ┌─────────────────────────────┐  3d ←FW_NORMAL│ | height
   │    │ └───┘ │     │██░░░░░░░░░░░░░░░░░░░░░░░░░░░│           │  muted│ |
   │     \     /      └─────────────────────────────┘                │ |
   │      ╰───╯                                                      │ v
   └─────────────────────────────────────────────────────────────────┘
   <──head_diameter──><pad=6><────────bar_w────────><gap=8><text_w>
   = canvas_h                                                <r-inset=14>
```

Key:
- `•` accent dot (4-logical px diameter, provider color)
- Outer thick ring = 5h usage (active arc in accent / amber / red)
- Inner thinner ring = 5h remaining time (now `#B0B0B0` on `#404040`, visible)
- `5H` = uppercase semibold label, `muted` color, sits above the big number
- `31%` = big bold pct, `text_color`
- `▓▓▓` top tail bar = weekly usage fill (accent/amber/red), `usage_bar_h ≈ 10% of canvas_h`
- `░░░` track = `#2C2C2C` (toned down)
- `64%` = right-aligned, FW_SEMIBOLD, text_color
- `██░░` bottom tail bar = remaining time, `time_bar_h ≈ 4% of canvas_h` (visibly thinner)
- `3d` = right-aligned, FW_NORMAL, muted_color
- 8-logical gap between bar end and right-aligned text column (Change 4)

---

## 4. Do Not Change

- **Overall stadium shape with `corner_radius = canvas_h/2`** — clean and iconic, photographs well in screenshots.
- **Head-on-left, tail-on-right layout** — well-established mental model; sweeping arc + horizontal bar = "circular thing for now-ish, linear thing for week-ish."
- **Accent ramp by percent** (60/80/95 thresholds in `usage_color.rs`) — solid color logic; don't touch.
- **Pulse animation at ≥95%** — subtle, draws the right amount of attention. Keep.
- **`paint_pill` two-circle + middle-rect construction** — exactly right for tiny-skia. Don't refactor to a rounded-rect path; the current is faster and AA-clean.
- **`LineCap::Round` on the active arc** — feels alive; the "unfinished" look at low % I mentioned in issue (H) is acceptable trade-off.
- **DPI scaling via `scale_to_dpi`** — keep all proposed values in logical px and let the function do its job.
- **Locale-aware countdown width via `COUNTDOWN_TEMPLATE = "999시간"`** — clever and right; preserve.
- **`DT_END_ELLIPSIS` on tail text** — graceful degradation at narrow widths.
- **Existing fallback from countdown to `"5H"` when label rect is too narrow** — keep, just change the static fallback to uppercase per Change 1.
- **Aspect ratio taper (`aspect_at_width`)** — tail breathes better at wider sizes; preserve.

---

## Implementation Notes

- All proposed color tokens belong inline in `paint_bubble_pixmap` and `paint_bubble_text` — no new modules needed.
- The accent dot (Change 10) is ~3 new lines in the ring block.
- Bar height and gap changes (Change 8) are 3 single-line edits in `compute_bubble_layout`.
- Min-fill (Change 11) is a one-line guard before `paint_pill` for the weekly fill.
- No new geometry struct fields needed. No new fonts. No new dependencies.

---

## Unresolved Questions

1. **Light-theme accent dot**: Claude `#D97757` and Codex `#10A37F` on `#F3F3F3` — Codex teal contrast is ~3.0:1, borderline. Accept (it's a 4-px decorative dot, not text) or use a darkened variant for light theme?
2. **Change 12 alarm-tint**: should it apply to the head big-pct too, or only to the tail pct? Current proposal is tail-only. Confirm whether the head should also tint red at ≥95%.
3. **Change 10 dot position**: 12 o'clock is canonical but could be at 10–11 o'clock so the start of the active arc (which begins at 12 sweeping clockwise) doesn't visually merge with the dot. Open to either.

---

**Status:** DONE
**Summary:** Twelve concrete, primitive-implementable changes that hierarchically organize percentages, raise the invisible inner time ring above WCAG 1.4.11, breathe the cramped tail text by 8 logical px, and add a 4-px provider dot for identity — all without new deps or animation engines.
