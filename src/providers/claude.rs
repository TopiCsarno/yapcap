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
    pub monthly_limit: f64,
    pub used_credits: f64,
}

pub async fn fetch(client: &reqwest::Client) -> Result<UsageSnapshot> {
    let auth = load_claude_auth()?;
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
        warn!("claude usage endpoint returned 401; local CLI credentials may be stale");
        return Err(ClaudeError::Unauthorized.into());
    }
    let response = response
        .error_for_status()
        .map_err(ClaudeError::UsageEndpoint)?;
    let payload: ClaudeUsageResponse = response.json().await.map_err(ClaudeError::DecodeUsage)?;
    normalize(payload)
}

fn normalize(payload: ClaudeUsageResponse) -> Result<UsageSnapshot> {
    let primary = payload
        .five_hour
        .as_ref()
        .map(|window| normalize_window("5h", window))
        .transpose()?;
    let secondary = payload
        .seven_day
        .as_ref()
        .map(|window| normalize_window("7d", window))
        .transpose()?;
    let headline = if secondary.is_some() {
        UsageHeadline::Secondary
    } else {
        UsageHeadline::Primary
    };
    if primary.is_none() && secondary.is_none() {
        return Err(ClaudeError::NoUsageData.into());
    }
    Ok(UsageSnapshot {
        provider: ProviderId::Claude,
        source: "OAuth".to_string(),
        updated_at: Utc::now(),
        headline,
        primary,
        secondary,
        tertiary: None,
        provider_cost: payload.extra_usage.map(|usage| ProviderCost {
            used: usage.used_credits,
            limit: Some(usage.monthly_limit),
            units: "usd".to_string(),
        }),
        identity: ProviderIdentity::default(),
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
        let snapshot = normalize(payload).unwrap();
        assert_eq!(snapshot.provider, ProviderId::Claude);
        assert_eq!(snapshot.primary.as_ref().unwrap().used_percent, 18.0);
        assert_eq!(snapshot.secondary.as_ref().unwrap().used_percent, 90.0);
        assert_eq!(snapshot.provider_cost.as_ref().unwrap().used, 244.0);
    }
}
