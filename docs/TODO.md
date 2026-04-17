# TODO

## Runtime

- Add runtime staleness state. Users should be able to tell which provider data is fresh, stale-but-cached, or failed on the last refresh.
- Replace ad hoc provider fallback code with an explicit source-plan layer in runtime/config.
- Add per-provider source mode in config instead of relying only on `YAPCAP_*_FORCE_SOURCE` env vars.

## Providers

- Codex: keep auto order `OAuth -> RPC -> PTY`. PTY should remain last-resort only.
- Codex: investigate a more reliable non-interactive status path. Interactive `/status` footer scraping is fragile.
- Claude web: decide whether it stays experimental/forced-only or gets a browser-faithful request path for normal fallback.

## UI

- Redesign Codex credits display. Current `credits0.00 spent`-style presentation is ugly and unclear.

## Release

- Add a one-line update notice when a newer GitHub release exists. No auto-exec; link/copy upgrade instructions only.
- Longer term: publish to crates.io when COSMIC dependencies allow it, and consider an AUR package.
