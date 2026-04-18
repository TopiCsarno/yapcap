---
summary: "Current YapCap project plan, release checklist, refactor targets, and provider notes"
read_when:
  - "starting work on YapCap"
  - "planning release polish"
  - "changing provider code, docs, packaging, or test coverage"
---

# YapCap Current Plan

Status: quiet v0.1 portfolio release plan  
Target audience: Pop!_OS / COSMIC users who use Codex, Claude Code, and Cursor  
Current stance: finish the existing COSMIC applet; do not compete with OpenUsage/CrossUsage

## Project Direction

YapCap started as a broader AI usage tracker idea, but the project is now intentionally smaller.

The goal is a small, tasteful Rust/COSMIC panel applet that solves the author's own workflow problem and works as a portfolio piece. It should demonstrate native Linux desktop work, practical Rust, local credential/session discovery, provider-specific parsing, caching, and a polished constrained UI.

Do not turn this into a universal provider tracker. Do not add more desktop environments. Do not add a plugin system. The project is valuable because it is specific and finished-looking.

## Current Scope

Supported frontend:

- COSMIC panel applet only.

Supported providers:

- Codex
- Claude Code
- Cursor

Supported local data sources:

- Codex OAuth token from `~/.codex/auth.json`
- Codex CLI RPC fallback
- Claude OAuth token from `~/.claude/.credentials.json`
- Cursor browser session cookie from supported local browsers

Current local state:

- Config: `~/.config/yapcap/config.toml`
- Snapshot cache: `~/.cache/yapcap/snapshots.json`
- Logs: `~/.local/state/yapcap/logs/yapcap.log`

## Non-Goals

These are not part of the quiet v0.1 release:

- Additional providers
- GNOME, KDE, tray, or other desktop frontends
- Historical charts
- Notifications
- Cost analytics beyond provider data already returned
- Full package repositories
- Flatpak/Snap
- Plugin architecture
- Cloud sync
- Telemetry
- Auto-updating or executing installer commands from the app

Future work should be pulled by actual personal need or user feedback, not by the old ambitious roadmap.

## Important TODO

Release-critical:

- Fix stale/fresh/error UI semantics. If refresh fails but cached data remains, the UI must say stale or failed, not live.
- Rewrite `README.md` for release users: screenshots/GIF, COSMIC-only positioning, provider support, privacy boundary, install steps, config paths, troubleshooting, and limitations.
- Add screenshots or a short GIF showing the panel and popup.
- Run clean Pop!_OS / COSMIC manual testing from checkout through install.
- Decide whether debug binaries should be documented as developer tools or excluded from release artifacts.
- Create a tagged `v0.1.0` GitHub release.

Release-nice:

- Build release binaries and attach a tarball plus `checksums.txt`.
- Add a `.deb` package if it is cheap enough, because Pop!_OS is the primary target.
- Add basic CI: formatting check, clippy, tests, and release build verification.
- Add a short `docs/troubleshooting.md` only if the README becomes too long. Prefer keeping release guidance in README first.

Not required for v0.1:

- Update notice in the app.
- `doctor` command.
- `.rpm`, AUR, APT repo, Flatpak, or Snap.
- Full settings UI.
- Secret vault fallback.

## Refactor Targets

Only refactor where it reduces release risk or future provider-maintenance pain.

Highest value:

- Split `src/providers/codex.rs`. It is the largest and riskiest provider file. Good final shape:
  - Codex OAuth fetch/normalize
  - Codex RPC fetch/normalize
  - Shared source selection and tests
- Make runtime state explicitly represent fresh, stale cached, refreshing, disabled, auth-required, and failed states.
- Keep cached snapshots on transient failure, but surface that state clearly in the panel and popup.

Medium value:

- Split browser cookie handling by browser family if touching it:
  - Chromium cookie DB/decryption
  - Firefox cookie DB parsing
  - shared cookie selection helpers
- Reduce large UI files only when changing related UI behavior. Avoid style-only churn before release.
- Consider moving debug binaries under an explicit developer-tool story.

Low value before v0.1:

- Multi-crate workspace split.
- Full runtime command/event architecture from the old spec.
- Abstract config backends.
- Secret vault implementation.

The old `docs/spec.md` is retained as historical design context. It is not the release plan.

## Release Checklist

Before tagging:

- `cargo test` passes.
- `cargo fmt --check` passes.
- `cargo clippy --all-targets -- -D warnings` passes, or known exceptions are documented.
- `cargo build --release --bin yapcap-cosmic` succeeds.
- `./install.sh` works on the target COSMIC environment.
- Applet appears in the COSMIC panel after install/restart.
- Popup opens, refreshes, and quits cleanly.
- All three providers have sane success and failure states.
- Cached data survives a transient provider failure and is visibly marked stale.
- Logs are useful and do not expose credentials, cookies, or bearer tokens.
- README has screenshots/GIF and honest limitations.
- GitHub release includes source archive, binary tarball, and checksums.

