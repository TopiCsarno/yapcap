// SPDX-License-Identifier: MPL-2.0

use super::USER_AGENT;
use crate::error::AntigravityError;
use crate::model::UsageWindow;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;

pub const WEEKLY_WINDOW_SECONDS: i64 = 7 * 24 * 3600;
pub const FIVE_HOUR_WINDOW_SECONDS: i64 = 5 * 3600;

#[derive(Debug, Deserialize)]
pub struct QuotaSummaryResponse {
    #[serde(default)]
    pub groups: Vec<RawGroup>,
}

#[derive(Debug, Deserialize)]
pub struct RawGroup {
    #[serde(default, rename = "displayName")]
    pub display_name: String,
    #[serde(default)]
    pub buckets: Vec<RawBucket>,
}

#[derive(Debug, Deserialize)]
pub struct RawBucket {
    #[serde(default, rename = "displayName")]
    pub display_name: String,
    #[serde(default)]
    pub window: Option<String>,
    #[serde(default, rename = "remainingFraction")]
    pub remaining_fraction: f64,
    #[serde(default, rename = "resetTime")]
    pub reset_time: Option<DateTime<Utc>>,
}

fn window_rank(window: Option<&str>) -> u8 {
    match window {
        Some("5h") => 0,
        Some("weekly") => 1,
        _ => 2,
    }
}

fn window_seconds(window: Option<&str>) -> Option<i64> {
    match window {
        Some("weekly") => Some(WEEKLY_WINDOW_SECONDS),
        Some("5h") => Some(FIVE_HOUR_WINDOW_SECONDS),
        _ => None,
    }
}

#[must_use]
pub fn normalize_quota_summary(response: &QuotaSummaryResponse) -> Vec<UsageWindow> {
    let mut windows = Vec::new();
    for group in &response.groups {
        let mut buckets: Vec<&RawBucket> = group.buckets.iter().collect();
        buckets.sort_by_key(|bucket| window_rank(bucket.window.as_deref()));
        for bucket in buckets {
            let used_percent = ((1.0 - bucket.remaining_fraction) * 100.0).clamp(0.0, 100.0) as f32;
            windows.push(UsageWindow {
                label: bucket
                    .display_name
                    .strip_suffix(" Remaining")
                    .unwrap_or(&bucket.display_name)
                    .to_string(),
                used_percent,
                reset_at: bucket.reset_time,
                window_seconds: window_seconds(bucket.window.as_deref()),
                reset_description: None,
                group: Some(group.display_name.clone()),
            });
        }
    }
    windows
}

pub async fn retrieve_user_quota_summary_typed(
    client: &reqwest::Client,
    endpoint: &str,
    access_token: &str,
    project: Option<&str>,
) -> Result<QuotaSummaryResponse, AntigravityError> {
    let body = match project {
        Some(project) => json!({ "project": project }),
        None => json!({}),
    };
    let response = client
        .post(endpoint)
        .bearer_auth(access_token)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .header("User-Agent", USER_AGENT)
        .json(&body)
        .send()
        .await
        .map_err(AntigravityError::QuotaRequest)?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(AntigravityError::Unauthorized);
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let retry_after_secs = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        return Err(AntigravityError::RateLimited { retry_after_secs });
    }
    if !status.is_success() {
        return Err(AntigravityError::QuotaHttp {
            status: status.as_u16(),
        });
    }
    let body = response
        .text()
        .await
        .map_err(|error| AntigravityError::QuotaParse(error.to_string()))?;
    parse_quota_summary(&body)
}

