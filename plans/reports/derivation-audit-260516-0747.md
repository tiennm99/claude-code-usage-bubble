# Derivation audit: claude-code-usage-bubble vs Claude-Code-Usage-Monitor

**Date:** 2026-05-16
**Source repo:** `/config/workspace/CodeZeno/Claude-Code-Usage-Monitor` — MIT, "Copyright (c) 2025 Craig Constable" per `LICENSE` (note: NOTICE in new repo claims "2026 Code Zeno Pty Ltd"; the actual upstream LICENSE names a person)
**New repo:** `/config/workspace/tiennm99/claude-code-usage-bubble` — Apache-2.0 with NOTICE
**Method:** byte comparison (`cmp -s`), `diff -u`, longest-contiguous-identical-run analysis (Python), line-set intersection (`comm -12`).

---

## Per-file classification

| File | LOC | Category | Similarity | Notes |
|---|---|---|---|---|
| `src/poller.rs` | 1099 | **COPY** | 100% | `cmp -s` byte-identical. Zero changes. Biggest single risk surface. |
| `src/tray_icon.rs` | 441 | **COPY** | 100% | Byte-identical. |
| `src/theme.rs` | 51 | **COPY** | 100% | Byte-identical. |
| `src/models.rs` | 19 | **COPY** | 100% | Byte-identical. |
| `src/localization/mod.rs` | 246 | **COPY** | 100% | Byte-identical. |
| `src/localization/dutch.rs` | 46 | **COPY** | 100% | Byte-identical. |
| `src/localization/english.rs` | 46 | **COPY** | 100% | Byte-identical. |
| `src/localization/french.rs` | 46 | **COPY** | 100% | Byte-identical. |
| `src/localization/german.rs` | 46 | **COPY** | 100% | Byte-identical. |
| `src/localization/japanese.rs` | 46 | **COPY** | 100% | Byte-identical. |
| `src/localization/korean.rs` | 46 | **COPY** | 100% | Byte-identical. |
| `src/localization/spanish.rs` | 46 | **COPY** | 100% | Byte-identical. |
| `src/localization/traditional_chinese.rs` | 46 | **COPY** | 100% | Byte-identical. |
| `src/updater.rs` | 512 | **PATCH** | ~95% | 33-line diff. Changes: `RELEASE_ASSET_NAME`/`WINGET_PACKAGE_ID` renamed, `current_install_channel()` short-circuits to `Portable`, `ClaudeCodeUsageMonitor` -> `ClaudeCodeUsageBubble` in cache dir, three `#[allow(dead_code)]` attributes added. Algorithm and structure intact. |
| `src/diagnose.rs` | 52 | **PATCH** | ~98% | Single-line rename: log file `claude-code-usage-monitor.log` -> `claude-code-usage-bubble.log`. Otherwise identical. |
| `src/native_interop.rs` | 71 | **PATCH** | trimmed (108 LOC dropped) | Removed taskbar-embedding helpers (`find_taskbar`, `find_child_window`, `get_taskbar_rect`, `embed_in_taskbar`, WinEvent hooks). Kept `Color` struct, `colorref`, `wide_str`, `move_window`, `get_window_rect_safe`, timer/WM_APP constants — all verbatim names, signatures, bodies. Added two new timer/message consts (`TIMER_FULLSCREEN_CHECK`, `WM_APP_PANEL_TOGGLE`, `WM_APP_PANEL_CLOSE`). What remains is a verbatim subset of upstream. |
| `src/main.rs` | 49 | **PATCH** | ~50% | Same shape (parse `--diagnose`, init logging, dispatch to UI). Module list expanded; `window::run()` -> `app::run()`; an `if let Ok(path) = ...` refactor replacing `match`. Modules `app`, `bubble`, `panel`, `settings`, `diag`, `net`, `os` added; `window` removed. |
| `src/app.rs` | 1354 | **REWRITE** | ~25% line-set overlap with upstream `window.rs` | Carved out of upstream's 2847-LOC `window.rs`. Shares constants (`STARTUP_REGISTRY_PATH`, `STARTUP_VALUE_NAME`, `RETRY_BASE_MS`, `UPDATE_CHECK_INTERVAL_SECS`) and short adapted blocks (mutex setup, ~10 contiguous lines max). Structure genuinely reworked but lifts named consts, message-pump idioms, and update-scheduler logic. Fair-use derivative — *not* clean-room. |
| `src/bubble.rs` | 817 | **REWRITE** | ~23% line-set overlap | New floating-bubble window (popup + layered alpha, hit-testing). Borrows Win32 boilerplate (`SendMessageW(WM_SETICON, ...)`, `ExtractIconExW`, `GetDpiForWindow`); longest contiguous identical run is 16 lines of icon-attach plumbing. Core bubble rendering (per-pixel alpha, circle hit-test) is original. |
| `src/panel.rs` | 506 | **REWRITE** | ~24% line-set overlap | New popup panel for the expanded view. Longest identical run = 8 lines (generic GDI boilerplate). Original UI design (horizontal bars, session/weekly). |
| `src/settings.rs` | 140 | **REWRITE** | ~26% line-set overlap (mostly `serde` boilerplate) | New `Settings` struct with `BubblePositions`, persisted to JSON in `%LOCALAPPDATA%\ClaudeCodeUsageBubble`. Mostly original; longest identical run = 4 lines. |
| `src/diag/mod.rs` | 29 | **ORIGINAL** | 0% | New `log` + `simplelog` facade. No upstream counterpart. Parallels (not replaces) upstream's `diagnose.rs` which still exists in both. |
| `src/os/mod.rs` | 14 | **ORIGINAL** | 0% | New. |
| `src/os/string.rs` | 10 | **ORIGINAL** | 0% | New. |
| `src/os/color.rs` | 46 | **ORIGINAL** | 0% | New. |
| `src/os/dpi.rs` | 31 | **ORIGINAL** | 0% | New. |
| `src/os/registry.rs` | 159 | **ORIGINAL** | 0% | New typed registry wrapper. Parallel to upstream's inline `RegOpenKeyExW` calls. |
| `src/os/theme.rs` | 16 | **ORIGINAL** | 0% | New. Parallel to upstream's inline `theme.rs` logic. |
| `src/net/mod.rs` | 9 | **ORIGINAL** | 0% | New. |
| `src/net/winhttp.rs` | 431 | **ORIGINAL** | 0% | New WinHTTP-based blocking HTTP client. Replaces `ureq` once Phase 4/6 ships. |
| `res/icon.rc` | 32 | **ORIGINAL** | 0% | New. Upstream uses `winres` builder API in `build.rs` for the same effect. |
| `Cargo.toml` | 47 | **PATCH** | ~70% | Same dep list (`ureq`, `native-tls`, `serde`, `serde_json`, `dirs`, `windows`) plus 3 added (`log`, `simplelog`, `thiserror`). Pkg name/version/license/repo URLs changed; winres -> embed-resource; one `windows` feature swapped (`Win32_UI_Accessibility` dropped, `Win32_Networking_WinHttp` added). The winres `[package.metadata.winres]` block removed in favour of `res/icon.rc`. Profile-release block unchanged. |
| `build.rs` | 16 | **REWRITE** | different impl | Same purpose (compile icon into PE resources). Old uses `winres::WindowsResource` builder API + custom `pack_version` helper; new shells out to `embed-resource::compile("res/icon.rc")`. Structurally different but functionally equivalent. |
| `src/icons/*` (9 binary files) | n/a | **COPY** | 100% | All 9 icons (`16.svg`, `16x16.png`, `32.svg`, `32x32.png`, `48.svg`, `48x48.png`, `256.svg`, `256x256.png`, `icon.ico`) byte-identical to upstream. |
| `LICENSE` | n/a | swapped | n/a | Apache-2.0 (upstream is MIT). |
| `NOTICE` | n/a | new | n/a | Attribution to upstream + reproduces upstream MIT text. **BUG:** names "© 2026 Code Zeno Pty Ltd" but the actual upstream LICENSE says "Copyright (c) 2025 Craig Constable". The reproduced MIT block does not match the actual upstream MIT block verbatim — that's a defect in the attribution. |
| `README.md` | n/a | new | n/a | Documents the derivation: "This project is a derivative of CodeZeno/Claude-Code-Usage-Monitor (MIT, © 2026 Code Zeno Pty Ltd)" — same 2026/Code-Zeno error as NOTICE. |

