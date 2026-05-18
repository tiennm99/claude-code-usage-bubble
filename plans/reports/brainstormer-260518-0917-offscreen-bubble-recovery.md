# Brainstorm: Off-Screen Bubble Recovery

## 1. Recovery strategies ranked

### RECOMMEND — Layered: validate on load + clamp on create

**A. Validate position in `settings::load`** (primary defense)
- After deserialize, walk `bubble_positions`. For each `Some((x,y))`, build a probe rect `(x, y, x+min_w, y+min_h)` and check via `MonitorFromRect(... MONITOR_DEFAULTTONULL)`. If null → set to `None`.
- One pass, ~15 lines, runs before any window code sees the value.
- Pro: KISS, no race with `ShowWindow`, fixes related bugs (hand-edited JSON, dpi-changed coords).

**B. Clamp on create** (defense-in-depth, kept as proposed)
- After `CreateWindowExW`, before `ShowWindow`, call `clamp_into_work_area(hwnd)`.
- Catches monitor unplug **between** `load()` and `create()` (rare but possible: laptop closed mid-startup).
- Cost: one extra call, idempotent.

Both together are the right answer. Neither alone covers all cases.

### CONSIDER — Visual cue

**C. Tray balloon "Widget repositioned to primary monitor"**
- Only when validator actually relocated. Uses existing `Shell_NotifyIconW NIF_INFO`. ~20 lines.
- Risk: balloon spam on dock/undock cycles. Fire only when *saved* position was killed.

### AVOID

**D. Topology fingerprint** — overkill, doesn't preserve intent better than (A).
**E. Per-monitor relative pinning** — future feature, not a fix. YAGNI.

## 2. Trade-off matrix

| Approach | Preserves intent on replug | Surprise on cold start | LOC | Risk |
|----------|---------------------------|------------------------|-----|------|
| A (validate-on-load) | No — wipes saved coord | Low | ~15 | None |
| B (clamp-on-create) | Partial — moves to nearest edge | Low | ~3 | None |
| A+B | No | Low | ~18 | None |
| A+B+C | Same + explains itself | Very low | ~38 | Balloon fatigue |
| D (topology hash) | Yes if same monitor before next launch | Medium | ~80 | Maintenance |
| E (relative pin) | Yes | Medium | ~150 | Premature |

## 3. Edge cases proposed fix misses

1. **Saved-pos monitor asleep / no input** — `MonitorFromRect` still returns handle; A+B no-op. Correct.
2. **DPI change while app closed** — px coords technically valid by topology; A passes, B no-op. Acceptable.
3. **Negative-coord monitors (secondary left of primary)** — validator MUST use `MONITOR_DEFAULTTONULL`, not `DEFAULTTONEAREST` (would silently snap valid secondary coord to primary).
4. **Dual bubbles overlap after relocate** — both clamped to bottom-right of primary → stacked. `default_position` staggers Codex; clamp doesn't. Minor.
5. **User drags to secondary, unplugs, restarts** — A+B: bubble at primary default; saved pos destroyed. No recovery on replug. Acceptable for v1.
6. **Dock-daily multi-monitor user** — every undock wipes pos; every dock back gives default. Annoying. Case where E would win. Punt unless reported.

## 4. Logging strategy (minimal)

On the visibility-affecting path only:

- `info`: `bubble create model={} pos=({},{}) size={}x{} dpi={}` — one line per bubble at create.
- `warn`: `bubble position ({},{}) outside all monitors, resetting to default` — fires in validator. **This is the line that would have solved this bug in 5 seconds.**
- `warn`: `clamp_into_work_area moved bubble from ({},{}) to ({},{})` — fires on create-time clamp.
- `debug`: monitor enumeration on startup.

Skip: per-render logs, drag logs, timer ticks.

## 5. "Reset position" discoverability — secondary

Menu item exists but buried. If A+B work, this path is unreachable. Don't add a "Reset position" balloon prompt — confirmation fatigue. Just fix silently and the §4 warn + §C balloon explain it once.

## Recommended action

1. Add `BubblePositions::validate(&mut self)` called from `settings::load`. Use `MonitorFromRect(... MONITOR_DEFAULTTONULL)` with `(x, y, x+MIN_BUBBLE_SIZE, y+MIN_BUBBLE_SIZE)`. Set to `None` on miss. Log `warn`.
2. Call `clamp_into_work_area(hwnd)` in `bubble::create` between `CreateWindowExW` and `ShowWindow`. Log `warn` on movement.
3. Add the 4 log lines from §4.
4. Single tray balloon "Widget repositioned: previous monitor not connected" once per launch when validator killed any saved position.
5. Defer monitor-index pinning (E) and topology hash (D).

Total: ~40 lines, one new function, two log statements, one balloon call.

## Unresolved questions

- Balloon (item 4): opt-in or always-on? Default always-on.
- `MIN_BUBBLE_SIZE` as probe rect, or account for current `bubble_size_logical`? Min safer.
- Re-attempt last-known coord on replug? Probably no — YAGNI.
- Codex/Claude stagger preservation on auto-relocate? Currently they'd stack. Worth fixing in same patch?
