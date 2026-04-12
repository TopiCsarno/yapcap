use crate::browser::load_cursor_cookie_from_brave;
use crate::config::CursorBrowser;
use crate::error::{CursorError, Result};
use crate::model::{ProviderId, ProviderIdentity, UsageHeadline, UsageSnapshot, UsageWindow};
use chrono::{DateTime, Utc};
use reqwest::header::{COOKIE, HeaderMap, HeaderValue};
use serde::Deserialize;

const USAGE_ENDPOINT: &str = "https://cursor.com/api/usage-summary";
const IDENTITY_ENDPOINT: &str = "https://cursor.com/api/auth/me";

#[derive(Debug, Deserialize)]
struct CursorUsageResponse {
    #[serde(rename = "billingCycleEnd")]
    pub billing_cycle_end: String,
    #[serde(rename = "membershipType")]
    pub membership_type: Option<String>,
    #[serde(rename = "individualUsage")]
    pub individual_usage: CursorIndividualUsage,
}

#[derive(Debug, Deserialize)]
struct CursorIndividualUsage {
    pub plan: CursorPlanUsage,
}

#[derive(Debug, Deserialize)]
struct CursorPlanUsage {
    #[serde(rename = "totalPercentUsed")]
    pub total_percent_used: f64,
    #[serde(rename = "autoPercentUsed")]
    pub auto_percent_used: f64,
    #[serde(rename = "apiPercentUsed")]
    pub api_percent_used: f64,
}

#[derive(Debug, Deserialize)]
struct CursorIdentityResponse {
    pub email: Option<String>,
    pub name: Option<String>,
}

pub async fn fetch(client: &reqwest::Client, browser: CursorBrowser) -> Result<UsageSnapshot> {
    let cookie_db = browser.cookie_db_path()?;
    let cookie_header =
        load_cursor_cookie_from_brave(&cookie_db, browser.keyring_application()).await?;
    let mut headers = HeaderMap::new();
    headers.insert(
        COOKIE,
        HeaderValue::from_str(&cookie_header).map_err(CursorError::InvalidCookieHeader)?,
    );

    let usage_response = client
        .get(USAGE_ENDPOINT)
        .headers(headers.clone())
        .send()
        .await
        .map_err(CursorError::UsageRequest)?;
    if usage_response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(CursorError::Unauthorized.into());
    }
    let usage_response = usage_response
        .error_for_status()
        .map_err(CursorError::UsageEndpoint)?;
    let usage: CursorUsageResponse = usage_response
        .json()
        .await
        .map_err(CursorError::DecodeUsage)?;

    let identity_response = client
        .get(IDENTITY_ENDPOINT)
        .headers(headers)
        .send()
        .await
        .map_err(CursorError::IdentityRequest)?;
    let identity = if identity_response.status().is_success() {
        Some(
            identity_response
                .json::<CursorIdentityResponse>()
                .await
                .map_err(CursorError::DecodeIdentity)?,
        )
    } else {
        None
    };

    normalize(usage, identity)
}

fn normalize(
    usage: CursorUsageResponse,
    identity: Option<CursorIdentityResponse>,
) -> Result<UsageSnapshot> {
    let reset_at = DateTime::parse_from_rfc3339(&usage.billing_cycle_end)
        .map_err(|source| CursorError::InvalidBillingCycleEnd {
            value: usage.billing_cycle_end.clone(),
            source,
        })?
        .with_timezone(&Utc);

    Ok(UsageSnapshot {
        provider: ProviderId::Cursor,
        source: "Browser Cookie".to_string(),
        updated_at: Utc::now(),
        headline: UsageHeadline::Primary,
        primary: Some(window(
            "Total",
            usage.individual_usage.plan.total_percent_used,
            reset_at,
        )),
        secondary: Some(window(
            "Auto + Composer",
            usage.individual_usage.plan.auto_percent_used,
            reset_at,
        )),
        tertiary: Some(window(
            "API",
            usage.individual_usage.plan.api_percent_used,
            reset_at,
        )),
        provider_cost: None,
        identity: ProviderIdentity {
            email: identity.as_ref().and_then(|value| value.email.clone()),
            account_id: None,
            plan: usage.membership_type,
            display_name: identity.and_then(|value| value.name),
        },
    })
}

fn window(label: &str, used_percent: f64, reset_at: DateTime<Utc>) -> UsageWindow {
    UsageWindow {
        label: label.to_string(),
        used_percent,
        reset_at: Some(reset_at),
        reset_description: Some(reset_at.to_rfc3339()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_fixture() {
        let usage: CursorUsageResponse =
            serde_json::from_str(include_str!("../../fixtures/cursor/usage_summary.json")).unwrap();
        let identity: CursorIdentityResponse =
            serde_json::from_str(include_str!("../../fixtures/cursor/auth_me.json")).unwrap();
        let snapshot = normalize(usage, Some(identity)).unwrap();
        assert_eq!(snapshot.provider, ProviderId::Cursor);
        assert_eq!(
            snapshot.primary.as_ref().unwrap().used_percent,
            68.71794871794872
        );
        assert_eq!(
            snapshot.secondary.as_ref().unwrap().used_percent,
            56.333333333333336
        );
        assert_eq!(snapshot.tertiary.as_ref().unwrap().used_percent, 100.0);
        assert_eq!(snapshot.identity.plan.as_deref(), Some("pro"));
    }
}
