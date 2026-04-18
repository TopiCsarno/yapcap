use crate::auth::load_claude_auth;
use crate::error::{ClaudeError, Result};
use crate::model::{
    ProviderCost, ProviderId, ProviderIdentity, UsageHeadline, UsageSnapshot, UsageWindow,
};
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
    pub utilization: f64,
    pub resets_at: String,
}

#[derive(Debug, Deserialize)]
struct ClaudeExtraUsage {
    #[serde(default)]
    pub is_enabled: Option<bool>,
    pub monthly_limit: Option<f64>,
    pub used_credits: Option<f64>,
    pub utilization: Option<f64>,
}

pub async fn fetch(client: &reqwest::Client) -> Result<UsageSnapshot> {
    fetch_oauth(client).await
}

async fn fetch_oauth(client: &reqwest::Client) -> Result<UsageSnapshot> {
    let auth = load_claude_auth()?;
    let subscription_type = auth.subscription_type.clone();
    if !auth.scopes.iter().any(|scope| scope == REQUIRED_SCOPE) {
        return Err(ClaudeError::MissingProfileScope.into());
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
        return Err(ClaudeError::Unauthorized.into());
    }
    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        warn!("claude usage endpoint returned 429; rate limited");
        return Err(ClaudeError::RateLimited.into());
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

fn normalize(payload: ClaudeUsageResponse, plan: Option<String>) -> Result<UsageSnapshot> {
    let primary = payload
        .five_hour
        .as_ref()
        .map(|window| normalize_window("Session", window))
        .transpose()?;
    let secondary = payload
        .seven_day
        .as_ref()
        .map(|window| normalize_window("Weekly", window))
        .transpose()?;
    if primary.is_none() && secondary.is_none() {
        return Err(ClaudeError::NoUsageData.into());
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

fn normalize_window(label: &str, window: &ClaudeWindow) -> Result<UsageWindow> {
    let reset_at = DateTime::parse_from_rfc3339(&window.resets_at)
        .map_err(|source| ClaudeError::InvalidResetTimestamp {
            value: window.resets_at.clone(),
            source,
        })?
        .with_timezone(&Utc);
    Ok(UsageWindow {
        label: label.to_string(),
        used_percent: window.utilization,
        reset_at: Some(reset_at),
        reset_description: Some(reset_at.to_rfc3339()),
    })
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
}
