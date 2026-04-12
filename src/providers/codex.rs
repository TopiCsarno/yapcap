use crate::auth::load_codex_auth;
use crate::error::{CodexError, Result};
use crate::model::{
    ProviderCost, ProviderId, ProviderIdentity, UsageHeadline, UsageSnapshot, UsageWindow,
};
use chrono::{DateTime, Utc};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Deserialize;
use tracing::warn;

const ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";

#[derive(Debug, Deserialize)]
struct CodexUsageResponse {
    pub account_id: Option<String>,
    pub email: Option<String>,
    pub plan_type: Option<String>,
    pub rate_limit: Option<CodexRateLimit>,
    pub credits: Option<CodexCredits>,
}

#[derive(Debug, Deserialize)]
struct CodexRateLimit {
    pub primary_window: Option<CodexWindow>,
    pub secondary_window: Option<CodexWindow>,
}

#[derive(Debug, Deserialize)]
struct CodexWindow {
    pub used_percent: f64,
    pub reset_at: i64,
}

#[derive(Debug, Deserialize)]
struct CodexCredits {
    pub balance: String,
}

pub async fn fetch(client: &reqwest::Client) -> Result<UsageSnapshot> {
    let auth = load_codex_auth()?;
    let mut headers = HeaderMap::new();
    let bearer = format!("Bearer {}", auth.access_token);
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&bearer).map_err(CodexError::InvalidBearerHeader)?,
    );
    if let Some(account_id) = &auth.account_id {
        headers.insert(
            "ChatGPT-Account-Id",
            HeaderValue::from_str(account_id).map_err(CodexError::InvalidAccountIdHeader)?,
        );
    }
    let response = client
        .get(ENDPOINT)
        .headers(headers)
        .send()
        .await
        .map_err(CodexError::UsageRequest)?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(CodexError::Unauthorized.into());
    }
    let response = response
        .error_for_status()
        .map_err(CodexError::UsageEndpoint)?;
    let payload: CodexUsageResponse = response.json().await.map_err(CodexError::DecodeUsage)?;
    normalize(payload)
}

fn normalize(payload: CodexUsageResponse) -> Result<UsageSnapshot> {
    let primary = payload
        .rate_limit
        .as_ref()
        .and_then(|rate| rate.primary_window.as_ref())
        .map(|window| normalize_window("5h", window));
    let secondary = payload
        .rate_limit
        .as_ref()
        .and_then(|rate| rate.secondary_window.as_ref())
        .map(|window| normalize_window("7d", window));
    let headline = if secondary.is_some() {
        UsageHeadline::Secondary
    } else {
        UsageHeadline::Primary
    };
    let provider_cost =
        payload
            .credits
            .as_ref()
            .and_then(|credits| match credits.balance.parse::<f64>() {
                Ok(used) => Some(ProviderCost {
                    used,
                    limit: None,
                    units: "credits".to_string(),
                }),
                Err(error) => {
                    let typed_error = CodexError::InvalidCreditBalance {
                        balance: credits.balance.clone(),
                        source: error,
                    };
                    warn!(
                        balance = %credits.balance,
                        error = %typed_error,
                        "failed to parse codex credit balance"
                    );
                    None
                }
            });

    if primary.is_none() && secondary.is_none() && provider_cost.is_none() {
        return Err(CodexError::NoUsageData.into());
    }

    Ok(UsageSnapshot {
        provider: ProviderId::Codex,
        source: "OAuth".to_string(),
        updated_at: Utc::now(),
        headline,
        primary,
        secondary,
        tertiary: None,
        provider_cost,
        identity: ProviderIdentity {
            email: payload.email,
            account_id: payload.account_id,
            plan: payload.plan_type,
            display_name: None,
        },
    })
}

fn normalize_window(label: &str, window: &CodexWindow) -> UsageWindow {
    let reset_at = DateTime::from_timestamp(window.reset_at, 0);
    UsageWindow {
        label: label.to_string(),
        used_percent: window.used_percent,
        reset_at,
        reset_description: reset_at.map(|time| time.to_rfc3339()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_fixture() {
        let payload: CodexUsageResponse =
            serde_json::from_str(include_str!("../../fixtures/codex/usage_oauth.json")).unwrap();
        let snapshot = normalize(payload).unwrap();
        assert_eq!(snapshot.provider, ProviderId::Codex);
        assert_eq!(snapshot.primary.as_ref().unwrap().used_percent, 3.0);
        assert_eq!(snapshot.secondary.as_ref().unwrap().used_percent, 24.0);
        assert_eq!(snapshot.identity.plan.as_deref(), Some("plus"));
    }

    #[test]
    fn keeps_credits_without_rate_windows() {
        let payload = CodexUsageResponse {
            account_id: Some("acct_123".to_string()),
            email: Some("user@example.com".to_string()),
            plan_type: Some("plus".to_string()),
            rate_limit: None,
            credits: Some(CodexCredits {
                balance: "12.5".to_string(),
            }),
        };
        let snapshot = normalize(payload).unwrap();
        assert!(snapshot.primary.is_none());
        assert!(snapshot.secondary.is_none());
        assert_eq!(snapshot.provider_cost.as_ref().unwrap().used, 12.5);
        assert_eq!(snapshot.identity.account_id.as_deref(), Some("acct_123"));
    }
}
