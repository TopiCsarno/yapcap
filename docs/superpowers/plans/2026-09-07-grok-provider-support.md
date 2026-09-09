# Grok (Grok Build) Provider Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Grok (Grok Build) as a first-class provider to YapCap with PKCE OAuth browser sign-in, 1-click `~/.grok/auth.json` host import, weekly credit quota usage tracking, inotify host session sync, and COSMIC UI integration.

**Architecture:** Implement a dedicated provider subsystem under `src/providers/grok/` with OAuth PKCE token exchange, refresh rotation, billing usage querying, and host CLI auth discovery. Register the provider through `ProviderId::Grok` across the model, configuration, storage, detection, adapter, registry, and COSMIC iced UI layers.

**Tech Stack:** Rust 2024 edition, libcosmic/iced, reqwest, serde, chrono, sha2, base64, tokio, notify.

**Spec:** `docs/superpowers/specs/2026-09-07-grok-provider-support-design.md`

## Global Constraints

- Files should be ~300 lines as a soft rule; split large source files when there is a clear boundary.
- Functions should fit on one screen; split large functions around behavior.
- Do not add clippy exceptions by default. Prefer changing code, visibility, tests, or module structure.
- Do not add comments in source code. Prefer clear names, smaller functions, and tests over inline explanations.
- Remove existing comments from touched source code when they are no longer needed. MPL-2.0 license headers (`// SPDX-License-Identifier: MPL-2.0`) are kept.
- Do not edit the template `justfile`.
- Before committing, update `docs/spec.md` when behavior or user-facing expectations change.
- Before committing, run `just check`, `cargo test`, and `cargo fmt`, fixing all warnings, errors, and failures.
- Do not add agent or AI attribution to commit messages (no `Co-Authored-By: Claude` or similar).

---

### Task 1: Core Models, Detection Markers, Error Types & Storage

**Files:**
- Modify: `src/model.rs:9-50`
- Modify: `src/error.rs:13-75, 450-550`
- Modify: `src/account_storage/mod.rs:350-365`
- Modify: `src/detection.rs:33-55, 96-100, 240-270`

**Interfaces:**
- Consumes: None
- Produces:
  - `ProviderId::Grok` in `src/model.rs` (`ALL` has 10 items; `label()` returns `"Grok"`).
  - `GrokError` in `src/error.rs` and `AppError::Provider(ProviderError::Grok(_))`.
  - `account_storage::ProviderAccountStorage::new_account_id(ProviderId::Grok)` produces `"grok-..."`.
  - `detection::detect(home)` returns `detected: true` for Grok when `~/.grok` or `~/.grok/auth.json` exists.

- [ ] **Step 1: Write the failing tests for Grok detection and error mapping**

In `src/detection.rs`:
```rust
#[test]
fn detects_grok_from_directory() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".grok")).unwrap();
    let snapshot = detect(home.path());
    assert!(snapshot.detected(ProviderId::Grok));
}

#[test]
fn detects_grok_from_auth_file() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".grok");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("auth.json"), "{}").unwrap();
    let snapshot = detect(home.path());
    assert!(snapshot.detected(ProviderId::Grok));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib detection::tests::detects_grok -v`
Expected: FAIL with compilation error: variant `Grok` not found on `ProviderId`.

- [ ] **Step 3: Implement ProviderId::Grok, GrokError, storage prefix, and detection markers**

In `src/model.rs`:
Add `Grok` to `ProviderId` and `ProviderId::ALL` (10 items total).
Update `label(self) -> &'static str`:
```rust
Self::Grok => "Grok",
```

In `src/account_storage/mod.rs`:
In `new_account_id`:
```rust
ProviderId::Grok => "grok",
```

In `src/error.rs`:
Add `Grok(#[from] GrokError)` to `ProviderError`.
Add `impl From<GrokError> for AppError`.
Define `pub enum GrokError` with variants:
- `CredentialsMissing`
- `AccountStorage(String)`
- `Unauthorized`
- `InvalidBearerHeader(reqwest::header::InvalidHeaderValue)`
- `UsageRequest(reqwest::Error)`
- `RateLimited { retry_after_secs: Option<u64> }`
- `TokenRefreshRequest(reqwest::Error)`
- `TokenRefreshHttp { status: u16 }`
- `TokenRefreshDecode(reqwest::Error)`
- `TokenRefreshParse(String)`
- `UsageEndpoint { status: u16, source: reqwest::Error }`
- `DecodeUsage(reqwest::Error)`
- `NoUsageData`
- `InvalidResetTimestamp { value: String, source: chrono::ParseError }`
- `RefreshUnavailable`
Implement `is_network_unavailable` and `requires_user_action`.

