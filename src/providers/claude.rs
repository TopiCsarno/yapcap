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
    pub extra_usage: Option<ClaudeExtraUsage>,
}

#[derive(Debug, Deserialize)]
struct ClaudeWindow {
    #[serde(default)]
    pub utilization: Option<f64>,
    #[serde(default)]
    pub resets_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClaudeExtraUsage {
    #[serde(default)]
    pub is_enabled: Option<bool>,
    pub monthly_limit: Option<f64>,
    pub used_credits: Option<f64>,
    pub utilization: Option<f64>,
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
    let primary = normalize_window("Session", payload.five_hour.as_ref())?;
    let secondary = normalize_window("Weekly", payload.seven_day.as_ref())?;
    if primary.is_none() && secondary.is_none() {
        return Err(ClaudeError::NoUsageData);
    }
    let extra_enabled = payload
        .extra_usage
        .as_ref()
        .is_some_and(|extra| extra.is_enabled.unwrap_or(true));
    let tertiary = payload
        .extra_usage
        .as_ref()
        .filter(|_| extra_enabled)
        .and_then(|extra| {
            extra.utilization.map(|utilization| UsageWindow {
                label: "Extra".to_string(),
                used_percent: utilization,
                reset_at: None,
                reset_description: None,
            })
        });
    let provider_cost = payload
        .extra_usage
        .filter(|_| extra_enabled)
        .and_then(|usage| {
            usage.used_credits.map(|used| ProviderCost {
                used: used / 100.0,
                limit: usage.monthly_limit.map(|limit| limit / 100.0),
                units: "$".to_string(),
            })
        });
    Ok(UsageSnapshot {
        provider: ProviderId::Claude,
        source: "OAuth".to_string(),
        updated_at: Utc::now(),
        headline: UsageHeadline::primary_first(
            primary.as_ref(),
            secondary.as_ref(),
            tertiary.as_ref(),
        ),
        primary,
        secondary,
        tertiary,
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
        assert_eq!(snapshot.primary.as_ref().unwrap().used_percent, 18.0);
        assert_eq!(snapshot.secondary.as_ref().unwrap().used_percent, 90.0);
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
        assert_eq!(snapshot.primary.as_ref().unwrap().used_percent, 83.0);
        assert_eq!(snapshot.secondary.as_ref().unwrap().used_percent, 21.0);
        assert!(snapshot.tertiary.is_none());
        assert!(snapshot.provider_cost.is_none());
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
        let primary = snapshot.primary.as_ref().unwrap();
        assert_eq!(primary.used_percent, 42.0);
        assert!(primary.reset_at.is_none());
        assert!(snapshot.secondary.is_none());
    }
}
