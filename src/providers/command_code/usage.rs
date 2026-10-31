// SPDX-License-Identifier: MPL-2.0

use super::CommandCodeError;
use crate::model::{
    ProviderCost, ProviderId, ProviderIdentity, UsageHeadline, UsageSnapshot, UsageWindow,
};
use chrono::{DateTime, Utc};
use serde_json::Value;

pub(super) fn parse(
    credits: &Value,
    subscription: Option<&Value>,
    mut identity: ProviderIdentity,
    updated_at: DateTime<Utc>,
) -> Result<UsageSnapshot, CommandCodeError> {
    let windows = windows(credits);
    let provider_cost = extra_balance(credits).ok().map(|balance| ProviderCost {
        used: balance,
        limit: None,
        units: "credits".to_string(),
    });
    if provider_cost.is_none() {
        if windows.is_empty() {
            return Err(CommandCodeError::InvalidResponse);
        }
        tracing::warn!("Command Code credit balance unavailable; keeping valid quota windows");
    }
    identity.plan = subscription.and_then(plan_name);
    Ok(UsageSnapshot {
        provider: ProviderId::CommandCode,
        source: "API Key".to_string(),
        updated_at,
        headline: UsageHeadline(0),
        windows,
        provider_cost,
        extra_usage: None,
        identity,
    })
}

fn plan_name(subscription: &Value) -> Option<String> {
    let data = subscription.get("data")?;
    if !matches!(
        data.get("status").and_then(Value::as_str),
        Some("active" | "trialing" | "past_due")
    ) {
        return None;
    }
    let id = data
        .get("planId")?
        .as_str()?
        .to_ascii_lowercase()
        .replace('_', "-");
    [
        ("individual-provider", "Provider"),
        ("individual-goat", "GOAT"),
        ("individual-ultra", "Ultra"),
        ("individual-max", "Max"),
        ("individual-pro", "Pro"),
        ("individual-go", "Go"),
        ("teams-pro", "Teams Pro"),
    ]
    .into_iter()
    .find_map(|(prefix, name)| {
        (id == prefix
            || id
                .strip_prefix(prefix)
                .is_some_and(|suffix| suffix.starts_with('-')))
        .then(|| name.to_string())
    })
}

fn extra_balance(value: &Value) -> Result<f64, CommandCodeError> {
    let balances = value
        .get("credits")
        .and_then(Value::as_object)
        .ok_or(CommandCodeError::InvalidResponse)?;
    let purchased = balances
        .get("purchasedCredits")
        .and_then(nonnegative_number)
        .ok_or(CommandCodeError::InvalidResponse)?;
    let free = balances
        .get("freeCredits")
        .and_then(nonnegative_number)
        .ok_or(CommandCodeError::InvalidResponse)?;
    let extra = purchased + free;
    if !extra.is_finite() {
        return Err(CommandCodeError::InvalidResponse);
    }
    Ok(extra)
}

fn windows(credits: &Value) -> Vec<UsageWindow> {
    let Some(limits) = credits
        .get("windowLimits")
        .filter(|limits| limits.get("limited").and_then(Value::as_bool) == Some(true))
    else {
        return Vec::new();
    };
    [
        ("fiveHour", "5 Hour", 18_000),
        ("weekly", "Weekly", 604_800),
    ]
    .into_iter()
    .filter_map(|(name, label, seconds)| {
        limits
            .get(name)
            .and_then(|value| window(value, label, seconds))
    })
    .collect()
}

fn nonnegative_number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .filter(|number| number.is_finite() && *number >= 0.0)
}

fn window(value: &Value, label: &str, seconds: i64) -> Option<UsageWindow> {
    let used = value.get("used").and_then(nonnegative_number)?;
    let cap = value
        .get("cap")
        .and_then(nonnegative_number)
        .filter(|cap| *cap > 0.0)?;
    let reset_at = value.get("resetAt").and_then(timestamp);
    Some(UsageWindow {
        label: label.to_string(),
        used_percent: ((used / cap) * 100.0).clamp(0.0, 100.0) as f32,
        reset_description: reset_at.map(|time| time.to_rfc3339()),
        reset_at,
        window_seconds: Some(seconds),
        group: None,
    })
}

fn timestamp(value: &Value) -> Option<DateTime<Utc>> {
    if let Some(text) = value.as_str() {
        return DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|time| time.with_timezone(&Utc))
            .or_else(|| text.parse::<i64>().ok().and_then(epoch));
    }
    value.as_i64().and_then(epoch)
}

fn epoch(value: i64) -> Option<DateTime<Utc>> {
    if value <= 0 {
        None
    } else if value >= 1_000_000_000_000 {
        DateTime::from_timestamp_millis(value)
    } else {
        DateTime::from_timestamp(value, 0)
    }
}