---

## Totals (LOC weighted, code only; excludes icons/LICENSE/NOTICE/README)

| Category | LOC | % |
|---|---|---|
| **COPY** (byte-identical) | 2224 | 33.9% |
| **PATCH** (light edits) | 731 | 11.1% |
| **REWRITE** (same purpose, restructured, some idiom carryover) | 2833 | 43.2% |
| **ORIGINAL** (no upstream counterpart) | 777 | 11.8% |
| **TOTAL** | 6565 | 100% |

- **Derivation % (COPY + PATCH only):** **45.0%** — this is the legally-attribution-required portion under the strictest reading.
- **Derivation % including REWRITE:** **88.2%** — what you'd cite under a broad reading of "derivative work" (REWRITE files are split from upstream's `window.rs` with shared constants/idioms and would not exist in their current form without the upstream codebase).
- **Wholly original code:** **11.8%** (777 LOC) — `diag/`, `os/`, `net/`, `res/icon.rc`.

The user's expectation that "most files are no longer related" is **wrong**. Of 31 code files compared:
- 13 files (1932 LOC) are **byte-for-byte identical** to upstream.
- 4 files (684 LOC) are **trivial renames** of upstream.
- 4 files (2817 LOC) are **restructurings** that still share idioms and constants.
- 10 files (777 LOC) are **fully new**.

