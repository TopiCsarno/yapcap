---
status: done
type: AFK
blocked_by:
  - 002-pure-deep-modules
---

# OAuth login + add-account end-to-end

## Completion note

Implemented Gemini loopback-callback OAuth flow end-to-end:
hardcoded `OAUTH_CLIENT_ID`/`OAUTH_CLIENT_SECRET` constants, PKCE S256
challenge, state-nonce validation, form-encoded token exchange against
`oauth2.googleapis.com/token` (with `client_secret`), single post-login
`loadCodeAssist` call, success HTML page, cancellable login task,
managed-account storage write (`metadata.json` + `tokens.json`), and
dedupe-by-normalized-email upsert. Wired into the COSMIC settings UI
through new `Add account` controls, a Gemini account row with
re-auth/delete actions, login-state messages on `AppModel`, and matching
i18n strings. Tests cover OAuth URL composition, PKCE generation,
token-response parsing, id-token decoding, account dedupe, and the
commit-login storage round-trip including duplicate-email upsert.

## Parent

[PRD](../PRD.md)

## What to build

Native loopback-callback OAuth flow that lets a user click "Add account"
in Gemini settings, sign in to Google in their browser, and end up with a
managed Gemini account stored under
`<state-root>/yapcap/gemini-accounts/<id>/`. After this slice lands, the
new account row is visible in Settings with the email and plan badge. No
quota fetching yet — refresh is a no-op that leaves the account in
`Loading`.

Reference: `src/providers/codex/login.rs` for loopback OAuth state machine
shape; `src/providers/claude/account/` for managed storage patterns.

Flow specifics:

- YapCap binds an ephemeral localhost TCP port and serves a single-purpose
  HTTP handler at `/oauth/callback`.
- Authorization URL: `https://accounts.google.com/o/oauth2/v2/auth` with
  the hardcoded gemini-cli `client_id`, `redirect_uri=http://localhost:<port>/oauth/callback`,
  `response_type=code`, `code_challenge` (PKCE S256), `code_challenge_method=S256`,
  `state` nonce, `access_type=offline`, `prompt=consent`, and the four
  scopes used by gemini-cli (`https://www.googleapis.com/auth/cloud-platform
  openid https://www.googleapis.com/auth/userinfo.profile
  https://www.googleapis.com/auth/userinfo.email`).
- Browser opens via the existing libcosmic helper (uses
  `org.freedesktop.portal.OpenURI` under Flatpak).
- On callback, YapCap validates the state nonce and exchanges the code at
  `https://oauth2.googleapis.com/token` with form-encoded body
  (`grant_type=authorization_code`, `code`, `code_verifier`, `client_id`,
  `client_secret`, `redirect_uri`). Form-encoded — not JSON.
- One call to `loadCodeAssist` at flow completion to capture
  `cloudaicompanionProject` and `currentTier.id` into account metadata.
- Account is written to managed storage with the id_token's `email`,
  `sub`, `hd` (if present), `name`, plus the tier id and project id from
  loadCodeAssist. Bearer tokens go in `tokens.json`; non-secret metadata
  in `metadata.json`.
- Duplicate add by normalized email upserts the existing managed account
  directory; does not create a second row.
- Cancel during login aborts the listener cleanly and commits nothing.
- Successful add immediately selects the new account in single-account
  mode; in show-all mode appends to existing selections when capacity
  allows.

The OAuth client_id and client_secret are hardcoded constants in the
provider crate, sourced from the public `@google/gemini-cli` bundle.

## Acceptance criteria

- [ ] Hardcoded `OAUTH_CLIENT_ID` and `OAUTH_CLIENT_SECRET` constants
  declared in the provider's oauth module (values per PRD).
- [ ] Settings → Gemini → Add account button opens the system browser to
  Google's authorize URL with PKCE S256 and a state nonce.
- [ ] Callback handler validates state, exchanges code for tokens,
  decodes `id_token` using the slice 2 decoder, and calls `loadCodeAssist`
  exactly once.
- [ ] Successful callback returns a static HTML page reading "Signed in
  to Gemini — you can close this tab and return to YapCap."
- [ ] Cancel button visible while login is running; pressing it aborts the
  listener and the OAuth task without writing any account state.
- [ ] Failed flows (browser closed, Google error, network drop, state
  mismatch, missing required claim) show a clear error in the YapCap UI;
  no partial account is committed.
- [ ] Account written to
  `<state-root>/yapcap/gemini-accounts/<id>/` with `metadata.json`
  containing email, sub, hd (if present), name, tier id, project id, and
  timestamps. `tokens.json` contains `access_token`, `refresh_token`,
  `expires_at`, and `scope`.
- [ ] Duplicate login by normalized email (`trim + ASCII lowercase`)
  upserts the existing account directory rather than creating a second
  one.
- [ ] New account row appears in Settings with email-derived label and
  plan badge from the tier mapper. Account is selected immediately in
  single-account mode.
- [ ] Under Flatpak, browser open + loopback listener + tokens-write all
  function without new permissions.
- [ ] Tests cover state-nonce validation, PKCE verifier/challenge
  generation, dedupe-by-normalized-email upsert behavior, account
  storage round-trip (write then read back), and cancel-mid-flow leaving
  storage unchanged.
- [ ] `just check`, `cargo test`, and `cargo fmt` clean.

## Blocked by

- 002-pure-deep-modules
