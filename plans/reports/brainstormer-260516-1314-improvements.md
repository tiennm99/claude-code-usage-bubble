# Brainstorm: high-leverage improvements for claude-code-usage-bubble

Date: 2026-05-16
Baseline: v0.1.2 — bubble UX just polished (commits 5df75c9...7f8ccf0), updater bug just fixed (a132c02), auto-update frequency just added (2ca5052), windows release pipeline just added (60cde29).

Read scope: README, src/app.rs, src/bubble.rs, src/panel.rs, src/usage/{types,mod,anthropic}.rs, Cargo.toml, last 10 commits.

---

## Reality-check (cuts I refuse to dress up)

- Codebase is heavily Win32. windows-rs 0.58, GDI, AppBar, WinHTTP, registry-based autostart. Anything multi-platform is a near-rewrite of the UI shell. Most polish effort beats most expand effort.
- The product is small and good. Two bars x two providers x one bubble. Do not bloat it. The risk is feature creep that turns it into the thing it was reacting against.
- The user is also the author — there is no marketing motion to feed. Distribution work only pays off if you actually want strangers using it. Decide that first; everything in section 4 and 7 is conditional on yes.

---

## 1. Bubble UX

What 360 Security / IObit memory balls do that this does not: a one-tap action (their boost button), idle micro-animation that draws the eye, a state-change pulse when a number crosses a threshold, edge-dock that fully tucks against the screen edge showing only a sliver.

### 1a. Threshold pulse + colour-state escalation
- One-liner: when 5h utilization crosses 80 / 95 percent, the ring pulses once (already have TIMER_PULSE) and the accent shifts amber to red; when it drops after reset, single subtle release animation.
- Why: the user reason to look at the bubble is "am I close?" — passive colour is fine when sitting at 30 percent, but at 92 percent you want the bubble to grab you once and then shut up.
- Effort: S. Pulse timer already exists; need state-machine on percentage threshold + hysteresis (do not pulse every poll).
- Mistake if: you make it pulse continuously above a threshold. That is a notification, not a bubble. One pulse per crossing, that is it.

### 1b. Edge-dock sliver mode
- One-liner: when snapped to an edge for >5s with no interaction, contract to a thin coloured stripe (~6px) along the edge showing only the higher-of-(5h, 7d) percentage as a bar. Hover or click expands back.
- Why: the bubble at 200px is a lot of permanent screen real estate. The memory ball UX wins because at rest it is almost invisible.
- Effort: M. Need new render path + hover-region + state transition. Risk of getting the hit-test wrong.
- Mistake if: the dock is so subtle the user cannot find it again. Keep a 1-2px coloured accent.

### 1c. Dark/light auto-follow (Windows system theme)
- One-liner: subscribe to WM_SETTINGCHANGE ImmersiveColorSet + read AppsUseLightTheme from HKCU; AppState.is_dark follows instead of being static.
- Why: bubble currently looks alien on the opposite theme. This is the cheapest feels-native win available.
- Effort: S. Single registry read on startup + WM_SETTINGCHANGE handler.
- Mistake if: you also try to do per-bubble theme override. YAGNI — system theme is correct ~100 percent of the time.

### 1d. Accent-colour follow (system)
- One-liner: read HKCU Software Microsoft Windows DWM AccentColor and tint the ring fill at low utilization (where the colour is currently arbitrary).
- Why: makes the widget feel like a system component. Cheap perceived-quality lift.
- Effort: S.
- Mistake if: you let accent colour override the amber/red threshold states. Threshold > accent.

### Cut from section 1
- Custom theming UI. No. YAGNI. System theme is enough.
- Bubble shapes other than rounded-rect. The whole circle framing in the README is already a fib (it is a 3:1 rounded rect). Do not add more shape options.

---

## 2. Information density

The data already in UsageWindows is only utilization (0-100) + resets_at. Anything else needs new endpoint work or local computation. Be honest about that cost.

### 2a. Burn rate + projected exhaustion (computed, no new API)
- One-liner: track utilization samples in a small ring buffer; in the panel, show "at this rate, you will hit 100 percent in ~Xh" beneath the 5h bar.
- Why: the actual decision the user is making is "do I context-switch now or finish this thought?" Projected exhaustion answers that; raw percentage does not.
- Effort: S-M. Ring buffer + linear regression over last N samples; only show when slope is meaningfully positive.
- Mistake if: you put the projection on the bubble itself. It is noise there. Panel-only.

