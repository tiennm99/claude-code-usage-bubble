---
phase: 2
title: "Apply renderer change"
status: pending
priority: P2
effort: "45m"
dependencies: [1]
---

# Phase 2: Apply renderer change

## Overview
Apply the tail text-placement change in the renderer so the weekly percent lane and weekly remaining-time lane follow the same `bar -> text` behavior, without changing provider or state plumbing.

## Requirements
- Functional: weekly percent text renders from `tail_usage_pct_rect`; weekly countdown renders from `tail_time_text_rect`.
- Functional: both texts stay right-aligned after the bar using `DT_RIGHT`.
- Functional: no tail text is drawn inside the bar fill.
- Non-functional: preserve the existing pulse behavior for `weekly_pct >= 95`.
- Non-functional: avoid touching `PaintInputs`, polling, or panel code unless a stale comment must be corrected.

## Architecture
- Bar drawing is tiny-skia-only in `src/bubble.rs:1298-1331`.
- Tail text drawing is a later GDI overlay in `src/bubble.rs:1643-1661`.
- `src/app.rs:636-638` currently describes the bubble percent as inline in the bar fill; if that wording is now false, correct it in the same phase.

## Related Code Files
- Modify: `src/bubble.rs`
- Optional modify: `src/app.rs`

## Implementation Steps
1. Align `paint_bubble_text` with the Phase 1 geometry contract and keep the percent/countdown text outside the bars.
2. Remove any remaining inline-percent assumption in comments or naming if it conflicts with the final behavior.
3. Keep the weekly percent highlight behavior and empty-countdown handling intact.
4. Stop scope creep: no changes to provider snapshots, update timers, or panel layout.

## Todo List
- [ ] Confirm `paint_bubble_text` is the only text-placement site for the tail rows.
- [ ] Update or remove stale inline-bar comments if they become misleading.
- [ ] Re-check placeholder and `None` states after the layout change.

## Success Criteria

- [ ] Weekly percent text appears after the top tail bar.
- [ ] Weekly countdown text appears after the bottom tail bar.
- [ ] Tail bar widths are driven only by shared geometry from `compute_bubble_layout`.

## Risk Assessment
- Medium: if the reported mismatch is only visual perception from unequal bar heights, text-placement edits alone will not fix it. Mitigation: compare runtime screenshots before and after Phase 1.
- Low: optional comment cleanup in `src/app.rs` can drift from renderer reality. Mitigation: change comments only after the final behavior is locked.
- Rollback: revert only the text-placement and comment diffs.

## Security Considerations
- None; no auth, network, or filesystem behavior changes.

## Next Steps
- Hand off to Phase 3 for compile checks and Windows visual verification.
