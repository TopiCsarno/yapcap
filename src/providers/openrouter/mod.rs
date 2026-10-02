// SPDX-License-Identifier: MPL-2.0

pub mod account;
pub mod login;
pub mod opencode;
pub mod storage;

use crate::config::{Config, ManagedOpenRouterAccountConfig};
use crate::error::OpenRouterError;
use crate::model::{
    ProviderCost, ProviderId, ProviderIdentity, UsageHeadline, UsageSnapshot, UsageWindow,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

pub use storage::load_api_key;

const OPENROUTER_CREDITS_URL: &str = "https://openrouter.ai/api/v1/credits";
const OPENROUTER_KEY_URL: &str = "https://openrouter.ai/api/v1/key";
const OPENROUTER_API_KEY_ENV: &str = "OPENROUTER_API_KEY";

#[derive(Debug, Deserialize)]
struct OpenRouterCreditsResponse {
    data: Option<OpenRouterCredits>,
    error: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterCredits {
    total_credits: Option<f64>,
    total_usage: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterKeyResponse {
    data: Option<OpenRouterKey>,
    error: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterKey {
    #[allow(dead_code)]
    label: Option<String>,
    limit: Option<f64>,
    usage: Option<f64>,
    #[allow(dead_code)]
    limit_remaining: Option<f64>,
    #[allow(dead_code)]
    is_free_tier: Option<bool>,
}

pub fn sync_managed_accounts(config: &mut Config) -> bool {
    let original_len = config.openrouter_managed_accounts.len();
    config.openrouter_managed_accounts.retain(|account| {
        account.api_key_source.starts_with("env:") || !account.api_key_source.is_empty()
    });
    config.openrouter_managed_accounts.len() != original_len
}

pub async fn fetch(
    client: &reqwest::Client,
    account: &ManagedOpenRouterAccountConfig,
) -> Result<UsageSnapshot, OpenRouterError> {
    let api_key = load_api_key(&account.id)
        .ok()
        .filter(|key| !key.is_empty())
        .or_else(|| std::env::var(OPENROUTER_API_KEY_ENV).ok())
        .filter(|key| !key.is_empty())
        .ok_or(OpenRouterError::LoginRequired)?;

    if let Ok(snapshot) = fetch_credits(client, &api_key).await {
        return Ok(snapshot);
    }

    let response = client
        .get(OPENROUTER_KEY_URL)
        .header(reqwest::header::ACCEPT, "application/json")
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(OpenRouterError::UsageRequest)?;

    match response.status() {
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
            return Err(OpenRouterError::LoginRequired);
        }
        reqwest::StatusCode::TOO_MANY_REQUESTS => {
            let retry_after_secs = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse().ok());
            return Err(OpenRouterError::RateLimited { retry_after_secs });
        }
        status if status.is_server_error() => {
            return Err(OpenRouterError::UsageHttp {
                status: status.as_u16(),
            });
        }
        _ => {}
    }

    let response = response
        .error_for_status()
        .map_err(OpenRouterError::UsageEndpoint)?;
    let body = response
        .text()
        .await
        .map_err(OpenRouterError::UsageEndpoint)?;
    parse_key(&body, Utc::now())
}

async fn fetch_credits(
    client: &reqwest::Client,
    api_key: &str,
) -> Result<UsageSnapshot, OpenRouterError> {
    let response = client
        .get(OPENROUTER_CREDITS_URL)
        .header(reqwest::header::ACCEPT, "application/json")
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(OpenRouterError::UsageRequest)?;

    if !response.status().is_success() {
        return Err(OpenRouterError::NoUsageData);
    }

    let body = response
        .text()
        .await
        .map_err(OpenRouterError::UsageEndpoint)?;
    parse(&body, Utc::now())
}

pub fn parse(body: &str, updated_at: DateTime<Utc>) -> Result<UsageSnapshot, OpenRouterError> {
    let response: OpenRouterCreditsResponse =
        serde_json::from_str(body).map_err(OpenRouterError::DecodeUsage)?;
    if let Some(error) = response.error {
        return Err(OpenRouterError::ApiError {
            message: error.to_string(),
        });
    }
    let data = response.data.ok_or(OpenRouterError::NoUsageData)?;
    let total_credits = data.total_credits.ok_or(OpenRouterError::NoUsageData)?;
    let total_usage = data.total_usage.ok_or(OpenRouterError::NoUsageData)?;
    let used_percent = if total_credits <= 0.0 {
        0.0
    } else {
        (total_usage / total_credits * 100.0).clamp(0.0, 100.0) as f32
    };
    let provider_cost = if total_credits > 0.0 {
        Some(ProviderCost {
            used: total_usage,
            limit: Some(total_credits),
            units: "USD".to_string(),
        })
    } else {
        None
    };
    Ok(snapshot(used_percent, provider_cost, updated_at))
}

pub fn parse_key(body: &str, updated_at: DateTime<Utc>) -> Result<UsageSnapshot, OpenRouterError> {
    let response: OpenRouterKeyResponse =
        serde_json::from_str(body).map_err(OpenRouterError::DecodeUsage)?;
    if let Some(error) = response.error {
        return Err(OpenRouterError::ApiError {
            message: error.to_string(),
        });
    }
    let data = response.data.ok_or(OpenRouterError::NoUsageData)?;
    let limit = data.limit.ok_or(OpenRouterError::NoUsageData)?;
    if limit <= 0.0 {
        return Err(OpenRouterError::NoUsageData);
    }
    let usage = data.usage.ok_or(OpenRouterError::NoUsageData)?;
    let used_percent = (usage / limit * 100.0).clamp(0.0, 100.0) as f32;
    let provider_cost = ProviderCost {
        used: usage,
        limit: Some(limit),
        units: "USD".to_string(),
    };
    Ok(snapshot(used_percent, Some(provider_cost), updated_at))
}

fn snapshot(
    used_percent: f32,
    provider_cost: Option<ProviderCost>,
    updated_at: DateTime<Utc>,
) -> UsageSnapshot {
    UsageSnapshot {
        provider: ProviderId::OpenRouter,
        source: "API Key".to_string(),
        updated_at,
        headline: UsageHeadline(0),
        windows: vec![UsageWindow {
            label: "Credits".to_string(),
            used_percent,
            reset_at: None,
            window_seconds: None,
            reset_description: None,
            group: None,
        }],
        provider_cost,
        extra_usage: None,
        identity: ProviderIdentity {
            email: None,
            account_id: None,
            plan: None,
            display_name: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UPDATED_AT: &str = "2026-08-04T06:21:48Z";

    fn updated_at() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(UPDATED_AT)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn parses_credits_usage() {
        let snapshot = parse(
            r#"{"data":{"total_credits":140.0,"total_usage":70.0}}"#,
            updated_at(),
        )
        .unwrap();

        assert_eq!(snapshot.provider, ProviderId::OpenRouter);
        assert_eq!(snapshot.source, "API Key");
        assert_eq!(snapshot.windows.len(), 1);
        assert_eq!(snapshot.windows[0].label, "Credits");
        assert!((snapshot.windows[0].used_percent - 50.0).abs() < 0.001);
        assert_eq!(snapshot.windows[0].window_seconds, None);
        assert_eq!(snapshot.windows[0].reset_at, None);
        assert_eq!(snapshot.windows[0].group, None);
        assert_eq!(snapshot.identity.plan, None);
        let cost = snapshot.provider_cost.expect("credits provider cost");
        assert_eq!(cost.used, 70.0);
        assert_eq!(cost.limit, Some(140.0));
        assert_eq!(cost.units, "USD");
    }

    #[test]
    fn credits_usage_is_clamped_to_a_hundred() {
        let snapshot = parse(
            r#"{"data":{"total_credits":10.0,"total_usage":25.0}}"#,
            updated_at(),
        )
        .unwrap();

        assert!((snapshot.windows[0].used_percent - 100.0).abs() < 0.001);
    }

    #[test]
    fn zero_total_credits_treats_usage_as_zero() {
        let snapshot = parse(
            r#"{"data":{"total_credits":0.0,"total_usage":5.0}}"#,
            updated_at(),
        )
        .unwrap();

        assert_eq!(snapshot.windows[0].used_percent, 0.0);
        assert_eq!(snapshot.provider_cost, None);
    }

    #[test]
    fn parses_key_fallback_usage() {
        let snapshot = parse_key(
            r#"{"data":{"label":"key","limit":100.0,"usage":40.0,"limit_remaining":60.0,"is_free_tier":false}}"#,
            updated_at(),
        )
        .unwrap();

        assert!((snapshot.windows[0].used_percent - 40.0).abs() < 0.001);
        let cost = snapshot.provider_cost.expect("key fallback provider cost");
        assert_eq!(cost.used, 40.0);
        assert_eq!(cost.limit, Some(100.0));
        assert_eq!(cost.units, "USD");
    }

    #[test]
    fn key_fallback_rejects_null_or_zero_limit() {
        assert!(matches!(
            parse_key(r#"{"data":{"limit":null,"usage":1.0}}"#, updated_at()),
            Err(OpenRouterError::NoUsageData)
        ));
        assert!(matches!(
            parse_key(r#"{"data":{"limit":0.0,"usage":1.0}}"#, updated_at()),
            Err(OpenRouterError::NoUsageData)
        ));
    }

    #[test]
    fn rejects_empty_credit_fields() {
        assert!(matches!(
            parse(r#"{"data":{"total_credits":10.0}}"#, updated_at()),
            Err(OpenRouterError::NoUsageData)
        ));
    }

    #[test]
    fn rejects_invalid_json() {
        assert!(matches!(
            parse("not-json", updated_at()),
            Err(OpenRouterError::DecodeUsage(_))
        ));
    }

    #[test]
    fn parses_api_errors() {
        let result = parse(r#"{"error":{"message":"invalid request"}}"#, updated_at());

        assert!(matches!(result, Err(OpenRouterError::ApiError { .. })));
    }

    #[test]
    fn sync_managed_accounts_filters_empty_sources() {
        let now = Utc::now();
        let mut config = Config {
            openrouter_managed_accounts: vec![
                ManagedOpenRouterAccountConfig {
                    id: "openrouter-1".to_string(),
                    label: "Configured".to_string(),
                    api_key_source: "env:OPENROUTER_API_KEY".to_string(),
                    created_at: now,
                    updated_at: now,
                    last_authenticated_at: None,
                },
                ManagedOpenRouterAccountConfig {
                    id: "openrouter-2".to_string(),
                    label: "Empty".to_string(),
                    api_key_source: String::new(),
                    created_at: now,
                    updated_at: now,
                    last_authenticated_at: None,
                },
            ],
            ..Config::default()
        };

        assert!(sync_managed_accounts(&mut config));
        assert_eq!(config.openrouter_managed_accounts.len(), 1);
        assert_eq!(config.openrouter_managed_accounts[0].id, "openrouter-1");
    }
}
