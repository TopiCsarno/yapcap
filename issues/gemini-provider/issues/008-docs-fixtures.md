---
status: done
type: AFK
blocked_by:
  - 007-error-states-reauth
---

# Docs and fixtures redaction

## Parent

[PRD](../PRD.md)

## What to build

Bring the documentation in line with the shipped feature and make the
captured Gemini fixtures safe to commit.

`docs/spec.md` updates:

- Add a new §3.4 Gemini section parallel to §3.1 (Codex), §3.2 (Claude),
  §3.3 (Cursor). Cover: account model (managed accounts under
  `<state-root>/yapcap/gemini-accounts/<id>/`, dedupe by normalized
  email, OAuth-only scope), native add-account flow (loopback
  callback, PKCE, scopes, browser open, post-success `loadCodeAssist`
  call), usage fetch (`loadCodeAssist` + `retrieveUserQuota` per refresh
  cycle, no caching), bucket family classification rules (Pro/Flash/Lite
  with substring + `-flash-lite` exclusion, lowest-remaining
  aggregation, hide rules, free-tier Pro force-hide), tier-and-`hd` plan
  label mapping, host session hint via
  `~/.gemini/google_accounts.json`, error classification including
  `cloudresourcemanager` fallback, and the OAuth client credential
  hardcoding rationale.
- Update §1.2 Supported Sources table to include Gemini.
- Update the document metadata Providers row from "Codex, Claude Code,
  Cursor" to include Gemini.
- Update §4.x Auth-and-Config to note Gemini OAuth credential file
  location is YapCap-owned only (no host `~/.gemini/oauth_creds.json`
  read for tokens), and that `~/.gemini/google_accounts.json` is the
  host session hint analog to `~/.claude.json`.
- Update §8 Packaging to confirm Gemini introduces no new Flatpak
  permissions; reuses existing network, portal-OpenURI for browser
  launch, and `--filesystem=home:ro` for the session hint.
- Update §10 Testing to include Gemini provider normalizer tests,
  fixtures path, and any new error-classification tests.

`docs/qa.md` updates:

- Add a new Gemini section covering: fresh install with no Gemini
  account shows Login required state; Add account flow on Native and
  Flatpak; multi-account dedupe by email; tier transitions (free ↔
  paid) update bars and plan badge on next refresh; Active badge
  follows `gemini auth login` switches; re-auth on revoked refresh
  token works; remove-account deletes only YapCap-owned state;
  pre-existing host CLI configurations (API key, Vertex) don't
  interfere with YapCap's OAuth flow (no Active badge is the expected
  behavior, not a bug); `cloudresourcemanager` fallback exercised when
  no `cloudaicompanionProject` is returned.

README update:

- Add a short note that YapCap meters Gemini accounts authenticated via
  OAuth only. API key (`selectedAuthType: gemini-api-key`) and Vertex AI
  (`selectedAuthType: vertex-ai`) are not supported and would require
  the user to switch to OAuth (`gemini auth login`) to use YapCap. Note
  also that only one project's quota is shown per Gemini account; users
  with multiple paid GCP projects see whichever project Google's
  `loadCodeAssist` returns.

Fixtures redaction:

- Walk `fixtures/gemini/oauth_token_response.json`,
  `load_code_assist_response.json`, and
  `retrieve_user_quota_response.json` and redact: `access_token`,
  `id_token`, `refresh_token`, email addresses, the `picture` URL,
  the `cloudaicompanionProject` slug, the `upgradeSubscriptionUri`
  (carries the user's email), and any other PII. Replace each with a
  fixed placeholder (e.g. `"<redacted>"`) that preserves the field type
  so the deserializers still work in tests.
- Capture error-path fixtures by extending `fixtures/gemini/probe.py`
  with simulation flags: `--simulate-bad-refresh` (POSTs an invalid
  refresh_token to capture the 4xx body), and optionally a way to
  capture a 429 if Google rate-limits the probe naturally during
  testing. Save these as `oauth_token_400_response.json`,
  `oauth_token_429_response.json`, and similar names alongside the
  existing captures.

## Acceptance criteria

- [ ] `docs/spec.md` includes §3.4 Gemini with the content described
  above, and §1.2, §4.x, §8, §10, and document metadata are updated for
  four providers.
- [ ] `docs/qa.md` includes a new Gemini section covering all the
  scenarios listed.
- [ ] README has the OAuth-only and single-project-per-account
  limitation notes.
- [ ] All three captured Gemini fixtures are redacted for safe commit;
  the redacted files still deserialize successfully into the types used
  by the slice 2 modules' tests.
- [ ] Error-path fixtures captured via extended probe; committed
  redacted.
- [ ] `just check`, `cargo test`, and `cargo fmt` clean.

## Blocked by

- 007-error-states-reauth

## Completion note

- `docs/spec.md`: added §3.4 Gemini (account model, OAuth login flow, usage
  fetch pipeline, `cloudresourcemanager` fallback, bucket classification,
  plan-label table, host session hint, re-auth flow, error taxonomy, hardcoded
  OAuth client rationale); updated §1.2 Supported Sources table, doc metadata
  providers row, doc map, §4.1 OAuth Credential Files, §8 Packaging Gemini
  note, and §10 Testing fixture/test list to include Gemini.
- `docs/qa.md`: added §8a Gemini covering fresh install, add account
  (Native+Flatpak), multi-account dedupe, usage display per tier, tier
  transitions, Active badge via `google_accounts.json`, token refresh,
  per-account re-auth (same-email guard), account removal,
  non-interfering host CLI configs (API key / Vertex), and
  `cloudresourcemanager` fallback.
- `README.md`: bumped provider count to four, added Gemini highlight, added
  OAuth-only and single-project-per-account limitation notes.
- `fixtures/gemini/probe.py`: added `--simulate-bad-refresh` flag that posts a
  bogus refresh_token to the OAuth endpoint and saves the 4xx body as
  `oauth_token_400_response.json`; documented the optional
  `oauth_token_429_response.json` slot.
- `fixtures/gemini/oauth_token_400_response.json`: committed a redacted
  `invalid_grant` envelope so error-path tests have a reference shape without
  needing live network access.
- Existing slice-2 fixtures already used `<redacted>` placeholders for tokens
  and synthetic identity values (`user@example.com`, `sub=1234567890`,
  `cloudaicompanionProject=example-project`); verified they still deserialize
  into the typed modules and tests pass.
