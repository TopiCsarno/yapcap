# Grok (Grok Build) Provider Support Design Document

**Date:** 2026-09-07  
**Status:** Approved  
**Target:** YapCap COSMIC Panel Applet  

---

## 1. Overview

This document specifies the architecture, data models, authentication flows, usage retrieval, and UI integration for supporting **Grok (Grok Build)** as a first-class provider in YapCap.

Grok Build is xAI's terminal-native coding agent. YapCap integrates with Grok via:
1. **Interactive In-App OAuth Browser Login**: Standard PKCE OAuth2 against `https://auth.x.ai` using the official Grok CLI public client ID.
2. **1-Click Host Import**: Detection of local `~/.grok/auth.json` with one-click import into YapCap-managed account storage.
3. **Usage Monitoring**: Querying the official Grok billing endpoint `https://cli-chat-proxy.grok.com/v1/billing?format=credits` to track the 7-day rolling/weekly credit quota, reset window, and plan tier (e.g. `SuperGrok`).
4. **Host Session Synchronization**: Inotify-backed monitoring of `~/.grok/auth.json` to award the **Active** badge to the account currently in use by the host CLI.

---

## 2. Architecture and Boundaries

### 2.1 Provider Registration
- Adds `ProviderId::Grok` to `src/model.rs` (expanding `ProviderId::ALL` to 10 entries).
- Label: `"Grok"`.
- Adds `grok_adapter` to `src/providers/adapters/` implementing `ProviderAdapter`.
- Registers `Grok` in `src/providers/adapters.rs` and `src/providers/registry.rs`.

### 2.2 Host Detection
- `src/detection.rs` checks for `~/.grok/` and `~/.grok/auth.json`.
- When present on startup and no Grok account is configured in YapCap, the popup displays Grok with a "Detected" chip prompting the user to add or import an account.

### 2.3 Configuration & State
- `src/config.rs`:
  - `Config.grok_enabled: bool` (defaults to true if detected or accounts exist).
  - `Config.grok_managed_accounts: Vec<ManagedGrokAccountConfig>`.
  - `ManagedGrokAccountConfig` stores:
    - `id: String`: Unique UUID string for YapCap storage directory.
    - `label: String`: User-facing account label (defaulting to email or user name).
    - `email: Option<String>`: Account email address.
    - `provider_account_id: Option<String>`: User UUID (`sub` claim).
    - `team_id: Option<String>`: Team identifier if part of an xAI team.
    - `plan: Option<String>`: Subscription plan (e.g. `"SuperGrok"`).
    - `created_at: DateTime<Utc>`.
    - `updated_at: DateTime<Utc>`.
  - Provider selection: `Config.selected_account_ids(ProviderId::Grok)` manages single-account active selection.

### 2.4 Account Storage Layout
Stored under `<state-root>/yapcap/grok-accounts/<id>/` via `account_storage`:
- Permissions: Directory `0o700`, files `0o600`.
- Files:
  - `metadata.json`: Serialized `ManagedGrokAccountMetadata` matching the fields above.
  - `tokens.json`:
    - `access_token: String`: JWT bearer token.
    - `refresh_token: Option<String>`: OAuth refresh token.
    - `token_type: String`: Usually `"bearer"`.
    - `expires_at: Option<DateTime<Utc>>`.
  - `snapshot.json`: Optional cached `UsageSnapshot` for fast startup restoration.

---

## 3. Authentication & Token Lifecycle

### 3.1 Public OAuth Client Credentials
- **Issuer**: `https://auth.x.ai`
- **Client ID**: `b1a00492-073a-47ea-816f-4c329264a828`
- **Authorization URL**: `https://auth.x.ai/oauth2/authorize`
- **Token URL**: `https://auth.x.ai/oauth2/token`
- **Scopes**: `openid profile email offline_access grok-cli:access api:access`

### 3.2 In-App PKCE Browser Login
1. When user triggers "Add account":
   - Spawns a local HTTP listener on `127.0.0.1:<ephemeral_port>`.
   - Generates random 32-byte PKCE `code_verifier`, derives `code_challenge = BASE64URL(SHA256(verifier))` with `code_challenge_method=S256`, and random CSRF `state`.
   - Opens the system browser with:
     `https://auth.x.ai/oauth2/authorize?response_type=code&client_id=b1a00492-073a-47ea-816f-4c329264a828&redirect_uri=http%3A%2F%2F127.0.0.1%3A<port>%2Fcallback&scope=openid%20profile%20email%20offline_access%20grok-cli%3Aaccess%20api%3Aaccess&code_challenge=<challenge>&code_challenge_method=S256&state=<state>`
   - UI provides "Waiting for sign-in in your browser...", an "Open Browser" button, and a "Cancel" button.
2. Callback Handling:
   - Receives redirect GET request at `/callback?code=<code>&state=<state>`.
   - Validates CSRF `state`.
   - Sends `POST https://auth.x.ai/oauth2/token` with URL-encoded parameters:
     - `grant_type=authorization_code`
     - `client_id=b1a00492-073a-47ea-816f-4c329264a828`
     - `code=<code>`
     - `redirect_uri=http://127.0.0.1:<port>/callback`
     - `code_verifier=<verifier>`
   - Parses response: `access_token`, `refresh_token`, `expires_in`.
   - Decodes JWT payload to extract `sub`, `email`, `name`, `team_id`, and `tier`.
   - Persists into YapCap account storage, updates config, selects the account, and triggers an immediate refresh.