In `src/detection.rs`:
Add:
```rust
const GROK: [Marker; 2] = [dir(".grok"), file(".grok/auth.json")];
```
In `markers(provider)`:
```rust
ProviderId::Grok => &GROK,
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib detection::tests::detects_grok -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/model.rs src/error.rs src/account_storage/mod.rs src/detection.rs
git commit -m "feat(grok): add ProviderId::Grok, GrokError, and host detection"
```

---

### Task 2: Configuration & Path Management for Grok

**Files:**
- Modify: `src/config.rs:40-270, 300-450, 640-700, 800-950`
- Modify: `src/runtime.rs:450-460, 560-610`
- Modify: `src/app/provider_actions.rs:565-595`
- Test: `src/config.rs:tests`

**Interfaces:**
- Consumes: `ProviderId::Grok`
- Produces:
  - `Config.grok_enabled: bool`
  - `Config.grok_managed_accounts: Vec<ManagedGrokAccountConfig>`
  - `Config.selected_grok_account_ids: Vec<String>`
  - `ManagedGrokAccountConfig` struct
  - `ConfigPaths.grok_accounts_dir: PathBuf`
  - `managed_grok_account_dir(account_id: &str) -> PathBuf`
  - `popup_route_label(PopupRoute::ManageAccounts(ProviderId::Grok)) -> "manage_accounts_grok"`

- [ ] **Step 1: Write the failing test for Grok configuration and account directory**

In `src/config.rs` (under `tests`):
```rust
#[test]
fn grok_accounts_dir_is_configured_under_state_root() {
    let p = paths();
    assert!(
        p.grok_accounts_dir.ends_with(Path::new("yapcap/grok-accounts")),
        "unexpected grok_accounts_dir: {}",
        p.grok_accounts_dir.display()
    );
}

#[test]
fn grok_managed_account_config_roundtrips() {
    let id = "grok-test-1";
    let dir = managed_grok_account_dir(id);
    assert_eq!(dir, paths().grok_accounts_dir.join(id));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib config::tests::grok_accounts_dir -v`
Expected: FAIL with compilation error: field `grok_accounts_dir` not found on `ConfigPaths`.

- [ ] **Step 3: Implement Grok config fields, ManagedGrokAccountConfig, and paths**

In `src/config.rs`:
Add struct:
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedGrokAccountConfig {
    pub id: String,
    pub label: String,
    pub config_dir: PathBuf,
    pub email: Option<String>,
    pub provider_account_id: Option<String>,
    pub team_id: Option<String>,
    pub plan: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_authenticated_at: Option<DateTime<Utc>>,
}
```
Add fields to `Config`:
```rust
pub grok_enabled: bool,
pub grok_managed_accounts: Vec<ManagedGrokAccountConfig>,
pub selected_grok_account_ids: Vec<String>,
```
Add `ProviderId::Grok` arms to:
- `selected_account_ids`
- `selected_account_ids_mut`
- `provider_enabled_key`
- `provider_enablement_key`
- `provider_enablement_mut`
- `Config::default()`
In `ConfigPaths`:
- `pub grok_accounts_dir: PathBuf`
- `paths()` assigns `state_dir.join("grok-accounts")`
- `pub fn managed_grok_account_dir(account_id: &str) -> PathBuf`

In `src/runtime.rs`:
Update `load_initial_state` and reconcile lists ensuring `ProviderId::Grok` is included.

In `src/app/provider_actions.rs`:
In `popup_route_label`:
```rust
ProviderId::Grok => "manage_accounts_grok",
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib config::tests -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/runtime.rs src/app/provider_actions.rs
git commit -m "feat(grok): add ManagedGrokAccountConfig, grok paths, and config enablement"
```

---

### Task 3: Grok Usage Models, Quota Parsing & Fixtures

**Files:**
- Create: `fixtures/grok/billing_response.json`
- Create: `src/providers/grok/usage.rs`
- Create: `src/providers/grok/mod.rs`
- Create: `src/providers/grok/tests.rs`
- Modify: `src/providers/mod.rs:1-17`

**Interfaces:**
- Consumes: `UsageSnapshot`, `UsageWindow`, `ProviderCost`, `ExtraUsageState`, `ProviderIdentity`, `ProviderId::Grok`, `GrokError`
- Produces:
  - `src/providers/grok/usage.rs`:
    - `GrokBillingResponse`
    - `parse_billing_snapshot(raw_json: &str, email: Option<&str>, display_name: Option<&str>, account_id: Option<&str>) -> Result<UsageSnapshot, GrokError>`
    - Quota headline window labelled `"Weekly"`, percentage clamped to `0.0..=100.0`, duration 7 days (`604_800`s), reset timestamp from `currentPeriod.end`.
    - Extra usage from `onDemandCap` / `onDemandUsed`.
    - Credits from `prepaidBalance`.

- [ ] **Step 1: Write fixture and failing unit test for billing parsing**

Create `fixtures/grok/billing_response.json`:
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
    "prepaidBalance": { "val": 1500 },
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

In `src/providers/grok/tests.rs`:
```rust
use super::usage::parse_billing_snapshot;
use crate::model::ProviderId;