Specifically `poller.rs` (1099 LOC), `tray_icon.rs` (441 LOC), and all 9 localization files (614 LOC) are unchanged. That alone is 2154 LOC = ~33% of the codebase, byte-identical.

---

## Verdict

### 1. Is the NOTICE file still legally required?
**Yes — unambiguously.** The MIT license requires *"The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software."* 13 files (2224 LOC, ~34%) are byte-identical copies; another 4 files are light renames. These are "substantial portions" by any reasonable measure. Removing the NOTICE while keeping these files would breach the MIT terms.

### 2. Is Apache-2.0 compatible with upstream MIT for the copied portions?
**Yes.** MIT is compatible with relicensing the derivative under Apache-2.0, provided the original MIT copyright and permission notice are preserved (which is what NOTICE accomplishes). This is the standard "MIT → Apache-2.0" upgrade path. The new repo's outbound Apache-2.0 license governs the *combined* work; the MIT terms still bind the copied portions but are satisfied by NOTICE.

### 3. Attribution defects to fix
Two material issues in the current NOTICE/README:

a) **Wrong copyright holder name.** Upstream `LICENSE` says `Copyright (c) 2025 Craig Constable`. NOTICE and README both claim `Copyright (c) 2026 Code Zeno Pty Ltd`. The reproduced MIT block in NOTICE does not match the actual upstream MIT block. **Action:** correct NOTICE to reproduce the upstream MIT verbatim (`(c) 2025 Craig Constable`), or at minimum cite both names if Code Zeno Pty Ltd is an additional rights holder somewhere else (e.g. winres metadata in upstream `Cargo.toml` does name `Code Zeno Pty Ltd` — but that is metadata, not the LICENSE copyright line).

