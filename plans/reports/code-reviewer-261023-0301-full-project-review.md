# Full-project review

Eleven review dimensions were checked by separate reviewers. Each finding was then checked by three independent verifiers, and kept only when two agreed. Of 40 distinct findings, 37 were confirmed and merged into the 28 items below. Items marked *needs decision* change user-visible behaviour or a product choice, so they were not implemented without the owner's answer.

## Summary

I merged 35 confirmed findings into 28 items. The two panel-placement findings (geom.rs:366 and panel.rs:286) share one cause, the virtual-screen union, so they are now one item. Eight dead-code findings all trace back to lib.rs making every module `pub`, so they are also one item; the Status/Utf8 variants are part of it. Verifier votes changed several severities. The licence item drops from high to medium. These drop from medium to low: Ctrl+wheel, the 429 fallback, poll-in-flight, refresh-path tests and the update-check retry.

The most important items:
- **Licence breach.** Upstream MIT icons ship byte-identical, with NOTICE deleted, while the README claims a clean-room implementation.
- **Signing-key exposure.** The updater signing key is visible to unpinned linuxdeploy downloads during the AppImage bundle step.
- **Untested releases.** A release tag publishes to every client without running tests.
- **Panel off-screen.** In the common laptop plus taller external monitor layout, the panel lands off-screen at the default bubble position.
- **Codex refresh broken.** `codex exec .` fails the git-repo check, reproduced with codex-cli 0.163.0, so Codex token refresh never works when the app is launched normally.
- **Refresh spends quota.** Credential refresh runs a real CLI prompt on every failed poll, with no backoff, and also does so on 403 responses.

Five items need a user decision:
- The licence fix: replace the icons, or restore NOTICE.
- The rounding fix, which changes the displayed number.
- Sending 403 responses to the Messages fallback instead of refreshing.
- The 429 cool-down, because it also changes how often the paid fallback probe runs.
- Removing unused but possibly planned code.

I modified no files.

## Items

### 1. Upstream MIT icons ship without the upstream notice, and the README claims a clean-room implementation *needs decision*

`README.md:23` · severity medium · docs · effort small

