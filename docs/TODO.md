# TODO

- Make provider refreshes independent in the UI. The popup should update providers one by one as each refresh completes instead of waiting for the whole batch. Right now one slow source blocks the visible update for everything.
- Add per-provider source mode in config instead of relying only on env vars for manual source forcing.
- Improve runtime/status messaging for partial refresh failure so users can tell which providers are stale vs freshly updated.
- Decide whether Claude web stays experimental long-term or gets a more browser-faithful request path.
- Replace ad hoc source ordering logic with an explicit source-plan layer in runtime/config.
- Codex: keep source order `OAuth -> RPC -> PTY`; `PTY` exists only as last-resort fallback when OAuth and RPC fail. Investigate a more reliable non-interactive or bounded-probe status path, because interactive `/status` footer scraping is fragile.
- extra credits ui is ugly for codex
- Add `update-informer` crate to print a one-line notice on launch when a newer GitHub release is available. No auto-exec — just a nudge for users who download a binary and never check back. Longer term: publish to crates.io and submit an AUR package.
