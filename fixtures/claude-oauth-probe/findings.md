# Claude OAuth Probe Findings

Captured on April 30, 2026 against a real Claude account using `.scratch/claude-oauth-probe/probe.py`.

## Result

All four manual authorization-code exchange variants returned HTTP 200 from Claude's token endpoint:

| Variant | Requested scope | State strategy | Result |
| --- | --- | --- | --- |
| `minimal-separate` | `user:profile` | random state distinct from verifier | 200 |
| `minimal-verifier` | `user:profile` | state equals PKCE verifier | 200 |
| `broad-separate` | `org:create_api_key user:profile user:inference` | random state distinct from verifier | 200 |
| `broad-verifier` | `org:create_api_key user:profile user:inference` | state equals PKCE verifier | 200 |

## Working Request Shape

- Authorization endpoint: `https://claude.ai/oauth/authorize`
- Token endpoint: `https://console.anthropic.com/v1/oauth/token`
- Client id: `9d1c250a-e61b-44d9-88ed-5944d1962f5e`
- Redirect URI: `https://console.anthropic.com/oauth/code/callback`
- Authorization parameters include `code=true`, `response_type=code`, `code_challenge_method=S256`, `code_challenge`, and `state`.
- Returned browser value is accepted as `code#state`.
- Token exchange body is JSON with `code`, `state`, `grant_type=authorization_code`, `client_id`, `redirect_uri`, and `code_verifier`.
- Token exchange used `Content-Type: application/json`.

## Response Shape

The successful token response contains:

- `access_token`
- `refresh_token`
- `expires_in`
- `scope`
- `token_type`
- `token_uuid`
- `organization`
- `account`

The `account` object includes `email_address`, so YapCap can create or update a Claude account atomically from the token exchange response without making a separate profile request.

## Scope Findings

`user:profile` is sufficient for the token exchange and identity response.

When requesting `org:create_api_key user:profile user:inference`, Claude returned only `user:inference user:profile` in the token response. The app should not depend on `org:create_api_key` being granted.

## Product Implications

- The app's observed `400 invalid_request` is not evidence that the overall OAuth design is rejected by Claude.
- The likely failure is a difference between YapCap's implementation path and the working probe request, such as request encoding, pasted-code parsing, stale verifier/state, redirect URI mismatch, or a UI copy/paste problem.
- The login UI should make the returned `code#state` easy to paste or enter. The current UI feedback that text is not selectable should be tracked separately because it can corrupt manual validation.
- Account creation should remain atomic: do not create the account unless the token response has required token fields plus `account.email_address`.

## Capture Files

Redacted captures are saved in `.scratch/claude-oauth-probe/captures/`:

- `minimal-separate.json`
- `minimal-verifier.json`
- `broad-separate.json`
- `broad-verifier.json`
