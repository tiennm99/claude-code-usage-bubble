---
phase: 1
title: "Lock geometry"
status: pending
priority: P1
effort: "45m"
dependencies: []
---

# Phase 1: Lock geometry

## Overview
Define the tail layout contract in `compute_bubble_layout` so both rows consume the same horizontal geometry and differ only in vertical placement and bar height.

## Requirements
- Functional: percent text and countdown text appear after the bar, not inside it.
- Functional: `tail_usage_bar_rect.left/right == tail_time_bar_rect.left/right`.
- Functional: `tail_usage_pct_rect.left/right == tail_time_text_rect.left/right`.
- Non-functional: preserve the 140-360 logical size behavior and the minimum bar-width guard.
- Non-functional: no new renderer inputs from `src/app.rs`.

## Architecture
- Data flow: `src/app.rs:639-655` -> `bubble::update_data` -> `PaintInputs` -> `compute_bubble_layout` -> `paint_bubble_pixmap` / `paint_bubble_text`.
- Geometry source of truth: shared `text_w`, `text_left`, `bar_left`, and `bar_right` in `src/bubble.rs:1114-1126`.
- Current rect assignment already fans those shared values into both tail rows at `src/bubble.rs:1141-1163`.

## Related Code Files
- Modify: `src/bubble.rs`
- Read-only check: `src/app.rs`

## Implementation Steps
1. Re-verify the live mismatch before changing code; the current source already shares bar and text columns.
2. Make the target contract explicit in `compute_bubble_layout`: one shared bar lane, one shared text lane, row-specific `top/bottom` only.
3. Preserve `bar_min` fallback so long countdowns shrink text first, not bar width below usability.
4. Keep bar-height asymmetry unless the user confirms that equal thickness is also required.

## Todo List
- [ ] Confirm whether the bug is still reproducible on `main`.
- [ ] Document the target geometry near `compute_bubble_layout`.
- [ ] Ensure no later row-specific width override remains.

## Success Criteria

- [ ] Both tail bars have identical `left/right` bounds.
- [ ] Both tail texts start at the same `left` and end at the same `right`.
- [ ] No upstream data-contract change is required.

## Risk Assessment
- High: the source may already satisfy the request; unnecessary edits would add churn. Mitigation: prove the runtime mismatch first.
- Medium: long localized countdown strings can starve bar width at minimum size. Mitigation: keep `bar_min` and shared fallback math.
- Rollback: revert only the `compute_bubble_layout` diff.

## Security Considerations
- None beyond normal memory-safety review; change is layout-only.

## Next Steps
- Hand off the shared-geometry contract to Phase 2 for text painting and stale-comment cleanup.
