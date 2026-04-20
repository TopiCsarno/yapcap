// SPDX-License-Identifier: MPL-2.0

use crate::auth::{claude_credentials_path, load_claude_auth_from_path};
use crate::error::ClaudeError;
use crate::model::{
    ProviderCost, ProviderId, ProviderIdentity, UsageHeadline, UsageSnapshot, UsageWindow,
};
use crate::providers::claude_refresh::{load_fresh_auth, refresh_claude_credentials};
use chrono::{DateTime, Utc};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Deserialize;
use tracing::warn;

const ENDPOINT: &str = "https://api.anthropic.com/api/oauth/usage";
const REQUIRED_SCOPE: &str = "user:profile";

#[derive(Debug, Deserialize)]
struct ClaudeUsageResponse {
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

pub async fn fetch(client: &reqwest::Client) -> Result<UsageSnapshot, ClaudeError> {
    fetch_oauth(client).await
}

async fn fetch_oauth(client: &reqwest::Client) -> Result<UsageSnapshot, ClaudeError> {
    let credentials_path = claude_credentials_path()?;
    let auth = load_fresh_auth(&credentials_path, Utc::now())?;
    match request_oauth(client, &auth).await {
        Err(ClaudeError::Unauthorized) => {
            warn!("claude usage endpoint returned 401; attempting Claude Code credential refresh");
            refresh_claude_credentials(&credentials_path)?;
            let auth = load_claude_auth_from_path(&credentials_path)?;
            request_oauth(client, &auth).await
        }
        result => result,
    }
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
    if let Some(w) = normalize_window("Session", payload.five_hour.as_ref())? {
        windows.push(w);
    }
    if let Some(w) = normalize_window("Weekly", payload.seven_day.as_ref())? {
        windows.push(w);
    }
    let model_windows: &[(&str, &Option<ClaudeWindow>)] = &[
        ("Sonnet", &payload.seven_day_sonnet),
        ("Opus", &payload.seven_day_opus),
        ("Cowork", &payload.seven_day_cowork),
    ];
    for &(label, window) in model_windows {
        if let Some(w) = normalize_window(label, window.as_ref())? {
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
            plan,
            ..Default::default()
        },
    })
}

fn normalize_window(
    label: &str,
    window: Option<&ClaudeWindow>,
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
        reset_description: reset_at.map(|dt| dt.to_rfc3339()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_fixture() {
        let payload: ClaudeUsageResponse =
            serde_json::from_str(include_str!("../../fixtures/claude/usage_oauth.json")).unwrap();
        let snapshot = normalize(payload, None).unwrap();
        assert_eq!(snapshot.provider, ProviderId::Claude);
        assert_eq!(snapshot.windows[0].used_percent, 18.0);
        assert_eq!(snapshot.windows[1].used_percent, 90.0);
        assert_eq!(snapshot.provider_cost.as_ref().unwrap().used, 2.44);
    }

    #[test]
    fn normalizes_fixture_with_extra_usage_disabled() {
        let payload: ClaudeUsageResponse = serde_json::from_str(include_str!(
            "../../fixtures/claude/usage_oauth_extra_disabled.json"
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
            "../../fixtures/claude/usage_oauth_max_2026_03_10.json"
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
            "../../fixtures/claude/usage_oauth_live_2026_04_20.json"
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
}
