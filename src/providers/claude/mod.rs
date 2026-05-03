// SPDX-License-Identifier: MPL-2.0

mod account;
mod login;
mod oauth;

use crate::account_storage::{ProviderAccountMetadata, ProviderAccountStorage};
use crate::auth::ClaudeAuth;
use crate::error::{ClaudeError, Result};
use crate::model::{
    ExtraUsageState, ProviderId, ProviderIdentity, UsageHeadline, UsageSnapshot, UsageWindow,
};
use crate::usage_display;
use chrono::{DateTime, Duration, Utc};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Deserialize;
use std::path::PathBuf;
use tracing::warn;

pub use account::{
    apply_login_account, discover_accounts, remove_managed_config_dir, sync_managed_account_dirs,
};
pub use login::{
    ClaudeLoginEvent, ClaudeLoginState, ClaudeLoginStatus, prepare, prepare_targeted, submit_code,
};

const ENDPOINT: &str = "https://api.anthropic.com/api/oauth/usage";
const FIVE_HOUR_SECONDS: i64 = 5 * 60 * 60;
const SEVEN_DAY_SECONDS: i64 = 7 * 24 * 60 * 60;

#[derive(Debug, Deserialize)]
struct ClaudeUsageResponse {
    #[serde(default)]
    pub email: Option<String>,
    pub five_hour: Option<ClaudeWindow>,
    pub seven_day: Option<ClaudeWindow>,
    pub seven_day_sonnet: Option<ClaudeWindow>,
    pub seven_day_opus: Option<ClaudeWindow>,
    pub seven_day_cowork: Option<ClaudeWindow>,
    #[allow(dead_code)]
    pub seven_day_omelette: Option<ClaudeWindow>,
    pub extra_usage: Option<ClaudeExtraUsage>,
}