### 2b. Delta-since-last-reset summary in panel
- One-liner: when the 5h window resets, snapshot the previous peak percentage. Show "Last cycle peaked at 88 percent" in the panel.
- Why: builds intuition over time about whether usage is growing or shrinking without any backend.
- Effort: S. One extra field in settings/state.
- Mistake if: you try to show a chart. Tiny number, one line, done.

### 2c. Do NOT add: token count, dollar cost, model breakdown
- Cut. Anthropic oauth/usage endpoint returns utilization buckets, not token counts. The fallback path scrapes rate-limit headers. Neither gives reliable dollar cost. Inventing one will be wrong and erode trust. Do not ship guesses as facts.

### Cut from section 2
- Per-conversation/per-project breakdown. Anthropic does not expose this in oauth/usage. Do not promise what you cannot deliver.

---

## 3. Workflow integrations

### 3a. Threshold balloon notification (one-shot)
- One-liner: at 80 / 95 percent crossings, fire a Shell_NotifyIcon balloon ("Claude 5h at 95 percent — resets in 42m"). Already have BALLOON_COOLDOWN and last_balloon_at plumbed; extend the trigger from "update available" to "threshold crossed".
- Why: a user with the bubble auto-hidden during fullscreen game/video still wants to know they are about to run out. This is the single highest-impact integration because the integration target is the user, not another app.
- Effort: S. Plumbing exists; just add the trigger.
- Mistake if: you fire it every poll above 95 percent. Once per crossing, per reset cycle.

### 3b. Cut: tailing Claude Code logs / hooks
- The Claude Code CLI does emit logs (~/.claude/) and supports hooks, but inferring usage from them is fragile vs. the official oauth/usage endpoint you are already hitting. Do not dual-source the same number.

### Cut from section 3
- Slack/Discord/webhook out. No. This is a personal desktop widget. If you want webhooks, you are building a different product.
- Pause Claude Code when over limit. Out of scope. The bubble observes, does not control.

---

## 4. Distribution + onboarding

Only worthwhile if you want strangers using it. State the goal explicitly before doing any of this.

### 4a. winget manifest
- One-liner: submit a manifest to microsoft/winget-pkgs once you have at least one signed (or accepted-unsigned-with-hash) release.
- Why: winget install tiennm99.ClaudeCodeUsageBubble is the only Windows install command anyone actually wants to run. Free distribution.
- Effort: S (one PR to winget-pkgs) once the release artifact has a stable URL + SHA256, which it now does.
- Mistake if: you submit before code signing. winget accepts unsigned packages but SmartScreen still nags; that user pain accrues to your repo, not winget.

### 4b. First-run is-everything-working check
- One-liner: on first launch with no settings.json, run the same checks as --diagnose once: can I find Claude creds? Can I reach Anthropic? Show a tiny one-time panel that says "Claude OK, Codex not configured (enable in Models menu)" and dismisses.
- Why: silent failure is the worst onboarding outcome. The bubble showing dash-percent tells users nothing.
- Effort: S. --diagnose already exists; reuse the logic.
- Mistake if: it becomes a wizard. One panel, one dismiss, never again.

### Cut from section 4
- MSIX. Cut. MSIX requires the Store or sideload pain. Cost > benefit for an indie widget.
- MSI installer. Cut. A 4-MB single exe that drops in LOCALAPPDATA is better than an MSI for this audience.
- Crash dumps. The app is small and runs in a single Win32 message loop. simplelog to TEMP claude-code-usage-bubble.log already covers 95 percent of post-mortem needs.

---

## 5. Multi-platform reality check

Skip this axis. The codebase is windows::Win32:: from top to bottom — bubble window, panel, tray, AppBar, registry autostart, WinHTTP. Porting macOS/Linux is a full UI-shell rewrite (~70 percent of the code), and the value proposition (a desktop memory ball) does not translate cleanly — macOS users expect a menu-bar app, Linux users expect a tray icon and most distros do not have a stable always-on-top floating layer.

If you genuinely want cross-platform, the right move is not to port this — it is a separate claude-code-usage-menubar for macOS that reuses src/usage/ and src/creds/ as a library crate. Worth noting but not worth doing unless someone asks.

---

## 6. Updater roadmap