pub fn parse_quota_summary(raw: &str) -> Result<QuotaSummaryResponse, AntigravityError> {
    serde_json::from_str(raw).map_err(|error| AntigravityError::QuotaParse(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_fixture_named(name: &str) -> QuotaSummaryResponse {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/antigravity")
            .join(name);
        let raw = std::fs::read_to_string(path).expect("fixture exists");
        let outer: serde_json::Value = serde_json::from_str(&raw).expect("valid json");
        serde_json::from_value(outer["body_json"].clone()).expect("body_json parses")
    }

    fn load_fixture() -> QuotaSummaryResponse {
        load_fixture_named("retrieve_user_quota_response.json")
    }

    #[test]
    fn normalizes_fixture_into_four_grouped_windows() {
        let response = load_fixture();
        let windows = normalize_quota_summary(&response);
        assert_eq!(windows.len(), 4);

        assert_eq!(windows[0].group.as_deref(), Some("Gemini Models"));
        assert_eq!(windows[0].label, "Five Hour Limit");
        assert_eq!(windows[0].window_seconds, Some(FIVE_HOUR_WINDOW_SECONDS));

        assert_eq!(windows[1].group.as_deref(), Some("Gemini Models"));
        assert_eq!(windows[1].label, "Weekly Limit");
        assert_eq!(windows[1].window_seconds, Some(WEEKLY_WINDOW_SECONDS));
        assert!((windows[1].used_percent - (100.0 - 99.77627)).abs() < 0.01);
        assert!(windows[1].reset_at.is_some());

        assert_eq!(windows[2].group.as_deref(), Some("Claude and GPT models"));
        assert_eq!(windows[2].label, "Five Hour Limit");
        assert_eq!(windows[2].window_seconds, Some(FIVE_HOUR_WINDOW_SECONDS));

        assert_eq!(windows[3].group.as_deref(), Some("Claude and GPT models"));
        assert_eq!(windows[3].label, "Weekly Limit");
        assert!((windows[3].used_percent - 0.0).abs() < 0.01);
    }

    #[test]
    fn normalizes_free_tier_fixture_into_two_weekly_windows() {
        let response = load_fixture_named("retrieve_user_quota_free_response.json");
        let windows = normalize_quota_summary(&response);
        assert_eq!(windows.len(), 2);

        assert_eq!(windows[0].group.as_deref(), Some("Gemini Models"));
        assert_eq!(windows[0].label, "Weekly Limit");
        assert_eq!(windows[0].window_seconds, Some(WEEKLY_WINDOW_SECONDS));
        assert!((windows[0].used_percent - (100.0 - 94.70576)).abs() < 0.01);
        assert!(windows[0].reset_at.is_some());

        assert_eq!(windows[1].group.as_deref(), Some("Claude and GPT models"));
        assert_eq!(windows[1].label, "Weekly Limit");
        assert_eq!(windows[1].window_seconds, Some(WEEKLY_WINDOW_SECONDS));
        assert!((windows[1].used_percent - (100.0 - 93.8137)).abs() < 0.01);

        assert!(
            windows
                .iter()
                .all(|window| window.window_seconds != Some(FIVE_HOUR_WINDOW_SECONDS))
        );
    }

    #[test]
    fn five_hour_ordered_before_weekly_regardless_of_input_order() {
        let raw = r#"{"groups":[{"displayName":"G","buckets":[
            {"displayName":"Weekly Limit","window":"weekly","remainingFraction":0.9,"resetTime":"2026-07-21T15:00:00Z"},
            {"displayName":"Five Hour Limit","window":"5h","remainingFraction":0.5,"resetTime":"2026-07-14T15:00:00Z"}
        ]}]}"#;
        let response = parse_quota_summary(raw).unwrap();
        let windows = normalize_quota_summary(&response);
        assert_eq!(windows[0].label, "Five Hour Limit");
        assert_eq!(windows[1].label, "Weekly Limit");
    }

    #[test]
    fn strips_trailing_remaining_from_labels() {
        let raw = r#"{"groups":[{"displayName":"G","buckets":[
            {"displayName":"Five Hour Limit Remaining","window":"5h","remainingFraction":0.8},
            {"displayName":"Gemini 4 Argon Weekly Limit","window":"weekly","remainingFraction":0.9}
        ]}]}"#;
        let response = parse_quota_summary(raw).unwrap();
        let windows = normalize_quota_summary(&response);
        assert_eq!(windows[0].label, "Five Hour Limit");
        assert_eq!(windows[1].label, "Gemini 4 Argon Weekly Limit");
    }

    #[test]
    fn unknown_window_leaves_window_seconds_none() {
        let raw = r#"{"groups":[{"displayName":"G","buckets":[
            {"displayName":"Odd","window":"monthly","remainingFraction":0.5}
        ]}]}"#;
        let response = parse_quota_summary(raw).unwrap();
        let windows = normalize_quota_summary(&response);
        assert_eq!(windows[0].window_seconds, None);
        assert_eq!(windows[0].reset_at, None);
    }

    #[test]
    fn empty_groups_yield_no_windows() {
        let response = parse_quota_summary(r#"{"groups":[]}"#).unwrap();
        assert!(normalize_quota_summary(&response).is_empty());
    }
}
