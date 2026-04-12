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

## Notes
- `utilization` is a float (0–100), not 0–1
- `resets_at` is ISO 8601 with microseconds and UTC offset
- `seven_day_opus`, `seven_day_sonnet`, `seven_day_cowork` are all null on default plans
- `seven_day_oauth_apps` is null (separate per-app limit, not standard)
- `iguana_necktie` is an undocumented field, always null in our sample
- `extra_usage` only present when Extra Usage is enabled on account; has `is_enabled`, `monthly_limit` (USD), `used_credits` (USD), `utilization` (0–100)
- Web API path (claude.ai sessionKey cookie) uses different endpoints:
  - `GET https://claude.ai/api/organizations` → org UUID
  - `GET https://claude.ai/api/organizations/{orgId}/usage` → different response shape (not captured here)
