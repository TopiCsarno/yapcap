// SPDX-License-Identifier: MPL-2.0

use crate::model::UsageWindow;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tracing::warn;

pub const FAMILY_PRO: &str = "Pro";
pub const FAMILY_FLASH: &str = "Flash";
pub const FAMILY_LITE: &str = "Lite";

pub const UNIT_REQUESTS: &str = "Requests";
pub const UNIT_TOKENS: &str = "Tokens";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    Pro,
    Flash,
    Lite,
}

impl Family {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Pro => FAMILY_PRO,
            Self::Flash => FAMILY_FLASH,
            Self::Lite => FAMILY_LITE,
        }
    }

    fn priority(self) -> u8 {
        match self {
            Self::Pro => 0,
            Self::Flash => 1,
            Self::Lite => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenType {
    Requests,
    Tokens,
    Other(String),
}

impl TokenType {
    fn from_api(value: &str) -> Self {
        match value.to_ascii_uppercase().as_str() {
            "REQUESTS" => Self::Requests,
            "TOKENS" => Self::Tokens,
            _ => Self::Other(value.to_string()),
        }
    }

    #[must_use]
    pub fn unit_label(&self) -> &str {
        match self {
            Self::Requests => UNIT_REQUESTS,
            Self::Tokens => UNIT_TOKENS,
            Self::Other(s) => s.as_str(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct RawBucket {
    #[serde(rename = "modelId")]
    pub model_id: String,
    #[serde(rename = "remainingFraction")]
    pub remaining_fraction: f64,
    #[serde(rename = "resetTime")]
    pub reset_time: DateTime<Utc>,
    #[serde(rename = "tokenType", default)]
    pub token_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RetrieveUserQuotaResponse {
    pub buckets: Vec<RawBucket>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FamilyUsageWindow {
    pub family: Family,
    pub used_percent: f32,
    pub reset_at: Option<DateTime<Utc>>,
    pub token_type: TokenType,
}

impl FamilyUsageWindow {
    #[must_use]
    pub fn unit_label(&self) -> &str {
        self.token_type.unit_label()
    }

    #[must_use]
    pub fn to_usage_window(&self) -> UsageWindow {
        UsageWindow {
            label: self.family.label().to_string(),
            used_percent: self.used_percent,
            reset_at: self.reset_at,
            window_seconds: None,
            reset_description: None,
        }
    }
}

fn classify_model(model_id: &str) -> Option<Family> {
    let lower = model_id.to_ascii_lowercase();
    if lower.contains("flash-lite") {
        Some(Family::Lite)
    } else if lower.contains("flash") {
        Some(Family::Flash)
    } else if lower.contains("pro") {
        Some(Family::Pro)
    } else {
        None
    }
}

pub fn classify_buckets(
    response: &RetrieveUserQuotaResponse,
    current_tier_id: &str,
    now: DateTime<Utc>,
) -> Vec<FamilyUsageWindow> {
    let mut groups: Vec<(Family, Vec<RawBucket>)> = Vec::new();
    for bucket in &response.buckets {
        match classify_model(&bucket.model_id) {
            Some(family) => {
                if let Some((_, items)) = groups.iter_mut().find(|(f, _)| *f == family) {
                    items.push(bucket.clone());
                } else {
                    groups.push((family, vec![bucket.clone()]));
                }
            }
            None => {
                warn!(model_id = %bucket.model_id, "gemini: unknown model id, skipping");
            }
        }
    }

    let mut out: Vec<FamilyUsageWindow> = Vec::new();
    for (family, buckets) in groups {
        if family == Family::Pro && current_tier_id == "free-tier" {
            continue;
        }

        let representative = buckets
            .iter()
            .min_by(|a, b| {
                a.remaining_fraction
                    .partial_cmp(&b.remaining_fraction)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .expect("group has at least one bucket");

        let min_remaining = representative.remaining_fraction;

        let future_reset = if representative.reset_time > now {
            Some(representative.reset_time)
        } else {
            buckets
                .iter()
                .find_map(|b| (b.reset_time > now).then_some(b.reset_time))
        };

        let reset_at = future_reset.or_else(|| {
            if representative.reset_time > now {
                Some(representative.reset_time)
            } else {
                None
            }
        });

        if min_remaining <= f64::EPSILON && reset_at.is_none() {
            continue;
        }

        let used_percent = ((1.0 - min_remaining) * 100.0).clamp(0.0, 100.0) as f32;
        let token_type = representative
            .token_type
            .as_deref()
            .map_or(TokenType::Other(String::new()), TokenType::from_api);

        out.push(FamilyUsageWindow {
            family,
            used_percent,
            reset_at,
            token_type,
        });
    }

    out.sort_by_key(|w| w.family.priority());
    out
}

pub fn classify_usage_windows(
    response: &RetrieveUserQuotaResponse,
    current_tier_id: &str,
    now: DateTime<Utc>,
) -> Vec<UsageWindow> {
    classify_buckets(response, current_tier_id, now)
        .iter()
        .map(FamilyUsageWindow::to_usage_window)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 14, 12, 0, 0).unwrap()
    }

    fn load_free_tier_fixture() -> RetrieveUserQuotaResponse {
        let raw = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/gemini/retrieve_user_quota_response.json"
        ))
        .expect("fixture exists");
        let outer: serde_json::Value = serde_json::from_str(&raw).expect("valid json");
        serde_json::from_value(outer["body_json"].clone()).expect("body_json parses")
    }

    fn bucket(model: &str, remaining: f64, reset: DateTime<Utc>, token_type: &str) -> RawBucket {
        RawBucket {
            model_id: model.to_string(),
            remaining_fraction: remaining,
            reset_time: reset,
            token_type: Some(token_type.to_string()),
        }
    }

    #[test]
    fn free_tier_capture_hides_pro_and_shows_flash_and_lite() {
        let response = load_free_tier_fixture();
        let out = classify_buckets(&response, "free-tier", now());
        let families: Vec<_> = out.iter().map(|w| w.family).collect();
        assert_eq!(families, vec![Family::Flash, Family::Lite]);
        let flash = &out[0];
        assert!((flash.used_percent - (100.0 - 82.666_664)).abs() < 0.01);
        assert!(flash.reset_at.is_some());
    }

    #[test]
    fn paid_tier_handcrafted_shows_all_three_families() {
        let future = now() + chrono::Duration::hours(12);
        let response = RetrieveUserQuotaResponse {
            buckets: vec![
                bucket("gemini-3-pro-preview", 0.5, future, "REQUESTS"),
                bucket("gemini-3-flash-preview", 0.75, future, "REQUESTS"),
                bucket("gemini-3.1-flash-lite-preview", 0.9, future, "REQUESTS"),
            ],
        };
        let out = classify_buckets(&response, "standard-tier", now());
        let families: Vec<_> = out.iter().map(|w| w.family).collect();
        assert_eq!(families, vec![Family::Pro, Family::Flash, Family::Lite]);
    }

    #[test]
    fn exhausted_flash_with_future_reset_is_visible_at_zero_remaining() {
        let future = now() + chrono::Duration::hours(8);
        let response = RetrieveUserQuotaResponse {
            buckets: vec![bucket("gemini-2.5-flash", 0.0, future, "REQUESTS")],
        };
        let out = classify_buckets(&response, "standard-tier", now());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].family, Family::Flash);
        assert!((out[0].used_percent - 100.0).abs() < 0.001);
        assert_eq!(out[0].reset_at, Some(future));
    }

    #[test]
    fn pro_at_epoch_zero_on_free_tier_is_hidden() {
        let epoch = Utc.with_ymd_and_hms(1970, 1, 1, 0, 0, 0).unwrap();
        let response = RetrieveUserQuotaResponse {
            buckets: vec![
                bucket("gemini-2.5-pro", 0.0, epoch, "REQUESTS"),
                bucket(
                    "gemini-2.5-flash",
                    0.8,
                    now() + chrono::Duration::hours(1),
                    "REQUESTS",
                ),
            ],
        };
        let out = classify_buckets(&response, "free-tier", now());
        let families: Vec<_> = out.iter().map(|w| w.family).collect();
        assert_eq!(families, vec![Family::Flash]);
    }

    #[test]
    fn pro_at_epoch_zero_on_standard_tier_is_also_hidden() {
        let epoch = Utc.with_ymd_and_hms(1970, 1, 1, 0, 0, 0).unwrap();
        let response = RetrieveUserQuotaResponse {
            buckets: vec![bucket("gemini-2.5-pro", 0.0, epoch, "REQUESTS")],
        };
        let out = classify_buckets(&response, "standard-tier", now());
        assert!(out.is_empty());
    }

    #[test]
    fn multi_version_family_picks_lowest_remaining() {
        let future = now() + chrono::Duration::hours(4);
        let response = RetrieveUserQuotaResponse {
            buckets: vec![
                bucket("gemini-2.5-flash", 0.9, future, "REQUESTS"),
                bucket("gemini-3-flash-preview", 0.2, future, "REQUESTS"),
            ],
        };
        let out = classify_buckets(&response, "standard-tier", now());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].family, Family::Flash);
        assert!((out[0].used_percent - 80.0).abs() < 0.01);
    }

    #[test]
    fn unknown_model_id_is_dropped_without_panic() {
        let future = now() + chrono::Duration::hours(1);
        let response = RetrieveUserQuotaResponse {
            buckets: vec![
                bucket("gemini-experimental-mystery", 0.5, future, "REQUESTS"),
                bucket("gemini-2.5-flash", 0.5, future, "REQUESTS"),
            ],
        };
        let out = classify_buckets(&response, "standard-tier", now());
        let families: Vec<_> = out.iter().map(|w| w.family).collect();
        assert_eq!(families, vec![Family::Flash]);
    }

    #[test]
    fn token_type_propagates_into_usage_window_unit_label() {
        let future = now() + chrono::Duration::hours(1);
        let response = RetrieveUserQuotaResponse {
            buckets: vec![
                bucket("gemini-2.5-flash", 0.5, future, "REQUESTS"),
                bucket("gemini-2.5-flash-lite", 0.5, future, "TOKENS"),
            ],
        };
        let out = classify_buckets(&response, "standard-tier", now());
        let flash = out.iter().find(|w| w.family == Family::Flash).unwrap();
        let lite = out.iter().find(|w| w.family == Family::Lite).unwrap();
        assert_eq!(flash.unit_label(), UNIT_REQUESTS);
        assert_eq!(lite.unit_label(), UNIT_TOKENS);
        let flash_uw = flash.to_usage_window();
        let lite_uw = lite.to_usage_window();
        assert_eq!(flash_uw.label, FAMILY_FLASH);
        assert_eq!(flash_uw.reset_description, None);
        assert_eq!(lite_uw.label, FAMILY_LITE);
    }

    #[test]
    fn can_return_plain_usage_windows_in_priority_order() {
        let future = now() + chrono::Duration::hours(2);
        let response = RetrieveUserQuotaResponse {
            buckets: vec![
                bucket("gemini-2.5-flash-lite", 0.9, future, "REQUESTS"),
                bucket("gemini-2.5-pro", 0.7, future, "REQUESTS"),
                bucket("gemini-2.5-flash", 0.8, future, "REQUESTS"),
            ],
        };
        let windows = classify_usage_windows(&response, "standard-tier", now());
        let labels: Vec<_> = windows.iter().map(|w| w.label.as_str()).collect();
        assert_eq!(labels, vec![FAMILY_PRO, FAMILY_FLASH, FAMILY_LITE]);
    }

    #[test]
    fn flash_lite_does_not_match_flash_family() {
        assert_eq!(classify_model("gemini-2.5-flash-lite"), Some(Family::Lite));
        assert_eq!(classify_model("gemini-2.5-flash"), Some(Family::Flash));
        assert_eq!(classify_model("gemini-2.5-pro"), Some(Family::Pro));
        assert_eq!(classify_model("GEMINI-3-PRO-PREVIEW"), Some(Family::Pro));
        assert_eq!(classify_model("gemini-experimental"), None);
    }
}