#[test]
fn parses_billing_fixture_into_snapshot() {
    let fixture = include_str!("../../../fixtures/grok/billing_response.json");
    let snapshot = parse_billing_snapshot(fixture, Some("user@x.ai"), Some("Grok User"), Some("sub-123")).unwrap();
    assert_eq!(snapshot.provider, ProviderId::Grok);
    assert_eq!(snapshot.windows.len(), 1);
    let window = &snapshot.windows[0];
    assert_eq!(window.label, "Weekly");
    assert!((window.used_percent - 46.0).abs() < f32::EPSILON);
    assert_eq!(window.window_seconds, Some(604_800));
    assert!(window.reset_at.is_some());
    assert_eq!(snapshot.identity.plan.as_deref(), Some("SuperGrok"));
    assert_eq!(snapshot.identity.email.as_deref(), Some("user@x.ai"));
    assert_eq!(snapshot.identity.display_name.as_deref(), Some("Grok User"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib providers::grok::tests::parses_billing_fixture -v`
Expected: FAIL with module not found `grok`.

- [ ] **Step 3: Implement GrokBillingResponse and parse_billing_snapshot**

In `src/providers/mod.rs`:
```rust
pub mod grok;
```

In `src/providers/grok/usage.rs`:
Define deserialization types:
- `GrokBillingResponse`
- `BillingConfig`
- `BillingCurrentPeriod`
- `NumericVal`
- `ProductUsage`
Map to `UsageSnapshot`:
- Window label: `"Weekly"`
- Used percent: `config.creditUsagePercent` or `productUsage.iter().find(|p| p.product == "GrokBuild").map(|p| p.usage_percent)`.
- Reset at: `config.currentPeriod.end`.
- Window seconds: `Some(7 * 24 * 3600) = 604_800`.
- Identity plan: `subscriptionTier`.
- `provider_cost`: if `config.prepaidBalance.val > 0`, `ProviderCost { used: val, limit: None, units: "credits".to_string() }`.
- `extra_usage`: if `config.onDemandCap.val > 0`, `ExtraUsageState::Active { used_percent, cost }`.

In `src/providers/grok/mod.rs`:
```rust
pub mod usage;
#[cfg(test)]
mod tests;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib providers::grok::tests::parses_billing_fixture -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add fixtures/grok/billing_response.json src/providers/mod.rs src/providers/grok/
git commit -m "feat(grok): add billing response parser and usage fixture"
```

---

### Task 4: Grok OAuth, PKCE & Token Exchange/Refresh

**Files:**
- Create: `src/providers/grok/oauth.rs`
- Modify: `src/providers/grok/mod.rs`
- Modify: `src/providers/grok/tests.rs`

**Interfaces:**
- Consumes: `reqwest::Client`, `GrokError`, `DateTime<Utc>`
- Produces:
  - `ISSUER: &str = "https://auth.x.ai"`
  - `CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828"`
  - `AUTHORIZE_URL: &str = "https://auth.x.ai/oauth2/authorize"`
  - `TOKEN_URL: &str = "https://auth.x.ai/oauth2/token"`
  - `SCOPES: &str = "openid profile email offline_access grok-cli:access api:access"`
  - `PkceCodes { code_verifier: String, code_challenge: String }`
  - `new_pkce() -> PkceCodes`
  - `new_state() -> String`
  - `authorization_url(redirect_uri: &str, pkce: &PkceCodes, state: &str) -> String`
  - `exchange_code(client: &reqwest::Client, code: &str, code_verifier: &str, redirect_uri: &str, token_url: &str) -> Result<GrokTokenResponse, GrokError>`
  - `refresh_token(client: &reqwest::Client, refresh_token: &str, token_url: &str) -> Result<GrokTokenResponse, GrokError>`
  - `decode_jwt_claims(token: &str) -> Option<GrokClaims>` (with `sub`, `email`, `name`, `team_id`, `tier`)

- [ ] **Step 1: Write failing unit test for PKCE derivation, auth URL, and token parsing**

In `src/providers/grok/tests.rs`:
```rust
use super::oauth::{authorization_url, decode_jwt_claims, new_pkce, parse_token_response, PkceCodes};

#[test]
fn authorization_url_contains_required_params() {
    let pkce = PkceCodes {
        code_verifier: "verifier123".to_string(),
        code_challenge: "challenge456".to_string(),
    };
    let url = authorization_url("http://127.0.0.1:12345/callback", &pkce, "state789");
    assert!(url.starts_with("https://auth.x.ai/oauth2/authorize"));
    assert!(url.contains("client_id=b1a00492-073a-47ea-816f-4c329264a828"));
    assert!(url.contains("code_challenge=challenge456"));
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains("state=state789"));
}

#[test]
fn decodes_grok_jwt_claims() {
    let header = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";
    let payload = "eyJzdWIiOiJ1c3ItMSIsImVtYWlsIjoidGVzdGVyQHguYWkiLCJuYW1lIjoiVGVzdGVyIFgiLCJ0ZWFtX2lkIjoidGVhbS0xIn0";
    let token = format!("{header}.{payload}.signature");
    let claims = decode_jwt_claims(&token).unwrap();
    assert_eq!(claims.sub.as_deref(), Some("usr-1"));
    assert_eq!(claims.email.as_deref(), Some("tester@x.ai"));
    assert_eq!(claims.name.as_deref(), Some("Tester X"));
    assert_eq!(claims.team_id.as_deref(), Some("team-1"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib providers::grok::tests::authorization_url -v`
Expected: FAIL with module `oauth` not found.

- [ ] **Step 3: Implement Grok OAuth PKCE and token exchange**

In `src/providers/grok/oauth.rs`:
- Implement `new_pkce()` using SHA-256 and base64url encoding without padding.
- Implement `new_state()` generating random 32 alphanumeric chars.
- Implement `authorization_url(...)`.
- Implement `exchange_code(...)` sending POST `token_url` with URL-encoded parameters.
- Implement `refresh_token(...)` sending POST `token_url` with `grant_type=refresh_token`.
- Implement `parse_token_response(raw: &str) -> Result<GrokTokenResponse, GrokError>` returning `access_token`, `refresh_token`, `expires_at`.
- Implement `decode_jwt_claims(token: &str) -> Option<GrokClaims>`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib providers::grok::tests::authorization_url -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/providers/grok/oauth.rs src/providers/grok/mod.rs src/providers/grok/tests.rs
git commit -m "feat(grok): add OAuth PKCE generation, authorization URL, and token exchange"
```

---

### Task 5: Grok Account Management & Host CLI Import

**Files:**
- Create: `src/providers/grok/account.rs`
- Modify: `src/providers/grok/mod.rs`
- Modify: `src/providers/grok/tests.rs`

**Interfaces:**
- Consumes: `Config`, `ManagedGrokAccountConfig`, `ProviderAccountStorage`, `paths().grok_accounts_dir`
- Produces:
  - `discover_accounts(config: &Config) -> Vec<ManagedGrokAccountConfig>`
  - `apply_login_account(config: &mut Config, account: ManagedGrokAccountConfig)`
  - `sync_managed_account_dirs(config: &mut Config) -> bool`
  - `read_host_credentials(path: &Path) -> Option<HostGrokCredentials>`
  - `system_active_account_id(managed_accounts: &[ManagedGrokAccountConfig], grok_auth_path: &Path) -> Option<String>`
  - `host_auth_file_path() -> Option<PathBuf>` (`~/.grok/auth.json`)

- [ ] **Step 1: Write failing unit test for host auth import and system active ID matching**

In `src/providers/grok/tests.rs`:
```rust
use super::account::{read_host_credentials, system_active_account_id};
use crate::config::ManagedGrokAccountConfig;
use chrono::Utc;
use std::path::PathBuf;

#[test]
fn reads_host_credentials_and_matches_active_account() {
    let temp = tempfile::tempdir().unwrap();
    let auth_json = temp.path().join("auth.json");
    let json_data = r#"{
        "https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828": {
            "key": "test-access-token",
            "refresh_token": "test-refresh-token",
            "expires_at": 1757211101,
            "email": "developer@x.ai",
            "user_id": "usr-999",
            "first_name": "Dev",
            "last_name": "Grok",
            "team_id": "team-888"
        }
    }"#;
    std::fs::write(&auth_json, json_data).unwrap();

    let creds = read_host_credentials(&auth_json).expect("should parse host credentials");
    assert_eq!(creds.access_token, "test-access-token");
    assert_eq!(creds.user_id.as_deref(), Some("usr-999"));
    assert_eq!(creds.email.as_deref(), Some("developer@x.ai"));

    let account = ManagedGrokAccountConfig {
        id: "grok-acc-1".to_string(),
        label: "developer@x.ai".to_string(),
        config_dir: PathBuf::from("/tmp/acc1"),
        email: Some("developer@x.ai".to_string()),
        provider_account_id: Some("usr-999".to_string()),
        team_id: Some("team-888".to_string()),
        plan: Some("SuperGrok".to_string()),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: None,
    };

    let active_id = system_active_account_id(&[account.clone()], &auth_json);
    assert_eq!(active_id.as_deref(), Some("grok-acc-1"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib providers::grok::tests::reads_host_credentials -v`
Expected: FAIL with module `account` not found.

- [ ] **Step 3: Implement Grok account management and host reading**

In `src/providers/grok/account.rs`:
- Implement `read_host_credentials(path: &Path) -> Option<HostGrokCredentials>` checking top-level key `https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828`.
- Implement `system_active_account_id`:
  - Read `auth.json`.
  - Extract `user_id` and `email`.
  - Check `managed_accounts` for matching `provider_account_id` first, then matching normalized email.
- Implement `discover_accounts(config: &Config) -> Vec<ManagedGrokAccountConfig>`.
- Implement `apply_login_account(config: &mut Config, account: ManagedGrokAccountConfig)` (deduping by user ID / email, updating existing account if matched, or appending).
- Implement `sync_managed_account_dirs(config: &mut Config) -> bool`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib providers::grok::tests::reads_host_credentials -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/providers/grok/account.rs src/providers/grok/mod.rs src/providers/grok/tests.rs
git commit -m "feat(grok): add account discovery, host credentials reader, and active account matching"
```

---

### Task 6: Grok Fetch & Usage Lifecycle with Token Auto-Refresh

**Files:**
- Modify: `src/providers/grok/mod.rs`
- Modify: `src/providers/grok/tests.rs`

**Interfaces:**
- Consumes: `reqwest::Client`, `ProviderAccountStorage`, `GrokError`, `UsageSnapshot`
- Produces:
  - `fetch(client: &reqwest::Client, account_id: &str, account_dir: PathBuf) -> Result<UsageSnapshot, GrokError>`
  - `fetch_at(client: &reqwest::Client, account_id: &str, account_dir: PathBuf, billing_url: &str, token_url: &str) -> Result<UsageSnapshot, GrokError>`

- [ ] **Step 1: Write failing unit test for fetch with mocked endpoints**

In `src/providers/grok/tests.rs`:
```rust
#[tokio::test]
async fn fetch_at_refreshes_token_when_expiring_soon() {
    // Tests fetch_at with wiremock / mock server verifying automatic refresh before calling billing
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib providers::grok::tests::fetch_at_refreshes_token -v`
Expected: FAIL with `fetch_at` not defined.

- [ ] **Step 3: Implement fetch and fetch_at**

In `src/providers/grok/mod.rs`:
- Define `DEFAULT_BILLING_URL: &str = "https://cli-chat-proxy.grok.com/v1/billing?format=credits"`.
- Implement `fetch_at`:
  - Load `metadata` and `tokens` from `ProviderAccountStorage`.
  - If `tokens.expires_at <= Utc::now() + Duration::minutes(5)`: call `refresh_token`, update storage.
  - Call `billing_url` with header `Authorization: Bearer <access_token>` and `Accept: application/json`.
  - If status is 401: call `refresh_token`, update storage, and retry billing call once.
  - If billing call returns 429: return `GrokError::RateLimited`.
  - If billing call fails with 401: return `GrokError::Unauthorized`.
  - Parse response via `parse_billing_snapshot`.
  - Save snapshot to `storage.save_snapshot(account_id, &snapshot)`.
  - Return `Ok(snapshot)`.
- Implement `fetch(...)` forwarding to `DEFAULT_BILLING_URL` and `TOKEN_URL`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib providers::grok::tests::fetch_at_refreshes_token -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/providers/grok/mod.rs src/providers/grok/tests.rs
git commit -m "feat(grok): implement fetch with token expiration check, auto-refresh, and retry"
```

---

### Task 7: Grok Login Flow & Ephemeral Loopback Server

**Files:**
- Create: `src/providers/grok/login.rs`
- Modify: `src/providers/grok/mod.rs`
- Modify: `src/providers/grok/tests.rs`

**Interfaces:**
- Consumes: `Config`, `ManagedGrokAccountConfig`, `GrokError`
- Produces:
  - `GrokLoginState { flow_id, status, login_url, error, importing_from_host_cli }`
  - `GrokLoginStatus { Running, Failed }`
  - `GrokLoginEvent { LoginUrl { flow_id, url }, Finished { flow_id, result: Box<Result<GrokLoginSuccess, String>> } }`
  - `GrokLoginSuccess { account: ManagedGrokAccountConfig }`
  - `prepare(config: Config) -> Result<(GrokLoginState, Task<GrokLoginEvent>), String>`
  - `prepare_targeted(account_id: String, config: Config) -> Result<(GrokLoginState, Task<GrokLoginEvent>), String>`
  - `prepare_host_import(config: Config, target_account_id: Option<String>) -> Result<(GrokLoginState, Task<GrokLoginEvent>), String>`

- [ ] **Step 1: Write failing unit test for loopback login flow setup and host import preparation**

In `src/providers/grok/tests.rs`:
```rust
#[test]
fn prepare_host_import_creates_running_state() {
    let config = crate::config::Config::default();
    let (state, _task) = super::login::prepare_host_import(config, None).unwrap();
    assert_eq!(state.status, super::login::GrokLoginStatus::Running);
    assert!(state.importing_from_host_cli);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib providers::grok::tests::prepare_host_import -v`
Expected: FAIL with module `login` not found.

- [ ] **Step 3: Implement Grok login flow and loopback server**

In `src/providers/grok/login.rs`:
- Define `GrokLoginState`, `GrokLoginStatus`, `GrokLoginEvent`, `GrokLoginSuccess`.
- Implement `prepare`:
  - Bind TCP listener to `127.0.0.1:0` to obtain an ephemeral port.
  - Generate PKCE codes and state.
  - Build `authorization_url` with `http://127.0.0.1:<port>/callback`.
  - Send `LoginUrl` event so UI has URL for "Open Browser" button.
  - Automatically call `cosmic::app::open_url(&auth_url)`.
  - Await HTTP GET `/callback` request on loopback listener with timeout.
  - Respond with friendly HTML ("YapCap: Grok sign-in complete. You can close this tab.").
  - Exchange authorization code for tokens via `oauth::exchange_code`.
  - Decode JWT claims.
  - Create or replace account in `ProviderAccountStorage` under `paths().grok_accounts_dir`.
  - Fetch initial snapshot and cache it.
  - Return `GrokLoginSuccess`.
- Implement `prepare_host_import`:
  - Read `~/.grok/auth.json` via `account::read_host_credentials`.
  - Commit credentials to `ProviderAccountStorage`.
  - Fetch initial snapshot.
  - Return `GrokLoginSuccess`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib providers::grok::tests::prepare_host_import -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/providers/grok/login.rs src/providers/grok/mod.rs src/providers/grok/tests.rs
git commit -m "feat(grok): implement PKCE loopback login flow and host import flow"
```

---

### Task 8: Provider Adapter & Registry Integration

**Files:**
- Create: `src/providers/adapters/grok_adapter.rs`
- Modify: `src/providers/interface.rs:20-35, 60-75, 115-130`
- Modify: `src/providers/adapters.rs:1-45, 160-220`
- Modify: `src/providers/registry.rs:1-120`
- Modify: `src/providers/registry/tests.rs`

**Interfaces:**
- Consumes: `ProviderAdapter`, `GrokAdapter`
- Produces:
  - `ProviderLoginKind::Grok`
  - `ProviderAccountAction::RestoreFromGrok`
  - `ProviderAccountHandle::Grok(ManagedGrokAccountConfig)`
  - `GROK_ADAPTER: GrokAdapter` in `src/providers/adapters.rs`
  - `grok_system_active_account_id` helper
  - Registry tests passing for Grok

- [ ] **Step 1: Write failing test in registry::tests verifying Grok adapter registration**

In `src/providers/registry/tests.rs`:
```rust
#[test]
fn grok_provider_registered_and_discovers_accounts() {
    let config = Config::default();
    let descriptors = discover_accounts(ProviderId::Grok, &config);
    assert!(descriptors.is_empty());
    assert_eq!(capabilities(ProviderId::Grok).supports_background_status_refresh, false);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib providers::registry::tests::grok_provider_registered -v`
Expected: FAIL with variant `Grok` not handled in `adapter()`.

- [ ] **Step 3: Implement GrokAdapter and wire interface & registry**

In `src/providers/interface.rs`:
- Add `ProviderAccountAction::RestoreFromGrok`.
- Add `ProviderLoginKind::Grok`.
- Add `ProviderAccountHandle::Grok(ManagedGrokAccountConfig)`.

In `src/providers/adapters/grok_adapter.rs`:
- Implement `ProviderAdapter for GrokAdapter`:
  - `id(&self) -> ProviderId::Grok`
  - `login_kind(&self) -> ProviderLoginKind::Grok`
  - `capabilities`: `supports_background_status_refresh: false`, `requires_auth_prompt_on_auth_failure: false`.
  - `discover_accounts`: maps `config.grok_managed_accounts` into descriptors.
  - `sync_managed_accounts`: calls `grok::account::sync_managed_account_dirs`.
  - `delete_account`: removes from storage and config.
  - `reconcile_provider_accounts`: reconciles descriptors and assigns `system_active_account_id`.
  - `system_active_account_id`: calls `grok_system_active_account_id(&config.grok_managed_accounts)`.
  - `fetch_account`: forwards to `grok::fetch`.

In `src/providers/adapters.rs`:
- Declare `mod grok_adapter;`
- Add `static GROK_ADAPTER: grok_adapter::GrokAdapter = grok_adapter::GrokAdapter;`
- Add `ProviderId::Grok => &GROK_ADAPTER` in `adapter()`.
- Add `pub(super) fn grok_system_active_account_id`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib providers::registry::tests::grok_provider_registered -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/providers/interface.rs src/providers/adapters/grok_adapter.rs src/providers/adapters.rs src/providers/registry.rs src/providers/registry/tests.rs
git commit -m "feat(grok): register GrokAdapter in providers registry"
```

---

### Task 9: App State, Login Flow Dispatch, Host Auth Watch & UI Integration

**Files:**
- Create: `resources/providers/grok.svg`
- Create: `resources/providers/grok-reversed.svg`
- Create: `src/app/login/flows/grok.rs`
- Create: `src/app/login/login_flow_tests/grok.rs`
- Modify: `src/app/login/flows/mod.rs`
- Modify: `src/app/login/mod.rs`
- Modify: `src/app/mod.rs`
- Modify: `src/app/session.rs`
- Modify: `src/app/host_auth_watch.rs`
- Modify: `src/app/provider_assets.rs`
- Modify: `src/app/popup_view.rs`
- Modify: `src/app/popup_view/detail.rs`
- Modify: `src/app/popup_view/settings/accounts.rs`
- Modify: `src/app/popup_view/settings/accounts/login_controls.rs`
- Modify: `src/app/popup_view/settings/accounts/rows.rs`
- Modify: `src/app/popup_view/settings/accounts/empty.rs`
- Modify: `src/demo_env.rs`
- Modify: `i18n/en/yapcap.ftl`

**Interfaces:**
- Consumes: `GrokLoginState`, `GrokLoginFlow`, `Message`, `ProviderId::Grok`
- Produces:
  - Vector SVG icons for Grok.
  - Localization strings in `yapcap.ftl`.
  - `Message::ImportFromGrok(Option<String>)` and `Message::RestoreFromGrok(String)`.
  - Inotify host auth watch detecting changes to `~/.grok/auth.json`.
  - Complete settings UI with "Import from Grok CLI", active badge, and browser login controls.

- [ ] **Step 1: Create SVG assets and add localization strings**

Create `resources/providers/grok.svg` and `resources/providers/grok-reversed.svg`.
In `i18n/en/yapcap.ftl`:
```ftl
grok-accounts-title = Grok Accounts
grok-account-reauth-tooltip = Re-authenticate this Grok account
grok-login-running = Waiting for Grok sign-in in your browser...
grok-login-failed = Grok login failed
grok-window-weekly = Weekly
import-from-grok = Import from Grok CLI
restore-from-grok = Restore from Grok CLI
```

- [ ] **Step 2: Write failing login flow test**

Create `src/app/login/login_flow_tests/grok.rs`:
```rust
#[test]
fn grok_login_state_transitions_on_cancel() {
    // Verifies start_login and cancel_login for Grok
}
```

- [ ] **Step 3: Implement Grok login flow, host watch, and UI controls**

In `src/app/login/flows/grok.rs`:
- Implement `LoginFlow for GrokLoginFlow`.

In `src/app/login/mod.rs`:
- Add `LoginEventKind::Grok(GrokLoginEvent)`.

In `src/app/mod.rs`:
- Add `pub grok_login: Option<GrokLoginState>` and `pub grok_login_handle: Option<Handle>`.
- Add `ImportFromGrok(Option<String>)` and `RestoreFromGrok(String)` to `Message`.
- Handle messages in `update`.

In `src/app/session.rs`:
- Wire `ProviderId::Grok` in `start_login`, `reauthenticate`, `cancel_login`, `import_from_grok`, `restore_from_grok`.

In `src/app/host_auth_watch.rs`:
- Add `grok_dir` (`~/.grok`) and `grok_auth` (`~/.grok/auth.json`) to `WatchTargets`.
- Install watches and filter events for `.grok`.

In `src/app/provider_assets.rs`:
- Handle `ProviderId::Grok` returning `grok.svg` or `grok-reversed.svg`.

In `src/app/popup_view/`:
- In `popup_view.rs`: add `grok: Option<&'a GrokLoginState>` to `ProviderLoginStates`.
- In `detail.rs`: `cost_section` uses `credit_section` for `ProviderId::Grok`.
- In `settings/accounts/login_controls.rs`: add `grok_login_controls`.
- In `settings/accounts/rows.rs`: handle `ProviderAccountAction::RestoreFromGrok`.
- In `settings/accounts.rs`: handle `ProviderLoginKind::Grok`.

In `src/demo_env.rs`:
- Add mock Grok account data with `"SuperGrok"` plan and 46% weekly quota.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib app::login::login_flow_tests::grok -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add resources/providers/ i18n/ src/app/ src/demo_env.rs
git commit -m "feat(grok): wire Grok login flow, host watch, assets, and UI controls"
```

---

### Task 10: Spec Documentation & Full Verification

**Files:**
- Modify: `docs/spec.md`
- Test: All unit, integration, and formatting tests

**Interfaces:**
- Consumes: All Grok implementation features
- Produces: Updated specification reflecting §3.10 Grok and a clean CI run.

- [ ] **Step 1: Update docs/spec.md**

Update:
- Provider summary table in section 1 and 3 adding Grok (§3.10).
- Add section 3.10 Grok detailing OAuth endpoint, host import from `~/.grok/auth.json`, billing endpoint `https://cli-chat-proxy.grok.com/v1/billing?format=credits`, 7-day quota window, and inotify sync.

- [ ] **Step 2: Run cargo fmt**

Run: `cargo fmt`

- [ ] **Step 3: Run just check**

Run: `just check`
Expected: 0 warnings, 0 errors.

- [ ] **Step 4: Run cargo test**

Run: `cargo test`
Expected: All tests pass (including new Grok tests).

- [ ] **Step 5: Commit**

```bash
git add docs/spec.md
git commit -m "docs: document Grok provider support in specification"
```