### 3.3 1-Click Import from Host Grok CLI
- Reads `~/.grok/auth.json`.
- Key structure:
  - Top-level key: `https://auth.x.ai::<client_id>`.
  - Inner object: `key` (access token), `refresh_token`, `expires_at`, `email`, `user_id`, `first_name`, `last_name`, `team_id`, etc.
- When valid credentials exist:
  - Settings card offers "Import from Grok CLI" (or "Restore from Grok CLI").
  - Clicking copies tokens and metadata into YapCap's managed storage.
  - Dedupes by user ID / email: if an account with that identity already exists, it refreshes the stored tokens without creating duplicate account entries.
  - YapCap never writes to `~/.grok/auth.json`.

### 3.4 Token Refresh & Rotation
- Before calling the billing endpoint, YapCap checks if the access token will expire in under 5 minutes.
- If nearing expiry or upon receiving an HTTP 401 response:
  - Sends `POST https://auth.x.ai/oauth2/token`:
    - `grant_type=refresh_token`
    - `client_id=b1a00492-073a-47ea-816f-4c329264a828`
    - `refresh_token=<refresh_token>`
  - Updates `access_token`, new `refresh_token` (if rotated by xAI), and `expires_at` in `tokens.json`.
  - Retries the billing call if triggered by 401.
  - If refresh fails with 400/401 (invalid grant or revoked token), marks the account as `LoginRequired`.

---

## 4. Usage Fetching and Windows

### 4.1 Endpoint
- **URL**: `GET https://cli-chat-proxy.grok.com/v1/billing?format=credits`
- **Headers**:
  - `Authorization: Bearer <access_token>`
  - `Accept: application/json`

### 4.2 Response Schema
```json
{
  "config": {
    "creditUsagePercent": 46.0,
    "currentPeriod": {
      "type": "USAGE_PERIOD_TYPE_WEEKLY",
      "start": "2026-09-04T01:51:41.813312+00:00",
      "end": "2026-09-11T01:51:41.813312+00:00"
    },
    "onDemandCap": { "val": 0 },
    "onDemandUsed": { "val": 0 },
    "prepaidBalance": { "val": 0 },
    "productUsage": [
      {
        "product": "GrokBuild",
        "usagePercent": 46.0
      }
    ],
    "isUnifiedBillingUser": true,
    "billingPeriodStart": "2026-09-04T01:51:41.813312+00:00",
    "billingPeriodEnd": "2026-09-11T01:51:41.813312+00:00"
  },
  "subscriptionTier": "SuperGrok"
}
```

### 4.3 Snapshot Mapping
- **Provider**: `ProviderId::Grok`
- **Headline Window**:
  - Label: `"Weekly"`
  - Percentage: `config.creditUsagePercent` (or matching `"GrokBuild"` in `productUsage`), clamped to `0.0..=100.0`.
  - Reset Timestamp: `config.currentPeriod.end`.
  - Window Duration: 7 days (`604_800` seconds), feeding the pace engine (`ahead`, `on track`, `room`).
- **Identity**:
  - `email`: Stored account email.
  - `display_name`: User's full name if available.
  - `account_id`: `user_id` / `sub`.
  - `plan`: Parsed from `subscriptionTier` or profile claims (e.g. `"SuperGrok"`).
- **Extra Usage / Credits**:
  - If `onDemandCap.val > 0`, mapped to `ExtraUsageState::Active` with used and limit values.
  - If `prepaidBalance.val > 0`, mapped to `provider_cost`.

---

## 5. Host CLI Synchronization & Active Badge

- In `src/app/host_auth_watch.rs`, an inotify watch monitors `~/.grok/auth.json` (and `~/.grok/` directory creation).
- On file changes:
  - Parses `~/.grok/auth.json` without modifying it.
  - Extracts the active host `user_id` / `sub`.
  - Compares against YapCap's configured accounts.
  - Sets `system_active_account_id` to the matching account ID.
  - Popup renders the **Active** badge on the matching account row.

---

## 6. UI Assets & Localization

### 6.1 Icons
- Added to `resources/providers/`:
  - `grok.svg`: Monochrome dark/light vector icon of Grok logo.
  - `grok-reversed.svg`: Reversed contrast vector icon.
- Registered in `src/app/provider_assets.rs`.

### 6.2 Localization
Added to `i18n/en/yapcap.ftl`:
- `grok-accounts-title = Grok Accounts`
- `grok-account-reauth-tooltip = Re-authenticate this Grok account`
- `grok-login-running = Waiting for Grok sign-in in your browser...`
- `grok-login-failed = Grok login failed`
- `grok-window-weekly = Weekly`
- `import-from-grok = Import from Grok CLI`

---

## 7. Testing & Verification

1. **Unit Tests**:
   - `src/providers/grok/tests.rs`:
     - Deserialization of valid billing JSON fixtures.
     - Parsing `creditUsagePercent` and `productUsage`.
     - Window reset time and 7-day duration calculation.
     - Token refresh grant payload serialization and response handling.
     - Reading and parsing `~/.grok/auth.json` for import and active ID matching.
2. **Registry Tests**:
   - `src/providers/registry/tests.rs`:
     - Account discovery, selection, capabilities, facts, and account deletion for Grok.
3. **Login Flow Tests**:
   - `src/app/login/login_flow_tests/grok.rs`:
     - OAuth loopback state transitions, error handling, cancellation, and token exchange.
4. **Detection Tests**:
   - `src/detection.rs`:
     - Marker detection for `.grok`.
5. **Project Verification**:
   - Run `just check`, `cargo test`, `cargo fmt`.
   - Update `docs/spec.md` with section 3.10 Grok and summary updates.