Suggested release artifacts:

- `yapcap-cosmic` release binary
- `resources/com.topi.YapCap.desktop`
- `resources/icon.svg`
- `install.sh`
- `checksums.txt`
- Optional `.deb`

Do not push or publish anything without explicit permission.

## Basic CI Plan

Start with a small GitHub Actions workflow:

- Trigger on pull requests and pushes to main.
- Install stable Rust.
- Cache Cargo dependencies.
- Run `cargo fmt --check`.
- Run `cargo clippy --all-targets -- -D warnings`.
- Run `cargo test`.
- Run `cargo build --release --bin yapcap-cosmic`.

If libcosmic system dependencies are required on CI, document them in the workflow instead of hiding failures.

## Manual Testing

Primary release target:

- Pop!_OS with COSMIC.

Minimum manual scenarios:

- Fresh checkout build and install.
- Applet visible in panel.
- Popup opens and provider tabs switch without resize flicker.
- Manual refresh works.
- Config file is created on first run.
- Snapshot cache is created after successful refresh.
- Logs are written under the XDG state directory.
- Provider unavailable states are readable.
- Provider auth failure states are readable.
- Cached snapshot remains visible after transient failure and is marked stale.

Provider scenarios:

- Codex logged in with valid auth file.
- Codex auth missing or expired.
- Codex RPC unavailable while OAuth still behaves predictably.
- Claude credentials present and OAuth succeeds.
- Claude OAuth succeeds, or failed OAuth leaves cached data visibly stale.
- Cursor browser cookie import succeeds.
- Cursor browser cookie missing or expired.
- Browser selection override works with `YAPCAP_CURSOR_BROWSER`.

Do not repeat provider QA across multiple desktop environments for v0.1. Other DEs are out of scope.

## Provider Notes

### Codex

Auth source:

- OAuth token from `~/.codex/auth.json`
- Fields include `tokens.access_token`, `tokens.refresh_token`, `tokens.id_token`, `tokens.account_id`, and `last_refresh`

OAuth request:

- `GET https://chatgpt.com/backend-api/wham/usage`
- Header: `Authorization: Bearer <tokens.access_token>`

Fixture notes:

- `fixtures/codex/usage_oauth.json` is a real `/wham/usage` response captured 2026-04-11.
- `primary_window` is the 5-hour session window.
- `secondary_window` is the weekly window.
- `reset_at` is a Unix timestamp.
- `credits.balance` is a string.
- Codex RPC can provide similar rate-limit data via JSON-RPC `account/rateLimits/read`.

### Claude

Auth source:

- OAuth token from `~/.claude/.credentials.json`
- Field: `claudeAiOauth.accessToken`
- Required scope: `user:profile`

OAuth request:

- `GET https://api.anthropic.com/api/oauth/usage`
- Headers:
  - `Authorization: Bearer <accessToken>`
  - `anthropic-beta: oauth-2025-04-20`

Fixture notes:

- `fixtures/claude/usage_oauth.json` is a real `/api/oauth/usage` response captured 2026-04-11.
- `utilization` is a float from 0 to 100, not 0 to 1.
- `resets_at` is ISO 8601 with microseconds and UTC offset.
- Extra usage appears only when enabled on the account.

### Cursor

Auth source:

- Browser session cookie `WorkosCursorSessionToken` from `cursor.com`
- Cookie format: `user_XXXX::JWT`
- Cursor's local `~/.config/cursor/auth.json` bearer token works for some `api2.cursor.sh` endpoints, but not for the `cursor.com` usage APIs.

Web API requests:

- `GET https://cursor.com/api/usage-summary`
- `GET https://cursor.com/api/auth/me`
- Header: `Cookie: WorkosCursorSessionToken=<value>`

Fixture notes:

- `fixtures/cursor/usage_summary.json` is a real response captured 2026-04-11.
- `fixtures/cursor/auth_me.json` is a real identity response.
- `fixtures/cursor/stripe_profile.json` is useful billing/plan context but does not provide usage percentages.
- `used` and `limit` in `plan` are request counts.
- `autoPercentUsed`, `apiPercentUsed`, and `totalPercentUsed` are separate dimensions.
- `billingCycleEnd` is the reset point.

## Positioning

Use this public framing:

> YapCap is a native COSMIC panel applet for Linux that shows local usage state for Codex, Claude Code, and Cursor.

Avoid this framing:

> Universal AI usage tracker.

Avoid competing directly with OpenUsage/CrossUsage. YapCap's value is that it is small, local-first, COSMIC-native, and finished.

Portfolio/CV framing:

> Built YapCap, a Rust/COSMIC panel applet for Linux that surfaces local usage limits for Codex, Claude Code, and Cursor by integrating OAuth credentials, CLI output, browser session cookies, cached refresh state, and native panel UI.
