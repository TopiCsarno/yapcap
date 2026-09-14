# Changelog

All notable changes to YapCap are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Added per-account COSMIC panel visibility, with marked accounts displayed
  side-by-side as compact panel cells.
- Added Grok subscription usage tracking with browser OAuth, explicit Grok CLI
  credential import/restore, managed accounts, and host Active matching.
- Added Z.AI Coding Plan usage tracking with managed API keys, five-hour, weekly,
  and optional MCP windows, plus content-aware OpenCode key detection and prefill.

## [0.6.0] - 2026-09-03

### Added

- Added Google Antigravity usage tracking with Google OAuth accounts, grouped
  Gemini and Claude/GPT quota windows, reauthentication, and multi-account
  support.
- Added Kimi for Coding usage tracking with API-key managed accounts, weekly and
  rate-limit windows, account reauthentication, and multi-account support.
- Re-added OpenCode Go usage tracking with API-key managed accounts and 5-hour,
  weekly, and monthly windows.
- Re-added optional one-time OpenCode `auth.json` credential discovery: compatible
  keys prefill Minimax, Kimi, and OpenCode Go forms, while Codex and GitHub
  Copilot offer explicit OAuth imports. OpenCode is not used as a live refresh
  or synchronization source.
- Added automatic provider detection with live re-detection, tri-state provider
  enablement, and first-run empty states that guide users to account setup.
- Added adaptive provider navigation, dedicated provider management, and
  detected-provider setup actions.

### Changed

- Improved provider usage meters, account matching, popup navigation, and usage
  amount handling across provider tabs.
- Redesigned provider account management with shared authentication and account
  lifecycle behavior across providers.

### Fixed

- Improved API-key account validation, popup sizing and transparency, account
  actions, and reauthentication badge wrapping.

## [0.5.2] - 2026-07-13

### Added

- Added Minimax usage tracking with API-key accounts.
- Added scoped Claude weekly-limit parsing.

### Changed

- Hardened multi-process refresh coordination, provider refresh backoff, and
  diagnostics.

## [0.5.1] - 2026-06-16

### Added

- Added shared runtime state and refresh coordination across panel instances.

### Fixed

- Updated Claude OAuth to use the working `claude.ai` token endpoint.

## [0.5.0] - 2026-05-19

### Added

- Added GitHub Copilot usage tracking with device-flow login, Free and paid
  quota support, and multi-account management.
- Added Gemini usage tracking with Google OAuth accounts.

### Fixed

- Preserved account selection after provider login.

## [0.4.0] - 2026-05-12

### Added

- Added managed authentication for Claude, Codex, and Cursor.
- Added multi-account usage rendering and concurrent provider refreshes.
- Added COSMIC configuration persistence and Flatpak packaging.

[Unreleased]: https://github.com/TopiCsarno/yapcap/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/TopiCsarno/yapcap/compare/v0.5.2...v0.6.0
[0.5.2]: https://github.com/TopiCsarno/yapcap/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/TopiCsarno/yapcap/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/TopiCsarno/yapcap/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/TopiCsarno/yapcap/releases/tag/v0.4.0
