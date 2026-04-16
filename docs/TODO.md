# TODO

- Make provider refreshes independent in the UI. The popup should update providers one by one as each refresh completes instead of waiting for the whole batch. Right now one slow source blocks the visible update for everything.
- Split Claude browser selection from Cursor browser selection. Claude web should not depend on `cursor_browser`.
- Add an env var override for browser selection during testing, e.g. `firefox` / Chromium-family (`brave`, `chrome`, `edge`).
- Add per-provider source mode in config instead of relying only on env vars for manual source forcing.
- Improve runtime/status messaging for partial refresh failure so users can tell which providers are stale vs freshly updated.
- Decide whether Claude web stays experimental long-term or gets a more browser-faithful request path.
- Replace ad hoc source ordering logic with an explicit source-plan layer in runtime/config.
- Codex: keep source order `OAuth -> RPC -> PTY`; `PTY` exists only as last-resort fallback when OAuth and RPC fail. Investigate a more reliable non-interactive or bounded-probe status path, because interactive `/status` footer scraping is fragile.
- providers shoud show 5h usage when selected (instead of weekly) on the top
- extra credits ui is ugly for codex
- codex should be Oath first now its rcp first
