# Claude fixtures

## Auth source
- Type: OAuth token from `~/.claude/.credentials.json`
- Field: `claudeAiOauth.accessToken` (sk-ant-oat... prefix)
- On macOS: also stored in Keychain service `Claude Code-credentials`
- On Linux: file only, no keychain
- Required scope: `user:profile` (tokens with only `user:inference` cannot call the usage endpoint)

## Request shape
- `GET https://api.anthropic.com/api/oauth/usage`
- Headers:
  - `Authorization: Bearer <accessToken>`
  - `anthropic-beta: oauth-2025-04-20`

## Credentials structure
```json
{
  "claudeAiOauth": {
    "accessToken": "sk-ant-oat-...",
    "refreshToken": "sk-ant-oat-...",
    "expiresAt": 1775917696719,
    "scopes": ["user:file_upload", "user:inference", "user:mcp_servers", "user:profile", "user:sessions:claude_code"],
    "subscriptionType": "pro",
    "rateLimitTier": "default_claude_ai"
  },
  "organizationUuid": "..."
}
```

## Files
- `usage_oauth.json` — real response from `/api/oauth/usage` (18% 5h, 90% weekly, 48.8% extra usage). Captured 2026-04-11.
- `error_401.json` — response when token is invalid
- `web_organizations.json` — real response from `GET https://claude.ai/api/organizations`. Captured manually from Brave DevTools.
- `web_usage.json` — real response from `GET https://claude.ai/api/organizations/{orgId}/usage`. Captured manually from Brave DevTools.
- `web_account.json` — real response from `GET https://claude.ai/api/account`. Captured manually from Brave DevTools.

## Notes
- `utilization` is a float (0–100), not 0–1
- `resets_at` is ISO 8601 with microseconds and UTC offset
- `seven_day_opus`, `seven_day_sonnet`, `seven_day_cowork` are all null on default plans
- `seven_day_oauth_apps` is null (separate per-app limit, not standard)
- `iguana_necktie` is an undocumented field, always null in our sample
- `extra_usage` only present when Extra Usage is enabled on account; has `is_enabled`, `monthly_limit` (USD), `used_credits` (USD), `utilization` (0–100)
- Web API path (claude.ai sessionKey cookie) uses different endpoints:
  - `GET https://claude.ai/api/organizations` → org UUID
  - `GET https://claude.ai/api/organizations/{orgId}/usage` → current captured shape is effectively the same usage schema as OAuth
  - `GET https://claude.ai/api/account` → account email, display name, org capabilities, and plan hints
- Current web fixtures are real captures from Brave DevTools, not placeholders.
- Claude web should be treated as experimental in the app runtime.
  - Reason: the cookie lookup works, but non-browser HTTP requests still appear vulnerable to Cloudflare challenge behavior.
  - Runtime policy: do not include web in the normal Claude fallback chain.
  - Manual testing only: `YAPCAP_CLAUDE_FORCE_SOURCE=web`

## Known issue

- Attempting to capture the Claude web endpoints with `curl` and a valid Brave `sessionKey` currently returns Cloudflare challenge HTML instead of JSON.
- Observed response: HTTP `403` with `cf-mitigated: challenge`.
- Result: live Claude web fixture capture cannot currently be automated with plain `curl` in this repo workflow.
- Workaround used: capture the three web JSON response bodies manually from Brave DevTools / Console.
