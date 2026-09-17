// SPDX-License-Identifier: MPL-2.0

pub mod account;
pub mod login;
pub mod opencode;
pub mod storage;

use crate::config::{Config, ManagedSyntheticAccountConfig};
use crate::error::SyntheticError;
use crate::model::{
    ProviderCost, ProviderId, ProviderIdentity, UsageHeadline, UsageSnapshot, UsageWindow,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

pub use storage::load_api_key;

const SYNTHETIC_QUOTAS_URL: &str = "https://api.synthetic.new/v2/quotas";
const FIVE_HOUR_SECONDS: i64 = 5 * 3600;
const WEEK_SECONDS: i64 = 7 * 24 * 3600;
const HOUR_SECONDS: i64 = 3600;
const DAY_SECONDS: i64 = 24 * 3600;
const FIVE_HOUR_TICK_SECONDS: i64 = 15 * 60;
const FIVE_HOUR_TICK_FRACTION: f32 = 0.05;
const WEEK_TICK_SECONDS: i64 = 202 * 60;
const WEEK_TICK_FRACTION: f32 = 0.02;

#[derive(Debug, Deserialize)]
struct SyntheticQuotasResponse {
    subscription: Option<WindowQuota>,
    search: Option<SearchQuota>,
    #[serde(rename = "freeToolCalls")]
    free_tool_calls: Option<WindowQuota>,
    #[serde(rename = "weeklyTokenLimit")]
    weekly_token_limit: Option<WeeklyTokenLimit>,
    #[serde(rename = "rollingFiveHourLimit")]
    rolling_five_hour_limit: Option<RollingFiveHourLimit>,
}

#[derive(Debug, Deserialize)]
struct WindowQuota {
    limit: Option<f64>,
    requests: Option<f64>,
    #[serde(rename = "renewsAt")]
    renews_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchQuota {
    hourly: Option<WindowQuota>,
}

#[derive(Debug, Deserialize)]
struct WeeklyTokenLimit {
    #[serde(rename = "nextRegenAt")]
    next_regen_at: Option<String>,
    #[serde(rename = "percentRemaining")]
    percent_remaining: Option<f64>,
    #[serde(rename = "maxCredits")]
    max_credits: Option<String>,
    #[serde(rename = "remainingCredits")]
    remaining_credits: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RollingFiveHourLimit {
    #[serde(rename = "nextTickAt")]
    next_tick_at: Option<String>,
    remaining: Option<f64>,
    max: Option<f64>,
}

pub fn sync_managed_accounts(config: &mut Config) -> bool {
    let original_len = config.synthetic_managed_accounts.len();
    config.synthetic_managed_accounts.retain(|account| {
        account.api_key_source.starts_with("env:") || !account.api_key_source.is_empty()
    });
    config.synthetic_managed_accounts.len() != original_len
}

#[must_use]
pub fn regeneration_tick_for(
    window_seconds: Option<i64>,
) -> Option<crate::model::RegenerationTick> {
    match window_seconds {
        Some(seconds) if seconds == FIVE_HOUR_SECONDS => Some(crate::model::RegenerationTick {
            interval_seconds: FIVE_HOUR_TICK_SECONDS,
            fraction: FIVE_HOUR_TICK_FRACTION,
        }),
        Some(seconds) if seconds == WEEK_SECONDS => Some(crate::model::RegenerationTick {
            interval_seconds: WEEK_TICK_SECONDS,
            fraction: WEEK_TICK_FRACTION,
        }),
        _ => None,
    }
}

pub async fn fetch(
    client: &reqwest::Client,
    account: &ManagedSyntheticAccountConfig,
) -> Result<UsageSnapshot, SyntheticError> {
    let api_key = load_api_key(&account.id)
        .ok()
        .filter(|key| !key.is_empty())
        .ok_or(SyntheticError::LoginRequired)?;

    let response = client
        .get(SYNTHETIC_QUOTAS_URL)
        .header(reqwest::header::ACCEPT, "application/json")
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(SyntheticError::UsageRequest)?;

    match response.status() {
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
            return Err(SyntheticError::LoginRequired);
        }
        status if status.is_server_error() => {
            return Err(SyntheticError::UsageHttp {
                status: status.as_u16(),
            });
        }
        _ => {}
    }

    let response = response
        .error_for_status()
        .map_err(SyntheticError::UsageEndpoint)?;
    let body = response
        .text()
        .await
        .map_err(SyntheticError::UsageEndpoint)?;
    parse(&body, Utc::now())
}

pub fn parse(body: &str, updated_at: DateTime<Utc>) -> Result<UsageSnapshot, SyntheticError> {
    let response: SyntheticQuotasResponse =
        serde_json::from_str(body).map_err(SyntheticError::DecodeUsage)?;

    let mut windows = Vec::new();
    let mut provider_cost = None;

    if let Some(window) = five_hour_window(&response) {
        windows.push(window);
    }
    if let Some((window, cost)) = weekly_credits_window(response.weekly_token_limit.as_ref()) {
        windows.push(window);
        provider_cost = Some(cost);
    }
    if let Some(window) = search_window(response.search.as_ref()) {
        windows.push(window);
    }
    if let Some(window) = tool_calls_window(response.free_tool_calls.as_ref()) {
        windows.push(window);
    }
    if windows.is_empty() {
        return Err(SyntheticError::NoUsageData);
    }

    Ok(UsageSnapshot {
        provider: ProviderId::Synthetic,
        source: "API Key".to_string(),
        updated_at,
        headline: UsageHeadline(0),
        windows,
        provider_cost,
        extra_usage: None,
        identity: ProviderIdentity::default(),
    })
}

fn five_hour_window(response: &SyntheticQuotasResponse) -> Option<UsageWindow> {
    if let Some(rolling) = response.rolling_five_hour_limit.as_ref() {
        let max = rolling.max?;
        if max <= 0.0 {
            return None;
        }
        let remaining = rolling.remaining.unwrap_or(0.0);
        let used_percent = ((max - remaining) / max * 100.0).clamp(0.0, 100.0) as f32;
        return Some(window(
            "5-hour".to_string(),
            used_percent,
            Some(FIVE_HOUR_SECONDS),
            rolling.next_tick_at.as_deref(),
        ));
    }
    let subscription = response.subscription.as_ref()?;
    let limit = subscription.limit?;
    if limit <= 0.0 {
        return None;
    }
    let used = subscription.requests.unwrap_or(0.0);
    let used_percent = (used / limit * 100.0).clamp(0.0, 100.0) as f32;
    Some(window(
        "5-hour".to_string(),
        used_percent,
        Some(FIVE_HOUR_SECONDS),
        subscription.renews_at.as_deref(),
    ))
}

fn weekly_credits_window(weekly: Option<&WeeklyTokenLimit>) -> Option<(UsageWindow, ProviderCost)> {
    let weekly = weekly?;
    let percent_remaining = weekly.percent_remaining?;
    let max = parse_credits(weekly.max_credits.as_deref())?;
    let remaining = parse_credits(weekly.remaining_credits.as_deref())?;
    let used = (max - remaining).max(0.0);
    let used_percent = (100.0 - percent_remaining).clamp(0.0, 100.0) as f32;
    Some((
        window(
            "Weekly credits".to_string(),
            used_percent,
            Some(WEEK_SECONDS),
            weekly.next_regen_at.as_deref(),
        ),
        ProviderCost {
            used,
            limit: Some(max),
            units: "USD".to_string(),
        },
    ))
}

fn search_window(search: Option<&SearchQuota>) -> Option<UsageWindow> {
    let hourly = search?.hourly.as_ref()?;
    let limit = hourly.limit?;
    if limit <= 0.0 {
        return None;
    }
    let used = hourly.requests.unwrap_or(0.0);
    let used_percent = (used / limit * 100.0).clamp(0.0, 100.0) as f32;
    Some(window(
        "Search (1h)".to_string(),
        used_percent,
        Some(HOUR_SECONDS),
        hourly.renews_at.as_deref(),
    ))
}

fn tool_calls_window(free_tool_calls: Option<&WindowQuota>) -> Option<UsageWindow> {
    let quota = free_tool_calls?;
    let limit = quota.limit?;
    if limit <= 0.0 {
        return None;
    }
    let used = quota.requests.unwrap_or(0.0);
    let used_percent = (used / limit * 100.0).clamp(0.0, 100.0) as f32;
    Some(window(
        "Free tool calls (24h)".to_string(),
        used_percent,
        Some(DAY_SECONDS),
        quota.renews_at.as_deref(),
    ))
}

fn parse_credits(value: Option<&str>) -> Option<f64> {
    let value = value?;
    let cleaned: String = value
        .chars()
        .filter(|character| character.is_ascii_digit() || *character == '.')
        .collect();
    cleaned.parse::<f64>().ok()
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

    const UPDATED_AT: &str = "2026-09-14T04:25:00Z";

    fn updated_at() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(UPDATED_AT)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn live_response() -> String {
        r#"{
            "subscription": {
                "limit": 1250,
                "requests": 0,
                "renewsAt": "2026-09-14T08:52:30.235Z"
            },
            "search": {
                "hourly": {
                    "limit": 250,
                    "requests": 1,
                    "renewsAt": "2026-09-14T04:43:59.236Z"
                }
            },
            "freeToolCalls": {
                "limit": 0,
                "requests": 0,
                "renewsAt": "2026-09-15T03:52:30.240Z"
            },
            "weeklyTokenLimit": {
                "nextRegenAt": "2026-09-14T04:25:34.000Z",
                "percentRemaining": 76.91566326666667,
                "maxCredits": "$60.00",
                "remainingCredits": "$46.14",
                "nextRegenCredits": "$1.20"
            },
                "rollingFiveHourLimit": {
                    "nextTickAt": "2026-09-14T04:00:08.000Z",
                    "tickPercent": 0.05,
                    "remaining": 1000,
                    "max": 1250,
                    "limited": false
                }
        }"#
        .to_string()
    }

    #[test]
    fn parses_live_quota_shape_into_windows() {
        let snapshot = parse(&live_response(), updated_at()).unwrap();

        assert_eq!(snapshot.provider, ProviderId::Synthetic);
        assert_eq!(snapshot.source, "API Key");
        assert_eq!(snapshot.windows.len(), 3);

        assert_eq!(snapshot.windows[0].label, "5-hour");
        assert!((snapshot.windows[0].used_percent - 20.0).abs() < 0.001);
        assert_eq!(
            snapshot.windows[0].reset_at.map(|value| value.to_rfc3339()),
            Some("2026-09-14T04:00:08+00:00".to_string())
        );
        assert_eq!(snapshot.windows[0].window_seconds, Some(18_000));

        assert_eq!(snapshot.windows[1].label, "Weekly credits");
        assert!((snapshot.windows[1].used_percent - 23.084).abs() < 0.01);
        assert_eq!(snapshot.windows[1].window_seconds, Some(604_800));

        assert_eq!(snapshot.windows[2].label, "Search (1h)");
        assert!((snapshot.windows[2].used_percent - 0.4).abs() < 0.001);
        assert_eq!(snapshot.windows[2].window_seconds, Some(3_600));

        assert_eq!(
            snapshot.provider_cost,
            Some(ProviderCost {
                used: 13.86,
                limit: Some(60.0),
                units: "USD".to_string()
            })
        );
    }

    #[test]
    fn subscription_fallback_builds_five_hour_window() {
        let snapshot = parse(
            r#"{
                "subscription": {
                    "limit": 500,
                    "requests": 250,
                    "renewsAt": "2026-09-14T08:52:30.235Z"
                }
            }"#,
            updated_at(),
        )
        .unwrap();

        assert_eq!(snapshot.windows.len(), 1);
        assert_eq!(snapshot.windows[0].label, "5-hour");
        assert!((snapshot.windows[0].used_percent - 50.0).abs() < 0.001);
        assert!(snapshot.windows[0].reset_at.is_some());
    }

    #[test]
    fn zero_limit_windows_are_skipped() {
        let snapshot = parse(
            r#"{
                "subscription": {"limit": 0, "requests": 0},
                "freeToolCalls": {"limit": 0, "requests": 0}
            }"#,
            updated_at(),
        );

        assert!(matches!(snapshot, Err(SyntheticError::NoUsageData)));
    }

    #[test]
    fn rejects_invalid_json() {
        let result = parse("not-json", updated_at());

        assert!(matches!(result, Err(SyntheticError::DecodeUsage(_))));
    }

    #[test]
    fn clamps_impossible_remaining_values() {
        let snapshot = parse(
            r#"{
                "rollingFiveHourLimit": {
                    "nextTickAt": "2026-09-14T04:00:08.000Z",
                    "remaining": 1400,
                    "max": 1250
                }
            }"#,
            updated_at(),
        )
        .unwrap();

        assert_eq!(snapshot.windows[0].used_percent, 0.0);
    }

    #[test]
    fn regeneration_tick_matches_window_durations() {
        assert_eq!(
            regeneration_tick_for(Some(FIVE_HOUR_SECONDS)),
            Some(crate::model::RegenerationTick {
                interval_seconds: 900,
                fraction: 0.05,
            })
        );
        assert_eq!(
            regeneration_tick_for(Some(WEEK_SECONDS)),
            Some(crate::model::RegenerationTick {
                interval_seconds: 12_120,
                fraction: 0.02,
            })
        );
        assert_eq!(regeneration_tick_for(Some(HOUR_SECONDS)), None);
        assert_eq!(regeneration_tick_for(None), None);
    }

    #[test]
    fn sync_managed_accounts_filters_empty_sources() {
        let now = Utc::now();
        let mut config = Config {
            synthetic_managed_accounts: vec![
                ManagedSyntheticAccountConfig {
                    id: "synthetic-1".to_string(),
                    label: "Configured".to_string(),
                    api_key_source: "stored".to_string(),
                    created_at: now,
                    updated_at: now,
                    last_authenticated_at: None,
                },
                ManagedSyntheticAccountConfig {
                    id: "synthetic-2".to_string(),
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
        assert_eq!(config.synthetic_managed_accounts.len(), 1);
        assert_eq!(config.synthetic_managed_accounts[0].id, "synthetic-1");
    }
}
