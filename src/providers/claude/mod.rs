// SPDX-License-Identifier: MPL-2.0

mod account;
mod login;
mod refresh;

use crate::auth::{email_from_claude_credentials, load_claude_auth_from_path};
use crate::error::{ClaudeError, Result};
use crate::model::{
    ProviderCost, ProviderId, ProviderIdentity, UsageHeadline, UsageSnapshot, UsageWindow,
};
use chrono::{DateTime, Utc};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tracing::warn;

pub(crate) use account::external_claude_config_dir_candidate;
pub use account::{
    ambient_active_account_id, apply_login_account, discover_accounts, remove_managed_config_dir,
    sync_imported_account, sync_managed_accounts,
};
pub use login::{ClaudeLoginEvent, ClaudeLoginState, ClaudeLoginStatus, prepare};
use refresh::load_fresh_auth;
pub use refresh::refresh_claude_credentials;

const ENDPOINT: &str = "https://api.anthropic.com/api/oauth/usage";
const OAUTH_ACCOUNT_ENDPOINT: &str = "https://api.anthropic.com/api/oauth/account";
const OAUTH_PROFILE_ENDPOINT: &str = "https://api.anthropic.com/api/oauth/profile";
const REQUIRED_SCOPE: &str = "user:profile";
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

#[derive(Debug, Deserialize)]
struct ClaudeOAuthAccountResponse {
    #[serde(default)]
    email_address: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClaudeOAuthProfileResponse {
    #[serde(default)]
    account: Option<ClaudeOAuthProfileAccount>,
}

#[derive(Debug, Deserialize)]
struct ClaudeOAuthProfileAccount {
    #[serde(default)]
    email: Option<String>,
}

pub async fn fetch(
    client: &reqwest::Client,
    config_dir: PathBuf,
) -> Result<UsageSnapshot, ClaudeError> {
    let credentials_path = crate::auth::claude_credentials_path_for_config_dir(&config_dir);
    let auth = load_fresh_auth(&credentials_path, Utc::now())?;
    let mut snapshot = match request_oauth(client, &auth).await {
        Err(ClaudeError::Unauthorized) => {
            warn!("claude usage endpoint returned 401; attempting Claude Code credential refresh");
            refresh_claude_credentials(&credentials_path)?;
            let refreshed = load_claude_auth_from_path(&credentials_path)?;
            request_oauth(client, &refreshed).await?
        }
        Ok(s) => s,
        Err(e) => return Err(e),
    };
    hydrate_claude_snapshot_identity_email(client, &config_dir, &mut snapshot).await;
    Ok(snapshot)
}

async fn hydrate_claude_snapshot_identity_email(
    client: &reqwest::Client,
    config_dir: &Path,
    snapshot: &mut UsageSnapshot,
) {
    if snapshot
        .identity
        .email
        .as_deref()
        .is_some_and(|e| !e.is_empty())
    {
        return;
    }
    let credentials_path = crate::auth::claude_credentials_path_for_config_dir(config_dir);
    let email = refresh::load_account_status(config_dir)
        .ok()
        .and_then(|s| s.email)
        .filter(|e| !e.is_empty())
        .or_else(|| {
            load_claude_auth_from_path(&credentials_path)
                .ok()
                .and_then(|a| email_from_claude_credentials(&a))
        });
    let email = match email {
        Some(email) => Some(email),
        None => fetch_usage_email(client, config_dir).await,
    };
    snapshot.identity.email = email.filter(|e| !e.is_empty());
}

pub(crate) fn blocking_fetch_usage_email(config_dir: &Path) -> Option<String> {
    let credentials_path = crate::auth::claude_credentials_path_for_config_dir(config_dir);
    let auth = load_claude_auth_from_path(&credentials_path).ok()?;
    if !auth.scopes.iter().any(|scope| scope == REQUIRED_SCOPE) {
        return None;
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(crate::runtime::HTTP_TIMEOUT)
        .connect_timeout(crate::runtime::HTTP_CONNECT_TIMEOUT)
        .build()
        .ok()?;

    let send =
        |path: &Path, url: &str| -> Option<(reqwest::StatusCode, reqwest::blocking::Response)> {
            let auth = load_claude_auth_from_path(path).ok()?;
            let bearer = format!("Bearer {}", auth.access_token);
            let auth_header = HeaderValue::from_str(&bearer).ok()?;
            let response = client
                .get(url)
                .header(AUTHORIZATION, auth_header)
                .header(
                    "anthropic-beta",
                    HeaderValue::from_static("oauth-2025-04-20"),
                )
                .send()
                .ok()?;
            Some((response.status(), response))
        };

    let get_json = |url: &str| -> Option<reqwest::blocking::Response> {
        let response = match send(&credentials_path, url)? {
            (status, _resp) if status == reqwest::StatusCode::UNAUTHORIZED => {
                refresh::refresh_claude_credentials(&credentials_path).ok()?;
                let (status2, resp2) = send(&credentials_path, url)?;
                if !status2.is_success() {
                    return None;
                }
                resp2
            }
            (status, resp) if status.is_success() => resp,
            _ => return None,
        };
        Some(response)
    };

    if let Some(resp) = get_json(OAUTH_ACCOUNT_ENDPOINT)
        && let Ok(payload) = resp.json::<ClaudeOAuthAccountResponse>()
        && let Some(e) = payload.email_address.filter(|e| !e.is_empty())
    {
        return Some(e);
    }

    if let Some(resp) = get_json(OAUTH_PROFILE_ENDPOINT)
        && let Ok(payload) = resp.json::<ClaudeOAuthProfileResponse>()
        && let Some(e) = payload
            .account
            .and_then(|a| a.email)
            .filter(|e| !e.is_empty())
    {
        return Some(e);
    }

    if let Some(resp) = get_json(ENDPOINT)
        && let Ok(payload) = resp.json::<ClaudeUsageResponse>()
        && let Some(e) = payload.email.filter(|e| !e.is_empty())
    {
        return Some(e);
    }

    None
}

async fn fetch_usage_email(client: &reqwest::Client, config_dir: &Path) -> Option<String> {
    let credentials_path = crate::auth::claude_credentials_path_for_config_dir(config_dir);
    let auth = load_claude_auth_from_path(&credentials_path).ok()?;
    if !auth.scopes.iter().any(|scope| scope == REQUIRED_SCOPE) {
        return None;
    }

    if let Some(resp) =
        get_usage_email_json(client, &credentials_path, OAUTH_ACCOUNT_ENDPOINT).await
        && let Ok(payload) = resp.json::<ClaudeOAuthAccountResponse>().await
        && let Some(e) = payload.email_address.filter(|e| !e.is_empty())
    {
        return Some(e);
    }

    if let Some(resp) =
        get_usage_email_json(client, &credentials_path, OAUTH_PROFILE_ENDPOINT).await
        && let Ok(payload) = resp.json::<ClaudeOAuthProfileResponse>().await
        && let Some(e) = payload
            .account
            .and_then(|a| a.email)
            .filter(|e| !e.is_empty())
    {
        return Some(e);
    }

    if let Some(resp) = get_usage_email_json(client, &credentials_path, ENDPOINT).await
        && let Ok(payload) = resp.json::<ClaudeUsageResponse>().await
        && let Some(e) = payload.email.filter(|e| !e.is_empty())
    {
        return Some(e);
    }

    None
}

async fn get_usage_email_json(
    client: &reqwest::Client,
    credentials_path: &Path,
    url: &str,
) -> Option<reqwest::Response> {
    let response = match send_usage_email_request(client, credentials_path, url).await? {
        (status, _) if status == reqwest::StatusCode::UNAUTHORIZED => {
            refresh::refresh_claude_credentials(credentials_path).ok()?;
            let (status2, resp2) = send_usage_email_request(client, credentials_path, url).await?;
            if !status2.is_success() {
                return None;
            }
            resp2
        }
        (status, resp) if status.is_success() => resp,
        _ => return None,
    };
    Some(response)
}

async fn send_usage_email_request(
    client: &reqwest::Client,
    credentials_path: &Path,
    url: &str,
) -> Option<(reqwest::StatusCode, reqwest::Response)> {
    let auth = load_claude_auth_from_path(credentials_path).ok()?;
    let bearer = format!("Bearer {}", auth.access_token);
    let auth_header = HeaderValue::from_str(&bearer).ok()?;
    let response = client
        .get(url)
        .header(AUTHORIZATION, auth_header)
        .header(
            "anthropic-beta",
            HeaderValue::from_static("oauth-2025-04-20"),
        )
        .send()
        .await
        .ok()?;
    Some((response.status(), response))
}

async fn request_oauth(
    client: &reqwest::Client,
    auth: &crate::auth::ClaudeAuth,
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
        .get(ENDPOINT)
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
        return Err(ClaudeError::RateLimited);
    }
    let status = response.status();
    let response = response
        .error_for_status()
        .map_err(|source| ClaudeError::UsageEndpoint {
            status: status.as_u16(),
            source,
        })?;
    let payload: ClaudeUsageResponse = response.json().await.map_err(ClaudeError::DecodeUsage)?;
    normalize(payload, subscription_type)
}

fn normalize(
    payload: ClaudeUsageResponse,
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
    let extra_enabled = payload
        .extra_usage
        .as_ref()
        .is_some_and(|extra| extra.is_enabled.unwrap_or(true));
    if let Some(extra) = payload.extra_usage.as_ref().filter(|_| extra_enabled)
        && let Some(utilization) = extra.utilization
    {
        windows.push(UsageWindow {
            label: "Extra".to_string(),
            used_percent: utilization,
            reset_at: None,
            window_seconds: None,
            reset_description: None,
        });
    }
    let currency = payload
        .extra_usage
        .as_ref()
        .and_then(|u| u.currency.clone())
        .unwrap_or_else(|| "$".to_string());
    let provider_cost = payload
        .extra_usage
        .filter(|_| extra_enabled)
        .and_then(|usage| {
            usage.used_credits.map(|used| ProviderCost {
                used: used / 100.0,
                limit: usage.monthly_limit.map(|limit| limit / 100.0),
                units: currency.clone(),
            })
        });
    Ok(UsageSnapshot {
        provider: ProviderId::Claude,
        source: "OAuth".to_string(),
        updated_at: Utc::now(),
        headline: UsageHeadline::first_available(&windows),
        windows,
        provider_cost,
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
    use crate::auth::load_claude_auth_from_config_dir;

    #[test]
    fn normalizes_fixture() {
        let payload: ClaudeUsageResponse =
            serde_json::from_str(include_str!("../../../fixtures/claude/usage_oauth.json"))
                .unwrap();
        let snapshot = normalize(payload, None).unwrap();
        assert_eq!(snapshot.provider, ProviderId::Claude);
        assert_eq!(snapshot.windows[0].used_percent, 18.0);
        assert_eq!(snapshot.windows[1].used_percent, 90.0);
        assert_eq!(snapshot.provider_cost.as_ref().unwrap().used, 2.44);
    }

    #[test]
    fn normalizes_fixture_with_extra_usage_disabled() {
        let payload: ClaudeUsageResponse = serde_json::from_str(include_str!(
            "../../../fixtures/claude/usage_oauth_extra_disabled.json"
        ))
        .unwrap();
        let snapshot = normalize(payload, Some("pro".to_string())).unwrap();
        assert_eq!(snapshot.provider, ProviderId::Claude);
        assert_eq!(snapshot.windows[0].used_percent, 83.0);
        assert_eq!(snapshot.windows[1].used_percent, 21.0);
        assert_eq!(snapshot.windows.len(), 2);
        assert!(snapshot.provider_cost.is_none());
    }

    #[test]
    fn normalizes_max_fixture_with_model_windows() {
        let payload: ClaudeUsageResponse = serde_json::from_str(include_str!(
            "../../../fixtures/claude/usage_oauth_max_2026_03_10.json"
        ))
        .unwrap();
        let snapshot = normalize(payload, Some("max".to_string())).unwrap();
        assert_eq!(snapshot.provider, ProviderId::Claude);
        assert_eq!(snapshot.windows[0].label, "Session");
        assert_eq!(snapshot.windows[0].used_percent, 37.0);
        assert_eq!(snapshot.windows[1].label, "Weekly");
        assert_eq!(snapshot.windows[1].used_percent, 26.0);
        assert_eq!(snapshot.windows[2].label, "Sonnet");
        assert_eq!(snapshot.windows[2].used_percent, 1.0);
        assert_eq!(snapshot.windows.len(), 3);
        assert!(snapshot.provider_cost.is_none());
    }

    #[test]
    fn normalizes_live_fixture_with_currency() {
        let payload: ClaudeUsageResponse = serde_json::from_str(include_str!(
            "../../../fixtures/claude/usage_oauth_live_2026_04_20.json"
        ))
        .unwrap();
        let snapshot = normalize(payload, Some("pro".to_string())).unwrap();
        assert_eq!(snapshot.windows[0].label, "Session");
        assert_eq!(snapshot.windows[1].label, "Weekly");
        assert_eq!(snapshot.windows[2].label, "Extra");
        assert_eq!(snapshot.windows.len(), 3);
        let cost = snapshot.provider_cost.as_ref().unwrap();
        assert_eq!(cost.units, "EUR");
        assert_eq!(cost.used, 4.80);
        assert_eq!(cost.limit, Some(5.0));
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

        let snapshot = normalize(payload, None).unwrap();
        assert_eq!(snapshot.windows.len(), 1);
        assert_eq!(snapshot.windows[0].used_percent, 42.0);
        assert!(snapshot.windows[0].reset_at.is_none());
    }

    #[test]
    fn parses_auth_from_managed_config_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".credentials.json"),
            r#"{
  "claudeAiOauth": {
    "accessToken": "token",
    "expiresAt": 1776609779660,
    "scopes": ["user:profile"],
    "subscriptionType": "pro"
  }
}"#,
        )
        .unwrap();