**Why.** src/icons/* are byte-identical to CodeZeno upstream (checked by git blob SHA). They have not changed since c0f3e3f. icons/icon.ico is identical to src/icons/icon.ico and is bundled through tauri.conf.json. runtime.rs:1010 embeds icons/128x128.png. Commit aa6217d deleted NOTICE, and the project's derivation audit says NOTICE must stay while COPY files remain. As a result, every exe, AppImage and deb breaches MIT attribution, and the README misdescribes where the artwork comes from.

**Fix.** Choose one. (a) Replace src/icons/* with original artwork, regenerate icons/ with `npx tauri icon`, and check that no blob SHA matches upstream. (b) Restore a root NOTICE that reproduces upstream's MIT LICENSE verbatim, including "Copyright (c) 2025 Craig Constable" (not the earlier wrong holder name). It should state that it covers src/icons/* and the icons/* derived from them, and it should be shipped with releases. Then reword README Acknowledgements so the code is described as clean-room and the icons as coming from upstream under MIT.

### 2. Updater signing key is visible to unpinned linuxdeploy tooling during the AppImage bundle step

`.github/workflows/release.yml:176` · severity medium · security · effort small

**Why.** The Linux bundle step passes TAURI_SIGNING_PRIVATE_KEY and its password into the environment of `tauri bundle --bundles appimage,deb`. To build the AppImage, that step downloads and runs linuxdeploy, the appimage plugin from the `continuous` release and the gtk plugin from a `master` raw URL, none of them hash-pinned. The workflow's own rule (release.yml lines 80-82 and 140, docs/release-process.md) says only signing steps may see the key, and the Windows job already follows it. A compromised upstream artifact could steal the key and sign updates that every client would install, and the key cannot be rotated without stranding every existing install.

**Fix.** 1. In the bundle step, remove both signing env vars and the `--config createUpdaterArtifacts:true` override, so it runs `npm run tauri bundle -- --bundles appimage,deb`.
2. In the rename step, drop the `cp`/`test -s` lines for `$appimage.sig`.
3. After the rename step, add a step that alone receives the key and password and runs `npx tauri signer sign --app-version "$VERSION" "dist/usage-monitor_${VERSION}_amd64.AppImage"` followed by `test -s` on the .sig, the same way release.yml lines 116-125 do it for Windows.
4. Update the AppImage .sig row in docs/release-process.md (line 73) and the workflow comment at lines 172-173.

### 3. Release workflow publishes a tag as latest without running tests, clippy or lint

`.github/workflows/release.yml:24` · severity medium · test-gap · effort small

**Why.** The prepare, win-x64, linux-x64 and publish jobs only check the tag, version and key, then build, sign and run `gh release create --latest`. ci.yml never runs on tags, and step 4 of docs/release-process.md pushes main and the tag back to back. A commit that compiles but fails a test or lint gets published and auto-updates to every client before CI reports red. A tag on a commit outside main gets no CI run at all.

**Fix.** 1. Add `workflow_call:` (with an optional `ref` input) to ci.yml, and check out `${{ inputs.ref || github.sha }}`.
2. In release.yml, add a `verify` job: `needs: prepare`, `uses: ./.github/workflows/ci.yml`, `with: { ref: ${{ needs.prepare.outputs.tag }} }`.
3. Set `needs: [prepare, verify]` on win-x64 and linux-x64.
4. Confirm that ci.yml's cancel-in-progress concurrency group cannot cancel the called run.
5. Change docs/release-process.md step 4 to: push main, wait for green CI, then push the tag.

### 4. Panel is placed within the union of all monitors' full rects, so it lands off-screen or over the taskbar on mixed-height layouts

`src/panel.rs:286` · severity medium · bug · effort small

**Why.** place_near_bubble passes virtual_screen(monitor_rects) as the bounds. monitor_rects uses each monitor's full rect, not its work area, and virtual_screen takes their bounding box. geom::place_near (geom.rs:366) only flips the panel above the bubble at bounds.b. Take a 1920x1080 primary (work-area bottom 1040) next to a top-aligned 2560x1440 secondary, with the bubble at its default spot. The panel then goes to y=1024..1144: it covers the taskbar, and rows 1080-1144 at x<1920 are on no monitor. This is the default position in a very common laptop plus external monitor layout. Even on a single monitor, the flip compares against the monitor bottom rather than the work-area bottom.

**Fix.** In place_near_bubble, use the work area of the bubble's own monitor as the bounds. Reuse bubble.rs's `screen_of(&bubble).work` (make it pub(crate) or move it into runtime.rs); its fallback chain is current_monitor then primary_monitor. Fall back to virtual_screen, and then to the anchor, only when no monitor is available. Leave place_near unchanged. Add geom tests for: a bubble near the bottom of the shorter monitor in a mixed-height layout (the panel must flip above), and a single monitor whose work area is shorter than its rect.

### 5. `codex exec .` refresh fails the Codex git-repo or trusted-directory check, so Codex token refresh never works

`src/usage/refresh.rs:85` · severity medium · bug · effort small

**Why.** This was reproduced with codex-cli 0.163.0. spawn_first never sets current_dir, so the child inherits the GUI app's working directory: $HOME, `/` or System32. In those directories `codex exec .` prints "Not inside a trusted directory and --skip-git-repo-check was not specified" and exits before it makes any request. auth.json is never rotated, so every Codex token expiry ends in Timeout and a session-expired alert. If the app was instead launched from inside a project, `codex exec .` runs a real agent turn in that project, and `claude -p .` loads that project's CLAUDE.md and hooks.

**Fix.** 1. Change the Codex args in both cli_for variants (Unix and Windows) to `["exec", "--skip-git-repo-check", "--sandbox", "read-only", "."]`.
2. In spawn_first, set `cmd.current_dir(std::env::temp_dir())` (or CODEX_HOME/~/.codex) for both CLIs.
3. Add a unit test asserting that the Codex args include `--skip-git-repo-check`.

### 6. Credential refresh runs a real CLI prompt on every failed poll, with no backoff, and treats every 403 as refreshable *needs decision*

`src/usage/refresh.rs:47` · severity medium · robustness · effort medium

**Why.** state.rs:143 records every AuthFailed result, and runtime.rs:320 then always calls refresh_credentials. Orchestrator::refresh always spawns `claude -p .` or `codex exec .`, each of which sends a real model request. Nothing records an earlier failed refresh. anthropic.rs:75-82 maps every 403 to AuthRequired and returns early with `?`, so the Messages fallback is never tried. With a persistent 403 that a refresh cannot fix (scope or org policy), or a CLI that cannot refresh, the app spawns another prompt every poll, as often as every 60 s, and uses the very quota it monitors. A timed-out child is only reaped, never killed.

**Fix.** 1. Keep per-provider refresh state in the runtime: the time of the last failed attempt and the credential signature at that attempt. Skip the CLI spawn while a doubling backoff runs (for example 5 min, capped at 1-6 h), and also skip while the signature is unchanged since the last failed refresh. Reset the state when a poll succeeds or the signature changes.
2. Trigger the CLI refresh only for a 401 or a token past expiresAt. For a 403 from /api/oauth/usage, return Ok(None) so the Messages fallback runs, or return a failure that does not trigger a refresh.
3. Killing the child after the timeout is optional. A slow Windows start can finish its refresh after 8 s, so use a longer grace period if you kill it.

### 7. Privacy section does not say the app runs `claude -p .` / `codex exec .` on auth failure

`README.md:232` · severity medium · docs · effort small

**Why.** The Privacy and security section claims to list what the app reads, sends and does. It leaves out the hidden, detached CLI runs (runtime.rs:320 calling refresh.rs:71-86). Those runs send a small prompt under the user's account, which counts against usage, and Codex runs an agent turn in the app's working directory. README:57 also says Claude Code '(CLI or App)' is enough, but the refresh needs the `claude` CLI on PATH, in ~/.local/bin or in ~/.claude/local.

**Fix.** 1. Add a 'What the app runs' bullet: when a provider rejects the stored credentials, the app runs `claude -p .` and/or `codex exec .` hidden in the background so the CLI can refresh its token. Say that each run sends one tiny prompt counted toward usage, and state the working directory the run uses once the rank-5 fix lands.
2. Do not claim this happens on every poll if the rank-6 backoff lands.
3. Change README:57 to say automatic refresh needs the `claude` CLI.

### 8. Bubble resize around its centre is never clamped back into the work area

`src/bubble.rs:667` · severity low · bug · effort small

**Why.** resize_all is reached from Ctrl+wheel and the Size menu. It calls set_position(cx - w/2, cy - h/2) with no clamp_into_work_area, and nothing re-clamps afterwards: reclamp is Windows-only and runs only on display-change messages. A bubble at its default bottom-right spot grown to 360 ends 56 px past the right edge and 12 px under the taskbar, and it stays there until the user drags it. The saved top-left also goes stale, so the bubble jumps to a different spot on the next launch.

**Fix.** 1. In resize_all, keep the provider from `windows(app)` and compute `pos = geom::clamp_into_work_area(Rect::from_xywh(cx-w/2, cy-h/2, w, h), screen_of(&window).work, provider)`. Use the pre-resize screen, then call set_position(pos).
2. Return the (provider, pos) pairs, and have Runtime::set_bubble_size call save_bubble_position for each provider that already has a saved entry. This keeps restarts consistent without pinning bubbles that are still at the default.

### 9. Linux live theme switching never fires: tao emits ThemeChanged with a dummy window id, which Tauri drops

`src/bubble.rs:532` · severity low · bug · effort medium

**Why.** In tao 0.37.1, the portal listener sends SetTheme to WindowId::dummy() (portal.rs:54), and the event loop emits ThemeChanged on that dummy id (event_loop.rs:959-973). tauri-runtime-wry 2.12.2 (lib.rs:4211) forwards a window event only when window_id_map knows the id, so this event is dropped. The Linux ThemeChanged arm in bubble.rs never runs, and the theme is read only at create time. When the GNOME or KDE colour scheme changes, existing bubbles keep the old theme until the app restarts, even though live re-theme is an advertised feature.

**Fix.** 1. Start one Linux-only background thread from setup. It opens `dbus::blocking::Connection::new_session()` and adds a match on `org.freedesktop.portal.Settings.SettingChanged`.
2. In the handler, filter for namespace `org.freedesktop.appearance` and key `color-scheme`, unwrap the nested Variant to a u32, and map it with the existing `color_scheme_is_dark`.
3. On Some, hop to the main thread with `app.run_on_main_thread` or the existing `defer`, and call `runtime.set_dark`.
4. If the session bus is unavailable, log once and exit the thread.
5. Delete the dead arm, or replace it with a comment explaining why tao's event never arrives.

### 10. A failed update check is recorded as completed and pushes the next attempt out by a full interval

`src/runtime.rs:622` · severity low · robustness · effort small

**Why.** run_update_check sets last_update_check_unix=now, saves it, and sends UpdateCheckDone{interval} for every outcome, Err included. The scheduler (scheduler.rs:110) re-arms the full interval, and the next startup treats the failed check as recent. With autostart before the network is up, one transient failure delays the next update check by a day or a week, so a release can sit unnoticed for that long.

**Fix.** 1. In run_update_check, stamp and save last_update_check_unix only on Ok(UpToDate | Available).
2. On Err, send UpdateCheckDone with a `succeeded: false` flag (or a new UpdateCheckFailed message). The scheduler then re-arms at `min(interval, 60s * 2^attempt)` and resets `attempt` after a success.
3. Apply the same handling to the thread-spawn failure branch (runtime.rs:584-588).
4. Add a Deadlines unit test.

### 11. Weekly "100%" truncates to "10…" at larger bubble sizes because the percent column is measured at weight 400 but drawn at 600

`ui/layout.js:189` · severity low · bug · effort small

**Why.** pctReserveW = ceil(measure('100%', fonts.small)) + 2. bubble.js:215 measures with MEASURE_WEIGHT 400, but `.tail-pct` is drawn at font-weight 600 with ellipsis overflow. Measured in Chromium with DejaVu Sans, once fonts.small reaches about 13 px the semibold '100%' is wider than the reserved width (for example 46.3 px against 45 px at 15 px). The number is hidden in exactly the state the pulse is meant to highlight.

**Fix.** 1. Pass the weight through the callback: `tailLayout(canvas, (text, px, weight) => measure(text, px, weight))`.
2. In layout.js:189, use `measure(PCT_TEMPLATE, fonts.small, TAIL_PCT_WEIGHT)`, with an exported `TAIL_PCT_WEIGHT = 600` that matches the CSS.
3. Update the MeasureText JSDoc.
4. Add a layout.test.js case whose stub returns a wider width at weight 600, and assert that usagePctRect covers it.

### 12. Enabling a provider while a poll is in flight leaves its bubble empty until the next scheduled poll

`src/runtime.rs:457` · severity low · bug · effort small

**Why.** toggle_provider calls spawn_poll, and spawn_poll drops the request when poll_in_flight is set (lines 280-288). The running poll copied the settings at its start, so it never polls the newly enabled provider. That bubble shows the placeholder for up to a full interval. The window in which this can happen widens by up to 8 s per rejected provider while refresh_credentials runs. Manual Refresh is a workaround. The reset_positions call site is harmless.

**Fix.** 1. Add `poll_requested: AtomicBool`. When spawn_poll's compare-exchange fails, set it to true.
2. In the worker, loop: `loop { poll_once(); propagate(); if !poll_requested.swap(false) { break } }`.
3. Clear poll_in_flight, then re-check poll_requested and call spawn_poll again if it is set. This closes the gap between the last swap and the clear.

### 13. 429/5xx from the usage endpoint ignores Retry-After and re-hits the endpoint plus a Messages probe on every poll *needs decision*

`src/usage/anthropic.rs:83` · severity low · robustness · effort medium

**Why.** Every non-2xx status other than 401/403, and every transport error, becomes Ok(None), which leads to try_messages_endpoint, a real max_tokens=1 inference POST. Nothing reads Retry-After, and the scheduler has no error backoff. While the token is rate-limited, every poll sends the GET to an endpoint that is already refusing it, plus a small quota-consuming probe. On a 429 the fallback is still useful, because it keeps the numbers showing; 5xx and transport failures turn into extra inference calls instead of a short-lived error.

**Fix.** 1. Keep the Messages fallback for 429.
2. On a 429, read Retry-After (or use a capped exponential delay) and store a per-provider 'usage endpoint blocked until' instant. While it is active, skip the GET and go straight to Messages. Optionally rate-limit the probe and show the last good values marked stale.
3. Map 5xx and transport errors to Err(Network/BadResponse) without probing Messages.
4. Keep the fallback for shape failures: an unparsable body, or missing reset times.

### 14. Threshold-crossing check ignores window resets, so a new session that passes 80/95 within one poll never alerts

`src/core/state.rs:131` · severity low · bug · effort small

**Why.** old_pct is the previous snapshot's utilization, and resets_at is never consulted. Suppose poll N shows 97%, the 5 h window resets, and poll N+1 shows 85% or 96% of the new window. Nothing crosses, so no alert fires for the new session. This can happen with the 1-hour poll interval, or after suspend or failed polls, because a Failed poll keeps the old snapshot.

**Fix.** 1. Treat the stored window as a different window when its resets_at has already passed (pass a SystemTime wall_now into apply_results next to the Instant), or when the new resets_at exceeds the old one by more than 60 s.
2. In that case, use old_pct = 0.0 before checking crossings.
3. Add tests: 97 then a reset then 85 alerts at 80; 96 alerts at both 80 and 95; a few seconds of resets_at jitter inside the same window does not re-alert.

### 15. Scheduler deadlines use the monotonic clock, which does not advance during suspend

`src/scheduler.rs:166` · severity low · robustness · effort medium

**Why.** poll_at, countdown_at and update_at are all Instants, and recv_timeout waits on CLOCK_MONOTONIC, which does not count suspended time on Linux. Nothing hooks resume. After resume, the bubbles keep showing pre-suspend data until the rest of the poll interval passes in awake time. A daily update check on a laptop awake 8 h/day runs about every 3 calendar days.

**Fix.** 1. Keep update_at as a unix-timestamp target, and recompute the remaining time from SystemTime on every loop pass.
2. Cap recv_timeout at about 60 s.
3. After each wake, compare the SystemTime elapsed with the Instant elapsed. If wall time jumped ahead by more than about 30 s, treat it as a resume: run Action::Poll, re-arm poll_at, and re-check the update target.
4. Pass now_unix into take_due/next_wakeup so the logic stays unit-testable.

### 16. One wrongly typed field in settings.json resets all settings, and the next save overwrites the file

`src/core/settings.rs:250` · severity low · robustness · effort small

**Why.** parse_and_normalise deserialises the whole struct at once. A float in an i32/u32 field (for example a hand edit of 250.5) fails the entire parse, so every field falls back to its default. Any later save (a drag, a toggle, or the automatic update-check timestamp at runtime.rs:622) then overwrites the user's file. One verifier judged this out of scope because the defaults are documented for corrupt files. The loss is silent and needs no user action, so it is kept as cheap hardening.

**Fix.** 1. Keep the strict parse as the fast path.
2. On failure, parse into serde_json::Value. If it is an object, start from Settings::default() and fill each known key with serde_json::from_value, skipping and logging keys that fail, then apply the existing normalisation.
3. If the file is not valid JSON at all, copy it to settings.json.bak before the first save.
4. Add a test: `{"bubble_size_logical":250.5,"show_codex":true,"poll_interval_ms":60000}` keeps show_codex and poll_interval_ms.

### 17. Taskbar snap uses the primary monitor's taskbar for bubbles on every monitor

`src/platform/windows.rs:395` · severity low · bug · effort small

**Why.** ABM_GETTASKBARPOS reports only the primary Shell_TrayWnd. bubble::move_end passes that rect to geom::snap together with the current monitor's work area. snap_alongside_taskbar (geom.rs:318) compares only one coordinate and never checks that the taskbar touches the bubble's work area. A bubble dropped on a secondary monitor within 12 px of the primary taskbar's line is pulled to it and saved there, for example resting 44 px above the bottom of a monitor that has no taskbar.

**Fix.** 1. Make the pure snap_alongside_taskbar guard itself: snap only when the taskbar's inner face equals the work-area edge on that side and the taskbar overlaps the work area on the other axis. Bottom: `tb.t==wa.b && tb.l<wa.r && tb.r>wa.l`. Top: `tb.b==wa.t` plus x-overlap. Left: `tb.r==wa.l` plus y-overlap. Right: `tb.l==wa.r` plus y-overlap. Otherwise return early.
2. Add a geom test with a secondary-monitor work area and a primary-monitor taskbar, asserting no snap.

### 18. Linux autostart desktop entry writes an unquoted Exec path, which breaks for AppImage paths containing spaces

`src/runtime.rs:873` · severity low · bug · effort small

**Why.** For an AppImage, tauri-plugin-autostart 2.7.0 passes the raw $APPIMAGE path, and auto-launch 0.6.0 writes `Exec={path} {args}` without quoting. A path such as `~/My Apps/usage-monitor.AppImage` makes the session run `/home/u/My`, which fails silently. Meanwhile is_enabled only checks that the file exists, so the menu shows start-at-login as checked. The README tells users to put the AppImage in a writable folder of their choice, so such paths are plausible. Windows already works around the same problem.

**Fix.** 1. On Linux, bypass the plugin the same way the Windows path does. Build the XDG entry with auto_launch directly, or write ~/.config/autostart/UsageMonitor.desktop yourself, keeping the same name.
2. Write Exec as a double-quoted path with `"`, `` ` ``, `$` and `\` backslash-escaped, and `%` written as `%%`. Then apply the desktop-file string escaping of `\`.
3. Put the escaping in a pure function, and unit-test it with paths containing a space and a `$`.

### 19. --diagnose log is written to a fixed /tmp path with default 0644 permissions

`src/diag/mod.rs:27` · severity low · security · effort small

**Why.** File::create(temp_dir().join("usage-monitor.log")) on Linux opens /tmp/usage-monitor.log with O_TRUNC and no O_EXCL, and creates it 0644. Other local users can read the request URIs, usage figures and refresh outcomes. Tokens are redacted. This only happens with the opt-in flag, and protected_regular blocks the pre-created-file attack on mainstream distros. The README and module comment describe a %TEMP% location, so the Linux path is probably unintended.

**Fix.** 1. On Unix, write to dirs::state_dir() or dirs::cache_dir() joined with `usage-monitor/` (created 0700), falling back to temp_dir.
2. Open with OpenOptions write+create+truncate and `.mode(0o600)`, plus set_permissions(0o600) for an existing file.
3. Keep %TEMP% on Windows.
4. Update the README and module comment with the Linux path.

### 20. Displayed percent rounds up past the colour band, alert and pulse thresholds *needs decision*

`src/core/i18n.rs:184` · severity low · bug · effort small

**Why.** format_pct and format_window_at use `{:.0}`, which rounds. bar_fill_color, the alert crossings and the JS pulseActive all use the raw float. So 79.6 reads "80%" with an amber fill and no 80% alert, and 94.6 reads "95%" with no deep red, no pulse and no alert. This only happens with fractional upstream values, which the parsers accept.

**Fix.** 1. Floor the displayed value in one shared helper used by format_pct and format_window_at (i18n.rs:184 and :197), for example `format!("{}%", p.floor() as i64)`, with a guard for non-finite and negative values.
2. Leave the band, alert and pulse checks on the raw value.
3. Replace the half-to-even test with floor cases (79.6 -> "79%", 94.99 -> "94%").

### 21. Ctrl+wheel sends one 20 px resize per wheel event, so a touchpad scroll or pinch jumps the bubble to the min or max

`ui/bubble.js:458` · severity low · robustness · effort small

**Why.** Every ctrl+wheel event invokes bubble_resize_step with Math.sign(-deltaY), and each step saves settings, resizes every window and runs propagate. Precision touchpads and pinch (delivered as ctrl+wheel by WebView2 and WebKitGTK) send dozens of small-delta events per gesture, and only 11 events cover the whole range from 140 to 360.

**Fix.** 1. Normalise deltaY by deltaMode to pixels (line ×16, page ×~800) and add it to an accumulator.
2. Reset the accumulator after about 200 ms idle or when the direction flips.
3. Each time |acc| reaches 100, send one step with that sign and subtract 100. A mouse notch still gives exactly one step. Do not use a plain per-frame rate limit instead.

### 22. Rollback workflow accepts a version that is not higher than the live release

`.github/workflows/release-legacy.yml:51` · severity low · robustness · effort small

**Why.** The input check (lines 51-58) only checks the X.Y.Z format and that the tag matches. The publish step (lines 176-186) runs `gh release create --latest` with no version comparison. An operator in an incident who enters an unused lower version (for example 0.6.9 while 0.7.1 is live) gets a successful run, but every client sees nothing newer and stays on the broken build. A post-publish check of /releases/latest would not catch this, because `--latest` forces the new release to be latest.

**Fix.** 1. Before building, in the input step (a read token works for a public repo) or before `gh release create`, run `cur=$(gh api repos/$GITHUB_REPOSITORY/releases/latest -q .tag_name)`.
2. Fail unless $VERSION sorts strictly after ${cur#v} under `sort -V`, and also fail when the two are equal.

### 23. Provider HTTP paths (401/403 mapping, fallback order, account header) have no tests

`src/usage/anthropic.rs:52` · severity low · test-gap · effort medium

**Why.** fetch_with_fallback, try_usage_endpoint, try_messages_endpoint and both poll() implementations are untested because the URLs are hard-coded consts, and the only live test is #[ignore]. These branches decide when a credential refresh and the session-expired alert fire. The code is correct today, but a reordered match arm or a dropped ChatGPT-Account-Id header would pass all current tests.

**Fix.** 1. Pass the endpoint URLs as arguments: fetch_with_fallback(http, token, usage_url, messages_url), with poll supplying the consts. Move the ChatGPT request out into fetch(http, url, token, account_id).
2. Add net::test_server::serve_once tests: usage 401/403 gives AuthRequired with no Messages call; usage 5xx or an unparsable body falls back to Messages; a partial body gets its reset times merged; Messages 401 gives AuthRequired; Messages with no headers gives BadResponse; ChatGPT 401 gives AuthRequired and 500 gives BadResponse; the account-id header is sent only when set.
3. Update these tests to match the 403 and 5xx changes from ranks 6 and 13.

### 24. Credential refresh decision (Orchestrator::refresh, Locator) is untested

`src/usage/refresh.rs:44` · severity low · test-gap · effort small

**Why.** The Refreshed/Timeout/CliMissing decision is untested. So are Locator::first_available and the signature() implementations. spawn_cli is hard-wired, so tests cannot drive it. Inverting the signature comparison would raise a false 'session expired' alert on every auth failure, and no test would fail. This becomes more important once the backoff from rank 6 lands.

**Fix.** 1. Add a private `refresh_with(source, spawn: impl FnOnce(&RefreshHint) -> bool)` that refresh calls with spawn_cli.
2. Test it with a fake CredentialSource (a Cell counter) and a timeout of a few ms: signature changes gives Refreshed; signature never changes gives Timeout; spawn returns false gives CliMissing.
3. Add first_available tests (skips None, keeps order).
4. Add a tempfile test that LocalClaudeCreds::signature changes after a rewrite; assert on the size change, because mtime has 1 s resolution.

### 25. README privacy claim that GitHub is contacted 'only if you use the update-check feature' is false by default

`README.md:244` · severity low · docs · effort small

**Why.** update_check_interval_secs defaults to hourly (settings.rs:55-57), and update_check_due returns Now when there is no stored timestamp. A fresh install therefore fetches the GitHub latest.json at first launch and then hourly, with no user action. The request carries no credentials, but the section exists to state network traffic accurately.

**Fix.** Replace the line with: "Requests to GitHub Releases (`latest.json`) to check for updates: at first launch and then hourly by default. Set the auto-update check to Disabled in the right-click menu to stop them; 'Check for updates' still works manually."

### 26. Linux shows the in-app hint 'Tray click: show/hide', but a tray click there only opens the menu

`src/core/menu_model.rs:270` · severity low · docs · effort small

**Why.** The Controls submenu adds the hint control_tray_click on every platform. On Linux, AppIndicator never delivers tray click events, and README:82-84 and :167-168 say any click opens the menu. The in-app help therefore contradicts both the README and the actual behaviour.

**Fix.** Add the hint only `if !cfg!(target_os = "linux")`. Alternatively, add an i18n string `control_tray_click_linux` = "Tray click: menu", register it in the key list at i18n.rs:288, and choose between the two by platform. Update any menu_model test that counts Controls entries.

### 27. README menu lists omit Show widget, Restart and Reset position, and the stored-data list omits widget visibility

`README.md:161` · severity low · docs · effort small

**Why.** The real menu (menu_model.rs:233-290) contains a 'Show widget' toggle (persisted as widget_visible), Restart, and Settings > Reset position. None of these appear in README:39-40 or :161-163. The Privacy 'stores locally' list claims completeness but leaves out widget visibility and the auto-update interval. A user can still recover hidden bubbles from the tray menu, so this is a completeness gap only.

**Fix.** 1. Add 'Show widget' (hides every bubble; re-check it from the tray menu), Settings > 'Reset position' and 'Restart' to both menu lists.
2. Add 'Widget visibility' and 'Auto-update check interval' to the stored-locally list (README:246-252).

### 28. `pub` modules hide about 30 dead items left from the port *needs decision*

`src/lib.rs:3` · severity low · quality · effort medium

**Why.** lib.rs makes every module `pub` to silence dead_code, but only main.rs uses the crate, through `run`. Changing the modules to private `mod` produces 34 warnings on Linux. The following items are dead on all targets:
- net::Error::Status/Utf8 and Response::text. The three `Err(Status(401|403))` arms in anthropic.rs:75/114 and chatgpt.rs:47 can never match, because the client sets http_status_as_error(false).
- color::pulse_triangle, brighten and Rgb::lerp. These duplicate ui/layout.js's live pulse, and the doc comment claims they 'mirror' it.
- geom::corner_radius_px and point_in_rounded_rect.
- Registry::credential_signatures, Locator::signatures/new, CredentialSource::id and LocalClaudeCreds::path. Their docs falsely claim re-login is detected between polls.
- usage::Error::TokenExpired (still matched at runtime.rs:758), refresh::Outcome::StillExpired and creds::Error::Unavailable.
- Five unread fields of ProviderMetadata, plus ProviderDisplayMode, ProviderSnapshot and UsageProvider::id. default_enabled duplicates settings::default_show_*.
- i18n::format_countdown and time_until_display_change (the non-`_at` versions), Version::current, UiBridge::app and Runtime::ui.
Separately, chatgpt.rs:99 uses Option<Option<Box<T>>> plus a FlattenBoxed trait that behaves exactly like Option<T>, and unix_to_systemtime is duplicated in chatgpt.rs:92 and headers.rs:49.

**Fix.** 1. Make the modules private (`mod x;`) and keep `pub fn run`.
2. Delete the items above with their unit tests.
3. Drop the TokenExpired arm at runtime.rs:758 and its test assertion; the AuthRequired assertion already covers that path.
4. Delete the Status arms and keep the resp.status() checks. Change the net test at mod.rs:367 to assert on body().
5. Replace ProviderMetadata with `const fn live_usage(self) -> bool`, and keep settings::default_show_* as the only source of defaults.
6. Repoint the intra-doc links at the `_at` functions, and keep the `id` struct fields that signature() uses.
7. Change the ChatGPT fields to Option<RateLimit>/Option<ApiWindow>, delete FlattenBoxed, and share one pub(crate) unix_to_systemtime.
8. Put `#[cfg(windows)]` on helpers only windows.rs uses: the proxy registry parsers, the WS_* constants, rect_covers_monitor, window_style_bits_allow_fullscreen and FULLSCREEN_EDGE_TOLERANCE_PX.
9. Before deleting anything, verify with `cargo check --lib` and `cargo check --lib --target x86_64-pc-windows-msvc` (or Windows CI), and run `cargo doc` to catch broken intra-doc links.

## Rejected by verification

- `src/core/state.rs:155` A 95% crossing blocked by the cooldown is lost for good; the user never gets the 'limit is close' alert. The mechanics are as described. In src/core/state.rs:133-136 a crossing is recorded only when old_pct < threshold <= new_pct. At lines 155-161 a crossing that admit_alert (lines 168-176) refuses is dropped and nothing remembers it. last_alert_at is shared by every provider and by session-expired alerts. So the sequence 70->85 at t0, then 85->96 at t0+10min, then parked in 96..100 does produce only
- `tauri.conf.json:35` requireSignedVersion is off, so a crafted latest.json can force a downgrade to an older signed build. The finding misreads the plugin. In tauri-plugin-updater 2.14.0, verify_signed_version (updater.rs ~1927-1965) always compares the signed version with the announced one when the signature's trusted comment has a `version:` field, and returns SignedVersionMismatch if they differ, whatever `require_signed_version` is set to. The flag matters only when the signature has no version at all: it decides 
- `ui/bubble.js:360` Bubble/panel DOM glue (bubble.js, panel.js) has no tests; only the pure helpers are covered. It is true that bubble.js and panel.js have no direct tests. tests/js only covers layout.js, panel-view.js and make-latest-json.js. But the failure scenario the finding gives does not hold. It claims that renaming a payload field would pass the Rust tests and only show up at runtime. That is wrong. The test `wire_format_is_camel_case_with_nulls` in src/core/payload.rs (around lines 281-338) serial
