// SPDX-License-Identifier: MPL-2.0

pub mod account;
pub mod login;
pub mod opencode;
pub mod storage;

use crate::config::{Config, ManagedKimiAccountConfig};
use crate::error::KimiError;
use crate::model::{
    ProviderCost, ProviderId, ProviderIdentity, UsageHeadline, UsageSnapshot, UsageWindow,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

pub use storage::load_api_key;

const KIMI_API_URL: &str = "https://api.kimi.com/coding/v1/usages";
const MOONSHOT_API_URL: &str = "https://api.moonshot.ai/v1/users/me/balance";
const KIMI_API_KEY_ENV: &str = "KIMI_API_KEY";
const WEEK_SECONDS: i64 = 7 * 24 * 3600;

#[derive(Debug, Deserialize)]
struct KimiUsageResponse {
    usage: Option<KimiUsage>,
    limits: Option<Vec<KimiLimit>>,
    user: Option<KimiUser>,
    error: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct KimiUsage {
    limit: Option<String>,
    used: Option<String>,
    remaining: Option<String>,
    #[serde(rename = "resetTime")]
    reset_time: Option<String>,
}

#[derive(Debug, Deserialize)]
struct KimiLimit {
    detail: Option<KimiUsage>,
    window: Option<KimiWindow>,
}

#[derive(Debug, Deserialize)]
struct KimiWindow {
    duration: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct KimiUser {
    membership: Option<KimiMembership>,
}

#[derive(Debug, Deserialize)]
struct KimiMembership {
    level: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MoonshotBalanceResponse {
    data: Option<MoonshotBalanceData>,
    status: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct MoonshotBalanceData {
    available_balance: Option<f64>,
}

pub fn sync_managed_accounts(config: &mut Config) -> bool {
    let original_len = config.kimi_managed_accounts.len();
    config.kimi_managed_accounts.retain(|account| {
        account.api_key_source.starts_with("env:") || !account.api_key_source.is_empty()
    });
    config.kimi_managed_accounts.len() != original_len
}

pub async fn fetch(
    client: &reqwest::Client,
    account: &ManagedKimiAccountConfig,
) -> Result<UsageSnapshot, KimiError> {
    let api_key = load_api_key(&account.id)
        .ok()
        .filter(|key| !key.is_empty())
        .or_else(|| std::env::var(KIMI_API_KEY_ENV).ok())
        .filter(|key| !key.is_empty())
        .ok_or(KimiError::LoginRequired)?;

    match fetch_kimi(client, &api_key).await {
        Ok(snapshot) => Ok(snapshot),
        Err(KimiError::LoginRequired | KimiError::NoUsageData) => {
            fetch_moonshot(client, &api_key).await
        }
        Err(error) => Err(error),
    }
}

async fn fetch_kimi(client: &reqwest::Client, api_key: &str) -> Result<UsageSnapshot, KimiError> {
    let response = client
        .get(KIMI_API_URL)
        .header(reqwest::header::ACCEPT, "application/json")
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(KimiError::UsageRequest)?;

    handle_status(&response)?;

    let response = response
        .error_for_status()
        .map_err(KimiError::UsageEndpoint)?;
    let body = response.text().await.map_err(KimiError::UsageEndpoint)?;
    parse(&body, Utc::now())
}

async fn fetch_moonshot(
    client: &reqwest::Client,
    api_key: &str,
) -> Result<UsageSnapshot, KimiError> {
    let response = client
        .get(MOONSHOT_API_URL)
        .header(reqwest::header::ACCEPT, "application/json")
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(KimiError::UsageRequest)?;

    handle_status(&response)?;

    let response = response
        .error_for_status()
        .map_err(KimiError::UsageEndpoint)?;
    let body = response.text().await.map_err(KimiError::UsageEndpoint)?;
    parse_moonshot(&body, Utc::now())
}

fn handle_status(response: &reqwest::Response) -> Result<(), KimiError> {
    match response.status() {
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
            Err(KimiError::LoginRequired)
        }
        reqwest::StatusCode::TOO_MANY_REQUESTS => {
            let retry_after_secs = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse().ok());
            Err(KimiError::RateLimited { retry_after_secs })
        }
        status if status.is_server_error() => Err(KimiError::UsageHttp {
            status: status.as_u16(),
        }),
        _ => Ok(()),
    }
}

pub fn parse(body: &str, updated_at: DateTime<Utc>) -> Result<UsageSnapshot, KimiError> {
    let response: KimiUsageResponse = serde_json::from_str(body).map_err(KimiError::DecodeUsage)?;
    if let Some(error) = response.error {
        return Err(KimiError::ApiError {
            message: error.to_string(),
        });
    }

    let mut windows = Vec::new();
    if let Some(usage) = response.usage.as_ref()
        && let Some(used_percent) = used_percent(usage)
    {
        windows.push(window(
            "Weekly".to_string(),
            used_percent,
            Some(WEEK_SECONDS),
            usage.reset_time.as_deref(),
        ));
    }
    if let Some(limits) = response.limits.as_ref() {
        for limit in limits {
            let Some(detail) = limit.detail.as_ref() else {
                continue;
            };
            let Some(duration_minutes) = limit.window.as_ref().and_then(|window| window.duration)
            else {
                continue;
            };
            let Some(used_percent) = used_percent(detail) else {
                continue;
            };
            windows.push(window(
                format!("Rate Limit ({duration_minutes}m)"),
                used_percent,
                duration_minutes.checked_mul(60),
                detail.reset_time.as_deref(),
            ));
        }
    }
    if windows.is_empty() {
        return Err(KimiError::NoUsageData);
    }

    let plan = response
        .user
        .and_then(|user| user.membership)
        .and_then(|membership| membership.level);
    Ok(UsageSnapshot {
        provider: ProviderId::Kimi,
        source: "API Key".to_string(),
        updated_at,
        headline: UsageHeadline(0),
        windows,
        provider_cost: None,
        extra_usage: None,
        identity: ProviderIdentity {
            email: None,
            account_id: None,
            plan,
            display_name: None,
        },
    })
}

pub fn parse_moonshot(body: &str, updated_at: DateTime<Utc>) -> Result<UsageSnapshot, KimiError> {
    let response: MoonshotBalanceResponse =
        serde_json::from_str(body).map_err(KimiError::DecodeUsage)?;

    if response.status == Some(false) {
        return Err(KimiError::NoUsageData);
    }

    let available_balance = response
        .data
        .as_ref()
        .and_then(|data| data.available_balance)
        .filter(|balance| *balance >= f64::EPSILON)
        .ok_or(KimiError::NoUsageData)?;

    const CREDITS_LIMIT: f64 = 100.0;
    let used_percent = (available_balance / CREDITS_LIMIT * 100.0) as f32;

    Ok(UsageSnapshot {
        provider: ProviderId::Kimi,
        source: "Moonshot API Key".to_string(),
        updated_at,
        headline: UsageHeadline(0),
        windows: vec![UsageWindow {
            label: "Credits".to_string(),
            used_percent: used_percent.clamp(0.0, 100.0),
            reset_at: None,
            window_seconds: None,
            reset_description: None,
            group: None,
        }],
        provider_cost: Some(ProviderCost {
            used: available_balance,
            limit: Some(CREDITS_LIMIT),
            units: "USD".to_string(),
        }),
        extra_usage: None,
        identity: ProviderIdentity {
            email: None,
            account_id: None,
            plan: Some("Moonshot".to_string()),
            display_name: None,
        },
    })
}

fn used_percent(usage: &KimiUsage) -> Option<f32> {
    let limit = usage.limit.as_deref()?.parse::<f32>().ok()?;
    if limit <= 0.0 {
        return None;
    }
    let percent = if let Some(used) = usage
        .used
        .as_deref()
        .and_then(|used| used.parse::<f32>().ok())
    {
        used / limit * 100.0
    } else {
        let remaining = usage
            .remaining
            .as_deref()
            .and_then(|remaining| remaining.parse::<f32>().ok())?;
        100.0 - remaining / limit * 100.0
    };
    Some(percent.clamp(0.0, 100.0))
}

fn window(
    label: String,
    used_percent: f32,
    window_seconds: Option<i64>,
    reset_time: Option<&str>,
) -> UsageWindow {
    let reset_at = reset_time
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc));
    UsageWindow {
        label,
        used_percent,
        reset_description: reset_at.map(|value| value.to_rfc3339()),
        reset_at,
        window_seconds,
        group: None,
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
    fn parses_used_usage_and_rate_limit_windows() {
        let snapshot = parse(
            r#"{
                "usage":{"limit":"100","used":"40","resetTime":"2026-08-05T06:21:48Z"},
                "limits":[{"window":{"duration":300,"timeUnit":"TIME_UNIT_MINUTE"},"detail":{"limit":"100","used":"15","resetTime":"2026-08-04T07:21:48Z"}}],
                "user":{"membership":{"level":"LEVEL_INTERMEDIATE"}}
            }"#,
            updated_at(),
        )
        .unwrap();

        assert_eq!(snapshot.provider, ProviderId::Kimi);
        assert_eq!(snapshot.windows.len(), 2);
        assert_eq!(snapshot.windows[0].label, "Weekly");
        assert_eq!(snapshot.windows[0].used_percent, 40.0);
        assert!(snapshot.windows[0].reset_at.is_some());
        assert_eq!(snapshot.windows[0].window_seconds, Some(7 * 24 * 3600));
        assert_eq!(snapshot.windows[1].label, "Rate Limit (300m)");
        assert!((snapshot.windows[1].used_percent - 15.0).abs() < 0.001);
        assert_eq!(snapshot.windows[1].window_seconds, Some(18_000));
        assert_eq!(
            snapshot.identity.plan.as_deref(),
            Some("LEVEL_INTERMEDIATE")
        );
    }

    #[test]
    fn parses_remaining_only_windows() {
        let snapshot = parse(
            r#"{
                "usage":{"limit":"100","remaining":"25"},
                "limits":[{"window":{"duration":60},"detail":{"limit":"200","remaining":"50"}}]
            }"#,
            updated_at(),
        )
        .unwrap();

        assert_eq!(snapshot.windows[0].used_percent, 75.0);
        assert_eq!(snapshot.windows[1].used_percent, 75.0);
    }

    #[test]
    fn parses_weekly_usage_without_limits() {
        let snapshot = parse(
            r#"{"usage":{"limit":"100","used":"40","resetTime":"2026-08-05T06:21:48Z"}}"#,
            updated_at(),
        )
        .unwrap();

        assert_eq!(snapshot.windows.len(), 1);
        assert_eq!(snapshot.windows[0].label, "Weekly");
    }

    #[test]
    fn rejects_empty_windows() {
        let result = parse(
            r#"{"usage":{"limit":"0","used":"0"},"limits":[]}"#,
            updated_at(),
        );

        assert!(matches!(result, Err(KimiError::NoUsageData)));
    }

    #[test]
    fn rejects_invalid_json() {
        let result = parse("not-json", updated_at());

        assert!(matches!(result, Err(KimiError::DecodeUsage(_))));
    }

    #[test]
    fn parses_api_errors() {
        let result = parse(r#"{"error":{"message":"invalid request"}}"#, updated_at());

        assert!(matches!(result, Err(KimiError::ApiError { .. })));
    }

    #[test]
    fn parses_moonshot_balance() {
        let snapshot = parse_moonshot(
            r#"{"code":0,"data":{"available_balance":40.577263,"voucher_balance":0,"cash_balance":40.577263},"scode":"0x0","status":true}"#,
            updated_at(),
        )
        .unwrap();

        assert_eq!(snapshot.provider, ProviderId::Kimi);
        assert_eq!(snapshot.source, "Moonshot API Key");
        assert_eq!(snapshot.windows.len(), 1);
        assert_eq!(snapshot.windows[0].label, "Credits");
        assert!((snapshot.windows[0].used_percent - 40.577263).abs() < 0.001);
        let cost = snapshot.provider_cost.expect("moonshot balance cost");
        assert!((cost.used - 40.577263).abs() < f64::EPSILON * 1000.0);
        assert_eq!(cost.limit, Some(100.0));
        assert_eq!(cost.units, "USD");
        assert_eq!(snapshot.identity.plan.as_deref(), Some("Moonshot"));
    }

    #[test]
    fn rejects_moonshot_failed_status() {
        let result = parse_moonshot(r#"{"code":1,"data":null,"status":false}"#, updated_at());

        assert!(matches!(result, Err(KimiError::NoUsageData)));
    }

    #[test]
    fn rejects_moonshot_zero_balance() {
        let result = parse_moonshot(
            r#"{"code":0,"data":{"available_balance":0},"status":true}"#,
            updated_at(),
        );

        assert!(matches!(result, Err(KimiError::NoUsageData)));
    }

    #[test]
    fn sync_managed_accounts_filters_empty_sources() {
        let now = Utc::now();
        let mut config = Config {
            kimi_managed_accounts: vec![
                ManagedKimiAccountConfig {
                    id: "kimi-1".to_string(),
                    label: "Configured".to_string(),
                    api_key_source: "env:KIMI_API_KEY".to_string(),
                    created_at: now,
                    updated_at: now,
                    last_authenticated_at: None,
                },
                ManagedKimiAccountConfig {
                    id: "kimi-2".to_string(),
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
        assert_eq!(config.kimi_managed_accounts.len(), 1);
        assert_eq!(config.kimi_managed_accounts[0].id, "kimi-1");
    }
}
