// SPDX-License-Identifier: MPL-2.0

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::error::GrokError;
use crate::model::{
    ExtraUsageState, ProviderCost, ProviderId, ProviderIdentity, UsageHeadline, UsageSnapshot,
    UsageWindow,
};

const WEEK_SECONDS: i64 = 604_800;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokBillingResponse {
    pub config: Option<BillingConfig>,
    pub subscription_tier: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BillingConfig {
    pub credit_usage_percent: Option<f32>,
    pub current_period: Option<BillingCurrentPeriod>,
    pub on_demand_cap: Option<NumericVal>,
    pub on_demand_used: Option<NumericVal>,
    pub prepaid_balance: Option<NumericVal>,
    pub product_usage: Option<Vec<ProductUsage>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BillingCurrentPeriod {
    pub end: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NumericVal {
    #[serde(default)]
    pub val: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductUsage {
    pub product: String,
    pub usage_percent: Option<f32>,
}

pub fn parse_billing_snapshot(
    raw_json: &str,
    email: Option<&str>,
    display_name: Option<&str>,
    account_id: Option<&str>,
) -> Result<UsageSnapshot, GrokError> {
    let payload: GrokBillingResponse =
        serde_json::from_str(raw_json).map_err(GrokError::DecodeUsage)?;
    let config = payload.config.ok_or(GrokError::NoUsageData)?;

    let used_percent = extract_used_percent(&config)?;
    let reset_at = parse_optional_reset_timestamp(config.current_period.as_ref())?;
    let reset_description = reset_at.map(|dt| dt.to_rfc3339());

    let window = UsageWindow {
        label: "Weekly".to_string(),
        used_percent,
        reset_at,
        window_seconds: Some(WEEK_SECONDS),
        reset_description,
        group: None,
    };
    let windows = vec![window];

    let provider_cost = extract_prepaid_balance(&config);
    let extra_usage = extract_on_demand_usage(&config);

    let identity = ProviderIdentity {
        email: email.map(str::to_string),
        account_id: account_id.map(str::to_string),
        plan: payload.subscription_tier,
        display_name: display_name.map(str::to_string),
    };

    Ok(UsageSnapshot {
        provider: ProviderId::Grok,
        source: "OAuth".to_string(),
        updated_at: Utc::now(),
        headline: UsageHeadline::first_available(&windows),
        windows,
        provider_cost,
        extra_usage,
        identity,
    })
}

fn extract_used_percent(config: &BillingConfig) -> Result<f32, GrokError> {
    let raw = config
        .credit_usage_percent
        .or_else(|| {
            config.product_usage.as_ref().and_then(|products| {
                products
                    .iter()
                    .find(|product| product.product == "GrokBuild")
                    .and_then(|product| product.usage_percent)
            })
        })
        .ok_or(GrokError::NoUsageData)?;
    Ok(raw.clamp(0.0, 100.0))
}

fn parse_optional_reset_timestamp(
    current_period: Option<&BillingCurrentPeriod>,
) -> Result<Option<DateTime<Utc>>, GrokError> {
    current_period
        .and_then(|period| period.end.as_deref())
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|source| GrokError::InvalidResetTimestamp {
                    value: value.to_string(),
                    source,
                })
        })
        .transpose()
}

fn extract_prepaid_balance(config: &BillingConfig) -> Option<ProviderCost> {
    config
        .prepaid_balance
        .as_ref()
        .filter(|balance| balance.val > 0.0)
        .map(|balance| ProviderCost {
            used: balance.val,
            limit: None,
            units: "credits".to_string(),
        })
}

fn extract_on_demand_usage(config: &BillingConfig) -> Option<ExtraUsageState> {
    let cap = config.on_demand_cap.as_ref()?;
    if cap.val <= 0.0 {
        return None;
    }
    let used = config.on_demand_used.as_ref().map_or(0.0, |used| used.val);
    let ratio = ((used / cap.val) * 100.0) as f32;
    Some(ExtraUsageState::Active {
        used_percent: ratio.clamp(0.0, 100.0),
        cost: ProviderCost {
            used,
            limit: Some(cap.val),
            units: "credits".to_string(),
        },
    })
}
