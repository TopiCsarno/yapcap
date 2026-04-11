---
summary: 'Release strategy, launch plan, and DE expansion policy'
read_when:
  - 'planning the v1 launch'
  - 'deciding whether to add another desktop frontend'
  - 'prioritizing scope against maintenance burden'
---

# Release Strategy

**Status:** Draft · **Last updated:** 2026-04-10

## Thesis

Ship COSMIC first. Ship it well. Treat other desktop frontends as earned expansions pulled by demand, not launch scope.

Stars and adoption follow launches, not feature matrices. A well-launched COSMIC-only v1 will outperform a quietly-dropped multi-DE v1. Desktop breadth is a retention lever (users stay because it works on their setup), not an acquisition lever.

## Why this niche is worth shipping

- No native Linux DE panel applet exists that unifies Codex + Claude Code + Cursor. The closest thing is a community Waybar shim around the macOS CodexBar binary.
- Demand is demonstrated: CodexBar has traction on macOS, users are already hacking Linux workarounds, and AI-usage anxiety is a recurring pain point for the target audience.
- COSMIC is in an adoption window. Early, polished applets in hot categories ride the wave of new users filling out their panels.
- The shared-backend architecture means future DE expansion is cheap *if* the backend/frontend split is honored.

## Non-goals for v1 launch

- Multi-DE support. COSMIC only.
- Flatpak/Snap distribution. Native `deb`/`rpm`/tarball only.
- Providers beyond Codex, Claude Code, Cursor.
- Historical charts, notifications, cost analytics, or anything that isn't "current usage in my panel."

These are not forever-nos. They are not-at-launch nos.

## Launch plan

### Prerequisites before announcing

- COSMIC applet works end-to-end for all three providers on a clean Pop!_OS install.
- README has a short demo GIF (under 10 seconds, shows panel + popup + one refresh).
- Install path is one command or one copy-paste block for the target distro.
- CodexBar credit is prominent in README, About dialog, and `--version` output.
- `doctor` command works and produces useful output when things break.
- At least one external tester (not the author) has installed it successfully from the README alone.

### Announcement targets (launch day)

- Show HN post. Title frames it as "native Linux panel applet for AI agent usage" not "CodexBar port."
- r/pop_os — primary audience, highest conversion.
- r/linux — broader reach, more noise.
- r/unixporn — if the panel looks good, this is free distribution.
- Bluesky Linux and Rust circles.
- COSMIC Matrix/Discord channels.
- Direct courtesy ping to steipete (CodexBar author) before public launch — not for permission, just good manners and possible cross-promotion.

### What to measure in the first month

- Star velocity week-over-week.
- Top 3 issue categories. Are people asking for other DEs, more providers, or bug fixes?
- Install success rate from README alone (GitHub issues tagged `install`).
- Which provider breaks most often and why.

These signals decide the post-launch roadmap. Do not pre-commit to features before seeing them.

## Post-launch expansion policy

### When to add a second frontend

Add a second frontend only when both are true:
1. COSMIC v1 is stable (bug issue rate is declining, not climbing).
2. The top 3 user requests include "support my DE" with clear volume.

Do not add a second frontend because it is architecturally possible. The shared backend exists so expansion is cheap *when justified*, not so expansion is automatic.

### Which frontend to add first

Ranked by recommended order:

1. **StatusNotifier/AppIndicator tray.** Single implementation covers KDE, XFCE, Cinnamon, MATE, LXQt, and any DE with tray support. Highest coverage per unit of work. Least UI polish ceiling, but a working tray icon is better than no icon on those DEs.
2. **GNOME Shell extension.** Biggest single-DE audience (Fedora, Ubuntu, Debian defaults). Painful stack: GJS, extension review process, breakage across GNOME versions. Only worth it if tray is insufficient and GNOME users are specifically asking.
3. **KDE Plasmoid (native QML).** Only if the tray version is clearly not good enough for KDE users and there is vocal demand for Plasma-native integration.

### Hard constraints on expansion

- New frontends must not require changes to provider crates. If they do, the backend abstraction has failed and must be fixed before the new frontend ships.
- New frontends must reuse the same `FrontendCommand`/`RuntimeEvent` boundary. No parallel protocols.
- Each new frontend adds maintenance load. If an existing frontend is rotting, fix it before adding another.
- A new frontend ships with its own milestone and acceptance gates, not bolted onto an existing release.

## Maintenance reality check

The biggest risk to this project is not scope. It is **provider API drift**.

- Claude's beta header changes.
- Codex RPC protocol shifts.
- Cursor cookie names rotate.
- OAuth endpoints change contract.

Budget more time for provider maintenance than for new features. A project that keeps working beats a project that adds frontends while the Claude provider has been broken for three weeks. If maintenance falls behind, stop all expansion work until it catches up.

## Star expectation bands

Rough, lottery-shaped, optimistic-ish. For internal expectation-setting, not promises.

| Scenario | 1 month | 6 months |
| --- | --- | --- |
| COSMIC only, good launch | 100-300 | 500-1000 |
| + tray fallback (covers ~5 DEs) | 200-500 | 1000-2000 |
| + GNOME and KDE native, maintained well | 300-800 | 2000-5000 |
| Upside: COSMIC stable ships during launch window, community boost | 2-3x any row above | 2-3x any row above |

Most projects undershoot these. The purpose of the table is to calibrate effort: do not build a GNOME extension expecting 5000 stars if the COSMIC launch landed at 80.

## Success criteria for v1

Independent of star count, v1 is a success if:

- It runs reliably on the author's daily driver for 30 days without manual intervention.
- At least 10 external users have installed it and not opened "it doesn't work" issues.
- The provider drift maintenance loop is in place and at least one drift incident has been handled cleanly.
- The shared-backend boundary has not leaked: adding a hypothetical second frontend is still a small, well-defined task.

Everything else is upside.
