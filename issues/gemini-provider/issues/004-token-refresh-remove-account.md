---
status: done
type: AFK
blocked_by:
  - 003-oauth-login-add-account
---

# Token refresh + remove-account

## Parent

[PRD](../PRD.md)

## What to build

Proactive access-token refresh, error classification machinery that later
slices wire into the UI, and the remove-account control in Settings.

Refresh details:

- Before each successful quota fetch (or whenever the adapter is asked to
  prepare a request), check whether the stored `expires_at` is within five
  minutes of `now`. If so, call `POST https://oauth2.googleapis.com/token`
  with form-encoded body (`grant_type=refresh_token`, `refresh_token`,
  `client_id`, `client_secret`). Persist the new `access_token` and
  `expires_at` (computed from `expires_in` seconds).
- Google's token endpoint does not rotate refresh tokens. The response
  has no `refresh_token` field. The adapter must preserve the original
  `refresh_token` across cycles.
- Error classification:
  - HTTP 4xx (`invalid_grant`, `invalid_token`, etc.) → permanent →
    `auth_state = ActionRequired`, `requires_user_action = true`.
  - HTTP 429 → transient. Store `rate_limit_until` on the per-account
    state from `Retry-After` if present, else exponential backoff
    (`300s * 2^(consecutive-1)`, capped at 3600s). Increment consecutive
    counter; clear on next successful refresh.
  - HTTP 5xx, network errors, timeouts → transient. Stale snapshot
    preserved. No consecutive counter change.
- This slice sets `auth_state` and `rate_limit_until` correctly in
  in-memory state; the popup UI doesn't render any special re-auth badge
  yet (that's slice 7).

Remove-account:

- Settings shows a delete control per account row.
- Confirming the deletion removes the YapCap-owned managed account
  directory (metadata, tokens, snapshot) and the corresponding
  `gemini_managed_accounts` config entry.
- Cursor's, Claude's, or Codex's own files are never touched (Gemini
  doesn't have shared host state to worry about, but the principle holds:
  only YapCap-owned state is deleted).
- The provider state and account list refresh; popup empty state returns
  if no accounts remain.

## Acceptance criteria

- [ ] Refresh threshold check (5 min before expiry) implemented and runs
  before each fetch attempt.
- [ ] Token endpoint is called with form-encoded body and the hardcoded
  client credentials; the response is parsed into rotated `access_token`
  + new `expires_at`; the original `refresh_token` is preserved when the
  response omits it.
- [ ] HTTP 4xx classified permanent; the per-account `auth_state` becomes
  `ActionRequired` and `requires_user_action` is set true.
- [ ] HTTP 429 with `Retry-After` honored; without, exponential backoff
  applied (capped at 3600s); consecutive counter incremented and cleared
  on next success. Skipped account stays on its stale snapshot.
- [ ] HTTP 5xx / network / timeout classified transient; stale snapshot
  preserved; no counter change.
- [ ] Tests cover: success path with rotated access token + preserved
  refresh token; 4xx classification; 429 with Retry-After (parses
  seconds); 429 without (applies exponential backoff with correct cap);
  5xx / network classified transient.
- [ ] Remove-account control visible in Settings; confirming deletes the
  YapCap-owned account directory and the config entry.
- [ ] After deletion, the popup updates: the row disappears, and the
  provider returns to empty state if no accounts remain.
- [ ] `just check`, `cargo test`, and `cargo fmt` clean.

## Blocked by

- 003-oauth-login-add-account

## Completion note

Added `GeminiError` (TokenRefreshRequest, TokenRefreshHttp, TokenRefreshDecode,
TokenRefreshParse, RateLimited) routed through `ProviderError::Gemini` and
`AppError` (including `is_rate_limited` / `rate_limit_retry_after_secs`).
Implemented `refresh_access_token_at` in `src/providers/gemini/oauth.rs` with
form-encoded grant_type=refresh_token + hardcoded client credentials,
`Retry-After` parsing on 429, and `parse_refresh_response` that preserves the
original refresh token when the response omits one. Added `needs_refresh` for
the 5-minute pre-expiry threshold. The shared runtime layer already maps
`is_rate_limited` errors to `rate_limit_until` + `consecutive_rate_limits` per
account (with the spec's `300s * 2^(consecutive-1)` cap at 3600s) and maps
`requires_user_action` errors to `AuthState::ActionRequired`, so adding the
classification was sufficient. Refresh entry points are gated with
`#[allow(dead_code)]` until slice 005 wires them through `fetch_account`.
Remove-account control and `delete_account` plumbing already shipped with
slice 003.
