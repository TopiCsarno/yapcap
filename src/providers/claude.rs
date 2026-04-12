use super::claude_cli;
use super::claude_web;
use crate::auth::load_claude_auth;
use crate::config::CursorBrowser;
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
const TEMP_DISABLE_OAUTH_FOR_TESTING: bool = true;

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
    pub utilization: f64,
}

pub async fn fetch(client: &reqwest::Client) -> Result<UsageSnapshot> {
    if TEMP_DISABLE_OAUTH_FOR_TESTING {
        warn!("claude oauth temporarily disabled; forcing CLI/web fallback");
        return fetch_without_oauth(client, CursorBrowser::Firefox).await;
    }

    match fetch_oauth(client).await {
        Ok(snapshot) => Ok(snapshot),
        Err(error) => {
            warn!(error = %error, "claude oauth failed; trying CLI then web fallback");
            fetch_without_oauth(client, CursorBrowser::Firefox).await
        }
    }
}

pub async fn fetch_with_browser(
    client: &reqwest::Client,
    browser: CursorBrowser,
) -> Result<UsageSnapshot> {
    if TEMP_DISABLE_OAUTH_FOR_TESTING {
        return fetch_without_oauth(client, browser).await;
    }

    match fetch_oauth(client).await {
        Ok(snapshot) => Ok(snapshot),
        Err(error) => {
            warn!(error = %error, "claude oauth failed; trying CLI then web fallback");
            fetch_without_oauth(client, browser).await
        }
    }
}

async fn fetch_without_oauth(
    client: &reqwest::Client,
    browser: CursorBrowser,
) -> Result<UsageSnapshot> {
    match claude_cli::fetch().await {
        Ok(snapshot) => Ok(snapshot),
        Err(cli_error) => {
            warn!(error = %cli_error, "claude CLI failed; trying web cookie fallback");
            claude_web::fetch(client, browser).await
        }
    }
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
        warn!("claude usage endpoint returned 401; local CLI credentials may be stale");
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
    let tertiary = payload.extra_usage.as_ref().map(|extra| UsageWindow {
        label: "Extra".to_string(),
        used_percent: extra.utilization,
        reset_at: None,
        reset_description: None,
    });
    let provider_cost = payload.extra_usage.map(|usage| ProviderCost {
        used: usage.used_credits / 100.0,
        limit: Some(usage.monthly_limit / 100.0),
        units: "$".to_string(),
    });
    Ok(UsageSnapshot {
        provider: ProviderId::Claude,
        source: "OAuth".to_string(),
        updated_at: Utc::now(),
        headline,
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
    use crate::providers::claude_cli;

    fn assert_usage_equivalent(left: &UsageSnapshot, right: &UsageSnapshot) {
        assert_eq!(
            left.primary.as_ref().map(|window| window.used_percent),
            right.primary.as_ref().map(|window| window.used_percent)
        );
        assert_eq!(
            left.secondary.as_ref().map(|window| window.used_percent),
            right.secondary.as_ref().map(|window| window.used_percent)
        );
        assert_eq!(
            left.tertiary.as_ref().map(|window| window.used_percent),
            right.tertiary.as_ref().map(|window| window.used_percent)
        );
    }

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
    fn oauth_and_cli_fixtures_produce_same_usage_windows() {
        let oauth_payload: ClaudeUsageResponse =
            serde_json::from_str(include_str!("../../fixtures/claude/usage_oauth.json")).unwrap();
        let oauth_snapshot = normalize(oauth_payload, Some("pro".to_string())).unwrap();
        let cli_snapshot =
            claude_cli::parse_usage_snapshot(include_str!("../../fixtures/claude/usage_cli.txt"))
                .unwrap();

        assert_usage_equivalent(&oauth_snapshot, &cli_snapshot);
    }
}
