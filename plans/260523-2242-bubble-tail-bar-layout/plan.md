---
title: "Bubble tail bar layout alignment"
description: "Scoped renderer-only plan to align weekly percent and remaining-time tail bar geometry."
status: pending
priority: P2
effort: 2h
branch: "main"
tags: [rust, renderer, bubble, layout]
blockedBy: []
blocks: []
created: 2026-05-23
createdBy: "ck:plan"
source: skill
---

# Bubble tail bar layout alignment

## Scope
- User-facing goal: weekly percent lane and weekly remaining-time lane both render as `bar -> text`, and both bars share identical left/right bounds.
- Expected code scope: `src/bubble.rs`; touch `src/app.rs` only if comment cleanup is needed.
- Backwards compatibility: no settings, storage, IPC, or provider-data changes.

## Verified Codebase Facts
- Bubble data already provides `weekly_pct`, `weekly_text`, and `weekly_resets_at` through `bubble::update_data`; no new inputs are needed (`src/app.rs:639-655`).
- `compute_bubble_layout` already derives one shared text column and one shared bar lane for the two tail rows (`src/bubble.rs:1114-1163`).
- `paint_bubble_text` already renders weekly percent and weekly countdown as separate right-aligned texts (`src/bubble.rs:1644-1661`).
- `paint_bubble_pixmap` paints both tail bars from rects only; text is a GDI overlay, so geometry must stay the single source of truth (`src/bubble.rs:1298-1331`).

## Phases

| Phase | Name | Status |
|-------|------|--------|
| 1 | [Lock geometry](./phase-01-lock-geometry.md) | Pending |
| 2 | [Apply renderer change](./phase-02-apply-renderer-change.md) | Pending |
| 3 | [Validate on Windows](./phase-03-validate-on-windows.md) | Pending |

## Dependencies

- Sequence: Phase 1 -> Phase 2 -> Phase 3.
- File ownership: `src/bubble.rs` stays single-owner across phases; optional `src/app.rs` comment cleanup happens only in Phase 2.
- Related existing plan: `plans/260523-ui-ux-improvement-plan/plan.md` Phase 3 overlaps in theme but does not block this scoped renderer-only change.

## Rollback
- Revert layout math and text-placement changes in `src/bubble.rs`.
- Revert optional comment cleanup in `src/app.rs`.
- No data migration or persisted-state rollback is needed.

## Unresolved Questions
- Does "same length" mean equal width only, or should the two tail bars also share the same height? Current code intentionally uses different heights (`src/bubble.rs:1105-1110`).
- The current source already looks close to the requested behavior. If runtime still differs, is the issue in this branch, a stale binary, or perception caused by different bar heights?