        let auth = load_claude_auth_from_config_dir(dir.path()).unwrap();

        assert_eq!(auth.subscription_type.as_deref(), Some("pro"));
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
        let snapshot = normalize(payload, None).unwrap();
        assert_eq!(snapshot.identity.email.as_deref(), Some("api@example.com"));
    }

    #[test]
    fn hydrates_identity_email_from_jwt_in_credentials() {
        use base64::Engine;

        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("{}");
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"email":"hydrate@example.com"}"#);
        let jwt = format!("{header}.{payload}.sig");
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".credentials.json"),
            format!(r#"{{"claudeAiOauth":{{"accessToken":"{jwt}","scopes":["user:profile"]}}}}"#),
        )
        .unwrap();

        let usage: ClaudeUsageResponse = serde_json::from_str(
            r#"{
                "five_hour": {"utilization": 1.0, "resets_at": "2026-04-11T11:00:01+00:00"},
                "seven_day": {"utilization": 2.0, "resets_at": "2026-04-11T14:00:00+00:00"}
            }"#,
        )
        .unwrap();
        let mut snapshot = normalize(usage, None).unwrap();
        assert!(snapshot.identity.email.is_none());
        let client = reqwest::Client::new();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(hydrate_claude_snapshot_identity_email(
                &client,
                dir.path(),
                &mut snapshot,
            ));
        assert_eq!(
            snapshot.identity.email.as_deref(),
            Some("hydrate@example.com")
        );
    }
}