#[derive(Debug, Deserialize)]
struct ClaudeWindow {
    #[serde(default)]
    pub utilization: Option<f32>,
    #[serde(default)]
    pub resets_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClaudeExtraUsage {
    #[serde(default)]
    pub is_enabled: Option<bool>,
    pub monthly_limit: Option<f64>,
    pub used_credits: Option<f64>,
    pub utilization: Option<f32>,
    pub currency: Option<String>,
}

fn claude_extra_usage_state(extra: &ClaudeExtraUsage) -> ExtraUsageState {
    if !extra.is_enabled.unwrap_or(true) {
        return ExtraUsageState::Disabled;
    }
    let pct = extra
        .utilization
        .or_else(|| {
            let limit = extra.monthly_limit.filter(|l| *l > 0.0)?;
            let used = extra.used_credits?;
            Some(usage_display::portion_percent(used, limit))
        })
        .unwrap_or(0.0)
        .clamp(0.0, 100.0);
    let currency = extra.currency.clone().unwrap_or_else(|| "$".to_string());
    ExtraUsageState::Active {
        used_percent: pct,
        cost: claude_snapshot_extra_cost(extra, pct, currency),
    }
}

fn claude_snapshot_extra_cost(
    extra: &ClaudeExtraUsage,
    pct: f32,
    currency: String,
) -> crate::model::ProviderCost {
    let limit = extra.monthly_limit.map(|limit| limit / 100.0);
    let used = extra
        .used_credits
        .map(|used| used / 100.0)
        .or_else(|| limit.map(|limit| limit * f64::from(pct) / 100.0))
        .unwrap_or(0.0);
    crate::model::ProviderCost {
        used,
        limit,
        units: currency,
    }
}

pub async fn fetch(
    client: &reqwest::Client,
    account_id: &str,
    config_dir: PathBuf,
) -> Result<UsageSnapshot, ClaudeError> {
    fetch_at(
        client,
        account_id,
        config_dir,
        ENDPOINT,
        oauth::TOKEN_ENDPOINT,
    )
    .await
}

const REFRESH_BEFORE_EXPIRY: Duration = Duration::minutes(5);

async fn fetch_at(
    client: &reqwest::Client,
    account_id: &str,
    account_dir: PathBuf,
    usage_endpoint: &str,
    token_endpoint: &str,
) -> Result<UsageSnapshot, ClaudeError> {
    let root = account_dir
        .parent()
        .ok_or_else(|| ClaudeError::TokenRefreshParse("invalid account dir path".to_string()))?;
    let storage = ProviderAccountStorage::new(root);

    let now = Utc::now();
    let mut tokens = storage
        .load_tokens(account_id)
        .map_err(|e| ClaudeError::TokenRefreshParse(e.to_string()))?;

    if tokens.expires_at <= now + REFRESH_BEFORE_EXPIRY {
        match oauth::refresh_access_token_at(client, token_endpoint, &tokens.refresh_token, now)
            .await
        {
            Ok(refreshed) => {
                tokens = refreshed_tokens(&refreshed);
                let _ = storage.save_tokens(account_id, &tokens);
                if let Some(meta) = refreshed_metadata(account_id, &refreshed, &storage) {
                    let _ = write_refreshed_metadata(&storage, account_id, &meta);
                }
            }
            Err(e) => {
                warn!(error = %e, "claude token preflight refresh failed");
                return Err(e);
            }
        }
    }

    let auth = auth_from_tokens(&tokens);
    let mut snapshot = match request_oauth_at(client, &auth, usage_endpoint).await {
        Err(ClaudeError::Unauthorized) => {
            warn!("claude usage endpoint returned 401; refreshing tokens and retrying once");
            let refreshed = oauth::refresh_access_token_at(
                client,
                token_endpoint,
                &tokens.refresh_token,
                Utc::now(),
            )
            .await?;
            let new_tokens = refreshed_tokens(&refreshed);
            let _ = storage.save_tokens(account_id, &new_tokens);
            if let Some(meta) = refreshed_metadata(account_id, &refreshed, &storage) {
                let _ = write_refreshed_metadata(&storage, account_id, &meta);
            }
            request_oauth_at(client, &auth_from_tokens(&new_tokens), usage_endpoint).await?
        }
        Ok(s) => s,
        Err(e) => return Err(e),
    };

    if snapshot.identity.email.as_deref().is_none_or(str::is_empty)
        && let Ok(metadata) = storage.load_metadata(account_id)
    {
        snapshot.identity.email = Some(metadata.email).filter(|e| !e.is_empty());
    }
    let _ = storage.save_snapshot(account_id, &snapshot);
    Ok(snapshot)
}

fn auth_from_tokens(tokens: &crate::account_storage::ProviderAccountTokens) -> ClaudeAuth {
    ClaudeAuth {
        access_token: tokens.access_token.clone(),
        scopes: tokens.scope.clone(),
        subscription_type: None,
    }
}

fn refreshed_tokens(
    response: &oauth::ClaudeTokenResponse,
) -> crate::account_storage::ProviderAccountTokens {
    use crate::account_storage::ProviderAccountTokens;
    ProviderAccountTokens {
        access_token: response.access_token.clone(),
        refresh_token: response.refresh_token.clone(),
        expires_at: response.expires_at,
        scope: response.scope.clone(),
        token_id: response.token_id.clone(),
    }
}

fn refreshed_metadata(
    account_id: &str,
    response: &oauth::ClaudeTokenResponse,
    storage: &ProviderAccountStorage,
) -> Option<ProviderAccountMetadata> {
    if response.account_id.is_none()
        && response.email.is_none()
        && response.organization_id.is_none()
    {
        return None;
    }
    let mut metadata = storage.load_metadata(account_id).ok()?;
    if let Some(email) = response.email.as_deref().filter(|e| !e.is_empty()) {
        metadata.email = email.to_ascii_lowercase();
    }
    if response.account_id.is_some() {
        metadata
            .provider_account_id
            .clone_from(&response.account_id);
    }
    if response.organization_id.is_some() {
        metadata
            .organization_id
            .clone_from(&response.organization_id);
    }
    if response.organization_name.is_some() {
        metadata
            .organization_name
            .clone_from(&response.organization_name);
    }
    metadata.updated_at = Utc::now();
    Some(metadata)
}

fn write_refreshed_metadata(
    storage: &ProviderAccountStorage,
    account_id: &str,
    metadata: &ProviderAccountMetadata,
) -> Result<(), ClaudeError> {
    storage
        .save_metadata(account_id, metadata)
        .map_err(|e| ClaudeError::TokenRefreshParse(e.to_string()))
}

async fn request_oauth_at(
    client: &reqwest::Client,
    auth: &crate::auth::ClaudeAuth,
    endpoint: &str,
) -> Result<UsageSnapshot, ClaudeError> {
    let subscription_type = auth.subscription_type.clone();
    if !auth.scopes.iter().any(|scope| scope == REQUIRED_SCOPE) {
        return Err(ClaudeError::MissingProfileScope);
    }
    let mut headers = HeaderMap::new();
    let bearer = format!("Bearer {}", auth.access_token);
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&bearer).map_err(ClaudeError::InvalidBearerHeader)?,
    );
    headers.insert(
        "anthropic-beta",
        HeaderValue::from_static("oauth-2025-04-20"),
    );
    let response = client
        .get(endpoint)
        .headers(headers)
        .send()
        .await
        .map_err(ClaudeError::UsageRequest)?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        warn!("claude usage endpoint returned 401; local OAuth credentials may be stale");
        return Err(ClaudeError::Unauthorized);
    }
    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        warn!("claude usage endpoint returned 429; rate limited");
        let retry_after_secs = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());
        return Err(ClaudeError::RateLimited { retry_after_secs });
    }
    let status = response.status();
    let response = response
        .error_for_status()
        .map_err(|source| ClaudeError::UsageEndpoint {
            status: status.as_u16(),
            source,
        })?;
    let payload: ClaudeUsageResponse = response.json().await.map_err(ClaudeError::DecodeUsage)?;
    normalize(&payload, subscription_type)
}

