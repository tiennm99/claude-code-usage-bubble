---
phase: 3
title: "Validate on Windows"
status: pending
priority: P2
effort: "30m"
dependencies: [2]
---

# Phase 3: Validate on Windows

## Overview
Verify that the scoped renderer change compiles and that the native layered-window bubble actually presents equal-width tail bars with text after each bar across common runtime conditions.

## Requirements
- Functional: both tail rows visually render as `bar -> text`.
- Functional: both tail bars have the same visible width.
- Non-functional: confirm no regression to head ring, head text, or tray/panel refresh behavior.
- Non-functional: validation stays command-light and uses the existing Windows runtime.

## Architecture
- Compile-time validation covers the Rust renderer path end to end.
- Runtime validation must observe the real layered window because there are no snapshot/golden tests for `tiny-skia + GDI` composition in this repo.

## Related Code Files
- Verify: `src/bubble.rs`
- Verify if touched: `src/app.rs`

## Implementation Steps
1. Run the compile/test commands below.
2. Launch the app and verify the bubble at 140, default, and max logical sizes.
3. Check light and dark theme, Claude and Codex bubbles, and a long countdown string if available.
4. Capture before/after notes so a no-op or perception-only result is explicit.

## Validation Commands
```powershell
cargo check
cargo test
cargo run
```

## Todo List
- [ ] `cargo check` passes.
- [ ] `cargo test` passes, or any pre-existing failures are called out separately.
- [ ] Manual runtime check confirms equal bar widths and text-after-bar alignment.
- [ ] No regression is seen in the head ring/text or bubble refresh path.

## Success Criteria

- [ ] Compile succeeds on the current branch.
- [ ] The top and bottom tail bars share the same left/right edges at runtime.
- [ ] The percent and countdown texts both sit to the right of their bars at runtime.
- [ ] Any remaining mismatch is explained with evidence, not assumption.

## Risk Assessment
- High: native renderer issues are hard to prove without manual observation. Mitigation: test at minimum/default/maximum sizes and common DPI settings.
- Medium: reproducing the original complaint may require a specific locale, DPI, or stale binary. Mitigation: record the runtime conditions used during verification.
- Rollback: revert the renderer change if compile or visual regression appears.

## Security Considerations
- None.

## Next Steps
- If validation passes, implementation can be approved as a scoped `src/bubble.rs` change. If not, reopen Phase 1 with the observed runtime evidence.
