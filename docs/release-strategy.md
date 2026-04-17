---
summary: 'Release strategy, launch plan, distribution, and DE expansion policy'
read_when:
  - 'planning the v1 launch'
  - 'deciding how users should install and receive updates'
  - 'deciding whether to add another desktop frontend'
  - 'prioritizing scope against maintenance burden'
---

# Release Strategy

**Status:** Draft · **Last updated:** 2026-04-16

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
- GitHub release includes a tarball and at least a `.deb` package. `.rpm` is useful if Fedora/COSMIC testing is ready, but not required for the first public push.
- The app surfaces a lightweight update notice when a newer GitHub release exists. No self-update execution in v1.
- CodexBar credit is prominent in README, About dialog, and `--version` output.
- `doctor` command works and produces useful output when things break.
- At least one external tester (not the author) has installed it successfully from the README alone.

### Distribution and update policy

The day-1 channel is **GitHub Releases with native Linux artifacts plus in-app update awareness**, not a full package repository.

YapCap is a COSMIC panel applet, not a plain Rust CLI. A useful install must place the binary, desktop entry, icon, and metadata in the right XDG locations, then account for COSMIC panel/app launcher discovery. `cargo install` only installs binaries and does not handle desktop integration.

Day-1 artifacts:

- `.tar.gz` archive for manual installs and debugging.
- `.deb` package for Pop!_OS, Ubuntu, and Debian-family users. This is the primary polished artifact because the initial audience is COSMIC/Pop-oriented.
- `checksums.txt` for release asset verification.

Do not build an APT repo before interest is proven. A third-party apt source adds signing-key management, repository metadata, CI publishing, install-doc complexity, and a support surface when apt sources break. It also asks brand-new users to trust a repository before the project has earned that trust. A downloadable `.deb` plus in-app update notice is enough for the first public push.

Day-1 update reach comes from three layers:

1. **In-app update notice.** Check GitHub Releases periodically and show a small notice in the applet popup when a newer version exists. This reaches users who downloaded a binary and still run it. It should link to the release page or copy an upgrade command. It must not auto-execute installers.
2. **GitHub release notifications.** Tell interested users to watch releases. This is weak but free and honest.
3. **AUR if cheap.** Add `yapcap-bin` if it is less than roughly half a day of work. It fits Arch user expectations and updates reach users through helpers such as `yay` or `paru`, but it is not required for the first announcement.

Package-manager escalation rules:

- Add an APT repository only if `.deb` downloads, user issues, or direct requests show Pop!_OS/Ubuntu users are sticking around.
- Add `.rpm` and eventually DNF repo support only after Fedora/COSMIC testing is real.
- Treat package repositories as retention infrastructure after early demand is visible, not as a launch prerequisite.

Do not optimize v1 around crates.io:

- Current COSMIC stack dependencies include Git dependencies, especially `libcosmic`, which blocks normal crates.io publishing.
- Even if crates.io publishing becomes possible later, `cargo install` is still a poor primary UX for a desktop applet because it skips desktop entry and icon installation.
- `cargo install --git` can remain a contributor/testing path, not the public recommendation.

Do not prioritize Flathub/Snap for v1. They may be useful later, but sandboxing and host integration are awkward for a panel applet that reads local CLI auth, browser cookies, keyring state, and desktop session data.

Package-manager cost model:

- Publishing release assets on GitHub is free.
- `.deb`/`.rpm` files are free to produce and attach to releases.
- AUR is free, but shifts build/update work to Arch-style user workflows.
- APT/DNF repos can be hosted cheaply or free on static hosting, but require signing and maintenance discipline.
- Flathub submission is free, but review/sandbox maintenance is the cost.
- Homebrew taps are free, but Homebrew is not a good fit for COSMIC panel integration and should not be a v1 target.

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
