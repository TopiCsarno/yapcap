# Codex fixtures

## Auth source
- Type: OAuth token from `~/.codex/auth.json`
- Fields: `tokens.access_token` (JWT), `tokens.refresh_token`, `tokens.id_token`, `tokens.account_id`, `last_refresh`
- On Linux: same path as macOS (no keychain involved)

## Request shape
- `GET https://chatgpt.com/backend-api/wham/usage`
- Header: `Authorization: Bearer <tokens.access_token>`
- No cookies needed for OAuth path

## Files
- `usage_oauth.json` — real response from `/wham/usage` (healthy, ~3% 5h / 24% weekly). Captured 2026-04-11.
- `error_401.json` — response when token is invalid/expired

## Notes
- `primary_window` = 5-hour session window (18000s)
- `secondary_window` = weekly window (604800s)
- `reset_at` is a Unix timestamp
- `credits.balance` is a string (not number)
- `code_review_rate_limit` and `additional_rate_limits` were null — may be non-null on other plans
- Codex CLI RPC (`codex -s read-only -a untrusted app-server`) provides similar data via JSON-RPC `account/rateLimits/read` — not captured here (no easy headless PTY on Linux for fixture capture)