b) **NOTICE understates the scope of copying.** Current NOTICE lists 7 modules as "ported with minor adaptations" and claims `app.rs`, `bubble.rs`, `panel.rs`, `settings.rs` are "original". That's defensible for `bubble.rs`/`panel.rs`/`settings.rs` but **`app.rs` is a carve-out of `window.rs`**, not original. It reuses upstream constants, the message-pump dispatch shape, and the update-check scheduler. Strictly truthful wording: *"`app.rs` was derived by splitting upstream's `window.rs` and is partly original / partly adapted"*.

### 4. Recommendation if the user wants to drop NOTICE
**Cannot drop NOTICE today.** The minimum work to be able to drop attribution:

1. **Phase 4** of the existing plan retires `poller.rs` (1099 LOC, 17% of total) — biggest single win.
2. **Phase 5** retires `tray_icon.rs` (441 LOC).
3. **Phase 2** retires `models.rs` (19) and `localization/*` (614 LOC).
4. **Phase 6** retires `updater.rs` (512), `diagnose.rs` (52), `theme.rs` (51), `native_interop.rs` (71).

After all six phases execute as planned, *every* COPY and PATCH file is removed. The only remaining derivative content would be the REWRITE files (`app.rs`, `bubble.rs`, `panel.rs`, `settings.rs`), and at that point the line-set overlap is mostly Win32 idioms / common consts (mutex setup, registry path) which are not copyrightable on their own. **NOTICE can be dropped only after Phase 6 completes** — not before. The existing plan file `phase-06-updater-and-remove-notice.md` correctly sequences NOTICE removal as the *last* step.

The plan's framing as a "clean-room rewrite" is technically inaccurate today: the current state is ~45% direct derivation, not clean-room. After Phase 6 it becomes a clean-room rewrite. Until then, NOTICE is load-bearing.

### 5. Risk summary (blunt)
- The user's mental model "most files are no longer related" is **false**. Roughly **half of the codebase by LOC is direct copy or trivial rename** of upstream. The other half is either substantial rework (genuinely derivative) or genuinely new.
- The biggest single derivation surface is `poller.rs` — 1099 LOC, byte-identical, 17% of the entire codebase by itself.
- The NOTICE has a copyright-holder-name defect that should be corrected regardless of any future plans (this is technically a license breach today — the MIT requires the *actual* copyright notice to be preserved, not a paraphrased one).
- If the project ships before Phase 4-6 complete, the NOTICE must stay, must be correct, and must continue to ship in every release artifact (binary releases, source tarballs).

---

## Unresolved questions

1. Why does the upstream `Cargo.toml` `[package.metadata.winres]` block list `Code Zeno Pty Ltd` as `CompanyName`/`LegalCopyright` while the upstream `LICENSE` names `Craig Constable`? Two possibilities: (a) `Craig Constable` is the author who later assigned to or works for Code Zeno Pty Ltd, or (b) the upstream LICENSE file was never updated when Code Zeno took over. Either way, the new repo's NOTICE should reproduce the upstream MIT text **verbatim** — not paraphrase it with a different copyright holder.
2. Phase 1 of the existing plan introduced `os/`, `net/`, `diag/`, and `res/icon.rc` but did *not* delete any legacy code — they sit alongside the unchanged copies. Confirmed by reading `main.rs` (both module sets are mounted). The "clean-room rewrite" name is therefore aspirational for the project, not descriptive of current state.
3. Should `res/icon.rc`'s `CompanyName "tiennm99"` instead read something acknowledging the upstream icon copyright? The `.ico` is byte-identical to upstream's. The PE VERSIONINFO block is the public-facing metadata users see in Properties dialogs.

**Status:** DONE
**Summary:** Forensic audit complete. ~34% of the new repo is byte-identical copy of upstream; another ~11% is trivially renamed; ~43% is structural derivative; only ~12% is fully original. NOTICE is legally required and has a copyright-name defect; can only be safely removed after the plan's Phase 6 retires the last COPY/PATCH files. Report written to `/config/workspace/tiennm99/claude-code-usage-bubble/plans/reports/derivation-audit-260516-0747.md`.