### 6a. SHA256 verification of downloaded artifact
- One-liner: GitHub Releases shipped via your windows-release.yml already produce a stable URL; publish a SHA256SUMS file as part of the release, fetch + verify before swapping the exe.
- Why: defends against a compromised CDN / MITM regardless of code signing. Way cheaper than signing.
- Effort: S. Add to the GH Actions release step; verify in src/update/.
- Mistake if: you skip this because GitHub releases use HTTPS so MITM is impossible. HTTPS is not integrity; an attacker with a release-asset upload token or a compromised CI also matters.

### 6b. Code signing — defer, do not romanticise
- A standard EV cert is ~300-600 dollars/year and an OV cert ~200-400 dollars/year, and even with OV you still wait weeks for SmartScreen reputation. EV gets you immediate SmartScreen trust but the HSM-bound key is operationally annoying for solo devs.
- Verdict: defer until install volume justifies it (>1000 downloads/release, say). The Run-anyway friction is real but survivable; the cost-per-user of a cert at low volume is very high.
- Effort if pursued: M (cert setup) + ongoing key custody pain.
- Mistake if: you sign without rotating to an HSM-backed solution. A leaked signing key is worse than no signing.

### 6c. Beta channel via GitHub pre-releases
- One-liner: settings.json already supports install_channel; surface it in right-click menu as "Channel Stable / Beta" so people can dogfood.
- Why: low-cost, high-trust signal for early adopters; gives you canaries before stable.
- Effort: S. One menu item, one settings field, one filter on the GH Releases list.
- Mistake if: you ship a beta that bricks the updater (see v0.1.2). Add a "downgrade to last stable" affordance.

### Cut from section 6
- Delta updates. No. The exe is ~4 MB. Bandwidth is not the bottleneck. Delta-patching machinery is bug surface.
- Rollback. Mostly cut. Keeping the previous exe as .bak on update and a --rollback flag is fine (S), but a full rollback UI is overkill.

---

## 7. Brand / discovery

Only worth doing if you actually want users beyond yourself. Sketched at low cost:

### 7a. Demo GIF in README (top, above install)
- One-liner: 6-8 second loop showing bubble at idle, drag-to-edge snap, left-click expand panel, right-click menu.
- Why: the entire product is visual. Words do not sell a floating bubble — the GIF will convert orders-of-magnitude better than the current shield-badges.
- Effort: S. ScreenToGif then optimise to <1 MB.
- Mistake if: you record it on a 4K monitor and it weighs 8 MB and breaks the README. Keep it <1.5 MB.

### 7b. GitHub topics + a short tagline
- Topics: windows-desktop, rust, claude-code, codex, usage-monitor, widget, system-tray. The README H1 is fine but the GitHub repo description / About should be one tweet-length line.
- Effort: 5 minutes. Free.

### Cut from section 7
- Landing page / dedicated site. Cut until install volume justifies. The repo is the landing page.
- Twitter/Bluesky launch posts. Author call. Not a product question.

---

## Top 5 I would ship first, ranked

1. Dark/light auto-follow (1c). S effort, immediate feels-native lift, zero risk. Ship today.
2. Threshold balloon at 80/95 percent (3a). S effort. The single biggest jump in usefulness on the entire list — it converts the widget from a thing you have to look at into a thing that tells you.
3. Demo GIF in README (7a). S effort, only useful if you want strangers, but if you do it is the unlock.
4. SHA256 verification in updater (6a). S effort, plugs a real integrity hole that exists today, cheaper than signing.
5. Edge-dock sliver mode (1b). M effort but this is the differentiator vs. the upstream taskbar-widget approach — it is what floating bubble actually wants to be. Worth the M.

Honourable mention: first-run sanity check (4b) — only if you ship #3 first and start getting "it shows nothing, is it broken?" issues.

---

## Unresolved questions

1. Do you actually want external users? Section 4 and 7 are no-ops if not.
2. Are you willing to take ~200-600 dollars/yr on code signing within the next year, or is "unsigned + SmartScreen warning" the permanent stance? Affects whether 6b is a roadmap item or a no.
3. Is there a deliberate reason Settings already has install_channel but no UI surface (6c)? If it was intentional dormancy, fine; if it was an oversight, that is a free win.
4. macOS port — yes/no/later? Affects whether src/usage/ and src/creds/ should be refactored into a separate crate now (cheap) vs. later (painful).

---

Status: DONE
Summary: 14 picks across 6 of 7 axes (multi-platform skipped with rationale). Top-5 ranked. 4 unresolved questions for the user.