const REQUIRED_SCOPE: &str = "user:profile";

fn normalize(
    payload: &ClaudeUsageResponse,
    plan: Option<String>,
) -> Result<UsageSnapshot, ClaudeError> {
    let mut windows = Vec::new();
    if let Some(w) = normalize_window("Session", payload.five_hour.as_ref(), FIVE_HOUR_SECONDS)? {
        windows.push(w);
    }
    if let Some(w) = normalize_window("Weekly", payload.seven_day.as_ref(), SEVEN_DAY_SECONDS)? {
        windows.push(w);
    }
    let model_windows: &[(&str, &Option<ClaudeWindow>)] = &[
        ("Sonnet", &payload.seven_day_sonnet),
        ("Opus", &payload.seven_day_opus),
        ("Cowork", &payload.seven_day_cowork),
    ];
    for &(label, window) in model_windows {
        if let Some(w) = normalize_window(label, window.as_ref(), SEVEN_DAY_SECONDS)? {
            windows.push(w);
        }
    }
    if windows.is_empty() {
        return Err(ClaudeError::NoUsageData);
    }
    let extra_usage = payload.extra_usage.as_ref().map(claude_extra_usage_state);
    Ok(UsageSnapshot {
        provider: ProviderId::Claude,
        source: "OAuth".to_string(),
        updated_at: Utc::now(),
        headline: UsageHeadline::first_available(&windows),
        windows,
        provider_cost: None,
        extra_usage,
        identity: ProviderIdentity {
            email: payload.email.clone(),
            plan,
            ..Default::default()
        },
    })
}

fn normalize_window(
    label: &str,
    window: Option<&ClaudeWindow>,
    window_seconds: i64,
) -> Result<Option<UsageWindow>, ClaudeError> {
    let Some(window) = window else {
        return Ok(None);
    };
    let Some(used_percent) = window.utilization else {
        return Ok(None);
    };
    let reset_at = window
        .resets_at
        .as_ref()
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|source| ClaudeError::InvalidResetTimestamp {
                    value: value.clone(),
                    source,
                })
        })
        .transpose()?;
    Ok(Some(UsageWindow {
        label: label.to_string(),
        used_percent,
        reset_at,
        window_seconds: Some(window_seconds),
        reset_description: reset_at.map(|dt| dt.to_rfc3339()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_storage::{NewProviderAccount, ProviderAccountTokens};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    struct MockResponse {
        method: &'static str,
        path: &'static str,
        status: u16,
        body: String,
    }

    async fn server(
        responses: Vec<MockResponse>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buffer = vec![0; 8192];
                let bytes = stream.read(&mut buffer).await.unwrap();
                let request = String::from_utf8_lossy(&buffer[..bytes]).to_string();
                assert!(request.starts_with(&format!(
                    "{} {} HTTP/1.1\r\n",
                    response.method, response.path
                )));
                let status_text = if response.status == 200 { "OK" } else { "ERR" };
                let raw = format!(
                    "HTTP/1.1 {} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    response.status,
                    status_text,
                    response.body.len(),
                    response.body
                );
                stream.write_all(raw.as_bytes()).await.unwrap();
                requests.push(request);
            }
            requests
        });
        (format!("http://{addr}"), handle)
    }

    fn usage_body() -> String {
        r#"{
            "email": "user@example.com",
            "five_hour": {"utilization": 12.0, "resets_at": "2026-04-30T12:00:00+00:00"},
            "seven_day": {"utilization": 34.0, "resets_at": "2026-05-01T12:00:00+00:00"}
        }"#
        .to_string()
    }

    fn token_body(access_token: &str, refresh_token: &str) -> String {
        format!(
            r#"{{
                "access_token": "{access_token}",
                "refresh_token": "{refresh_token}",
                "expires_in": 3600,
                "scope": "user:profile"
            }}"#
        )
    }

    fn create_account(
        storage: &ProviderAccountStorage,
        expires_at: DateTime<Utc>,
    ) -> (String, PathBuf) {
        let stored = storage
            .create_account(NewProviderAccount {
                provider: ProviderId::Claude,
                email: "user@example.com".to_string(),
                provider_account_id: Some("claude-account".to_string()),
                organization_id: None,
                organization_name: None,
                tokens: ProviderAccountTokens {
                    access_token: "old-access".to_string(),
                    refresh_token: "old-refresh".to_string(),
                    expires_at,
                    scope: vec!["user:profile".to_string()],
                    token_id: None,
                },
                snapshot: None,
            })
            .unwrap();
        (stored.account_ref.account_id, stored.account_dir)
    }

    fn claude_oauth_usage_from_probe_fixture() -> ClaudeUsageResponse {
        let envelope: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/claude/oauth_usage_response.json"
        ))
        .unwrap();
        serde_json::from_value(envelope["body_json"].clone()).unwrap()
    }

    #[test]
    fn normalizes_oauth_usage_probe_fixture() {
        let payload = claude_oauth_usage_from_probe_fixture();
        let snapshot = normalize(&payload, None).unwrap();
        assert_eq!(snapshot.provider, ProviderId::Claude);
        assert_eq!(snapshot.windows.len(), 2);
        assert_eq!(snapshot.windows[0].label, "Session");
        assert_eq!(snapshot.windows[0].used_percent, 0.0);
        assert_eq!(snapshot.windows[1].label, "Weekly");
        assert_eq!(snapshot.windows[1].used_percent, 100.0);
        assert!(snapshot.provider_cost.is_none());
        assert!(snapshot.identity.email.is_none());
        match snapshot.extra_usage.as_ref() {
            Some(ExtraUsageState::Active { cost, used_percent }) => {
                assert!((*used_percent).abs() < f32::EPSILON);
                assert_eq!(cost.units, "EUR");
                assert_eq!(cost.used, 0.0);
                assert_eq!(cost.limit, Some(20.0));
            }
            _ => panic!("expected active extra usage"),
        }
    }

    #[test]
    fn normalizes_window_without_reset_time() {
        let payload: ClaudeUsageResponse = serde_json::from_str(
            r#"{
                "five_hour": {
                    "utilization": 42.0,
                    "resets_at": null
                },
                "seven_day": {
                    "utilization": null,
                    "resets_at": null
                },
                "extra_usage": null
            }"#,
        )
        .unwrap();

        let snapshot = normalize(&payload, None).unwrap();
        assert_eq!(snapshot.windows.len(), 1);
        assert_eq!(snapshot.windows[0].used_percent, 42.0);
        assert!(snapshot.windows[0].reset_at.is_none());
    }

    #[test]
    fn normalizes_usage_response_email_field() {
        let payload: ClaudeUsageResponse = serde_json::from_str(
            r#"{
                "email": "api@example.com",
                "five_hour": {"utilization": 1.0, "resets_at": "2026-04-11T11:00:01+00:00"},
                "seven_day": {"utilization": 2.0, "resets_at": "2026-04-11T14:00:00+00:00"}
            }"#,
        )
        .unwrap();
        let snapshot = normalize(&payload, None).unwrap();
        assert_eq!(snapshot.identity.email.as_deref(), Some("api@example.com"));
    }

    #[test]
    fn auth_from_tokens_sets_access_token_and_scopes() {
        use crate::account_storage::ProviderAccountTokens;
        let tokens = ProviderAccountTokens {
            access_token: "my-token".to_string(),
            refresh_token: "my-refresh".to_string(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
            scope: vec!["user:profile".to_string()],
            token_id: None,
        };
        let auth = auth_from_tokens(&tokens);
        assert_eq!(auth.access_token, "my-token");
        assert_eq!(auth.scopes, ["user:profile"]);
    }

    #[tokio::test]
    async fn preflight_permanent_refresh_failure_does_not_call_usage() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let (account_id, account_dir) = create_account(&storage, Utc::now());
        let (base_url, handle) = server(vec![MockResponse {
            method: "POST",
            path: "/token",
            status: 400,
            body: "{}".to_string(),
        }])
        .await;

        let error = fetch_at(
            &reqwest::Client::new(),
            &account_id,
            account_dir,
            &format!("{base_url}/usage"),
            &format!("{base_url}/token"),
        )
        .await
        .unwrap_err();

        assert!(error.requires_user_action());
        assert!(!error.is_transient());
        assert_eq!(handle.await.unwrap().len(), 1);
        let tokens = storage.load_tokens(&account_id).unwrap();
        assert_eq!(tokens.access_token, "old-access");
        assert_eq!(tokens.refresh_token, "old-refresh");
    }

    #[tokio::test]
    async fn preflight_transient_refresh_failure_does_not_call_usage() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let (account_id, account_dir) = create_account(&storage, Utc::now());
        let (base_url, handle) = server(vec![MockResponse {
            method: "POST",
            path: "/token",
            status: 500,
            body: "{}".to_string(),
        }])
        .await;

        let error = fetch_at(
            &reqwest::Client::new(),
            &account_id,
            account_dir,
            &format!("{base_url}/usage"),
            &format!("{base_url}/token"),
        )
        .await
        .unwrap_err();

        assert!(matches!(
            error,
            ClaudeError::TokenRefreshHttp { status: 500 }
        ));
        assert!(!error.requires_user_action());
        assert!(error.is_transient());
        assert_eq!(handle.await.unwrap().len(), 1);
        let tokens = storage.load_tokens(&account_id).unwrap();
        assert_eq!(tokens.access_token, "old-access");
        assert_eq!(tokens.refresh_token, "old-refresh");
    }

    #[tokio::test]
    async fn preflight_refresh_success_fetches_usage_with_fresh_token() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let (account_id, account_dir) = create_account(&storage, Utc::now());
        let (base_url, handle) = server(vec![
            MockResponse {
                method: "POST",
                path: "/token",
                status: 200,
                body: token_body("new-access", "new-refresh"),
            },
            MockResponse {
                method: "GET",
                path: "/usage",
                status: 200,
                body: usage_body(),
            },
        ])
        .await;

        fetch_at(
            &reqwest::Client::new(),
            &account_id,
            account_dir,
            &format!("{base_url}/usage"),
            &format!("{base_url}/token"),
        )
        .await
        .unwrap();

        let requests = handle.await.unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].contains("\"refresh_token\":\"old-refresh\""));
        assert!(requests[1].contains("authorization: Bearer new-access"));
        let tokens = storage.load_tokens(&account_id).unwrap();
        assert_eq!(tokens.access_token, "new-access");
        assert_eq!(tokens.refresh_token, "new-refresh");
    }

    #[tokio::test]
    async fn reactive_refresh_success_retries_usage_with_fresh_token() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let (account_id, account_dir) = create_account(&storage, Utc::now() + Duration::hours(1));
        let (base_url, handle) = server(vec![
            MockResponse {
                method: "GET",
                path: "/usage",
                status: 401,
                body: "{}".to_string(),
            },
            MockResponse {
                method: "POST",
                path: "/token",
                status: 200,
                body: token_body("new-access", "new-refresh"),
            },
            MockResponse {
                method: "GET",
                path: "/usage",
                status: 200,
                body: usage_body(),
            },
        ])
        .await;

        let snapshot = fetch_at(
            &reqwest::Client::new(),
            &account_id,
            account_dir,
            &format!("{base_url}/usage"),
            &format!("{base_url}/token"),
        )
        .await
        .unwrap();

        assert_eq!(snapshot.identity.email.as_deref(), Some("user@example.com"));
        let requests = handle.await.unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests[0].contains("authorization: Bearer old-access"));
        assert!(requests[1].contains("\"refresh_token\":\"old-refresh\""));
        assert!(requests[2].contains("authorization: Bearer new-access"));
        let tokens = storage.load_tokens(&account_id).unwrap();
        assert_eq!(tokens.access_token, "new-access");
        assert_eq!(tokens.refresh_token, "new-refresh");
    }

    #[tokio::test]
    async fn reactive_refresh_retries_usage_only_once() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let (account_id, account_dir) = create_account(&storage, Utc::now() + Duration::hours(1));
        let (base_url, handle) = server(vec![
            MockResponse {
                method: "GET",
                path: "/usage",
                status: 401,
                body: "{}".to_string(),
            },
            MockResponse {
                method: "POST",
                path: "/token",
                status: 200,
                body: token_body("new-access", "new-refresh"),
            },
            MockResponse {
                method: "GET",
                path: "/usage",
                status: 401,
                body: "{}".to_string(),
            },
        ])
        .await;

        let error = fetch_at(
            &reqwest::Client::new(),
            &account_id,
            account_dir,
            &format!("{base_url}/usage"),
            &format!("{base_url}/token"),
        )
        .await
        .unwrap_err();

        assert!(matches!(error, ClaudeError::Unauthorized));
        let requests = handle.await.unwrap();
        assert_eq!(requests.len(), 3);
        let tokens = storage.load_tokens(&account_id).unwrap();
        assert_eq!(tokens.access_token, "new-access");
        assert_eq!(tokens.refresh_token, "new-refresh");
    }

    #[tokio::test]
    async fn reactive_permanent_refresh_failure_requires_user_action() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let (account_id, account_dir) = create_account(&storage, Utc::now() + Duration::hours(1));
        let (base_url, handle) = server(vec![
            MockResponse {
                method: "GET",
                path: "/usage",
                status: 401,
                body: "{}".to_string(),
            },
            MockResponse {
                method: "POST",
                path: "/token",
                status: 400,
                body: "{}".to_string(),
            },
        ])
        .await;

        let error = fetch_at(
            &reqwest::Client::new(),
            &account_id,
            account_dir,
            &format!("{base_url}/usage"),
            &format!("{base_url}/token"),
        )
        .await
        .unwrap_err();

        assert!(error.requires_user_action());
        assert!(!error.is_transient());
        assert_eq!(handle.await.unwrap().len(), 2);
        let tokens = storage.load_tokens(&account_id).unwrap();
        assert_eq!(tokens.access_token, "old-access");
        assert_eq!(tokens.refresh_token, "old-refresh");
    }

    #[tokio::test]
    async fn reactive_transient_refresh_failure_preserves_stale_behavior() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let (account_id, account_dir) = create_account(&storage, Utc::now() + Duration::hours(1));
        let (base_url, handle) = server(vec![
            MockResponse {
                method: "GET",
                path: "/usage",
                status: 401,
                body: "{}".to_string(),
            },
            MockResponse {
                method: "POST",
                path: "/token",
                status: 500,
                body: "{}".to_string(),
            },
        ])
        .await;

        let error = fetch_at(
            &reqwest::Client::new(),
            &account_id,
            account_dir,
            &format!("{base_url}/usage"),
            &format!("{base_url}/token"),
        )
        .await
        .unwrap_err();

        assert!(!error.requires_user_action());
        assert!(error.is_transient());
        assert_eq!(handle.await.unwrap().len(), 2);
        let tokens = storage.load_tokens(&account_id).unwrap();
        assert_eq!(tokens.access_token, "old-access");
        assert_eq!(tokens.refresh_token, "old-refresh");
    }
}
