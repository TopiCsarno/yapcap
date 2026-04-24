// SPDX-License-Identifier: MPL-2.0

use crate::browser::{load_cursor_cookie_header_chromium, load_cursor_cookie_header_firefox};
use crate::config::{Browser, CursorCredentialSource, ManagedCursorAccountConfig};
use crate::error::CursorError;
use crate::model::{ProviderId, ProviderIdentity, UsageHeadline, UsageSnapshot, UsageWindow};
#[cfg(debug_assertions)]
use crate::providers::cursor::storage::debug_expired_cookie_override_path;
use crate::providers::cursor::storage::{imported_cookie_header_path, profile_dir};
use chrono::{DateTime, Utc};
use reqwest::header::{COOKIE, HeaderMap, HeaderValue};
use serde::Deserialize;
use std::path::Path;

const USAGE_ENDPOINT: &str = "https://cursor.com/api/usage-summary";
const IDENTITY_ENDPOINT: &str = "https://cursor.com/api/auth/me";

#[derive(Debug, Deserialize)]
struct CursorUsageResponse {
    #[serde(rename = "billingCycleStart")]
    pub billing_cycle_start: String,
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
    pub total: f32,
    #[serde(rename = "autoPercentUsed")]
    pub auto_mode: f32,
    #[serde(rename = "apiPercentUsed")]
    pub api: f32,
}

#[derive(Debug, Deserialize)]
struct CursorIdentityResponse {
    pub email: Option<String>,
    pub name: Option<String>,
}

pub async fn fetch(
    client: &reqwest::Client,
    account: &ManagedCursorAccountConfig,
) -> Result<UsageSnapshot, CursorError> {
    let cookie_header = load_account_cookie_header(account).await?;
    fetch_with_cookie_header(client, &cookie_header).await
}

pub async fn fetch_at(
    client: &reqwest::Client,
    browser: Browser,
    db_path: &Path,
) -> Result<UsageSnapshot, CursorError> {
    let cookie_header = cookie_header_from_db(browser, db_path).await?;
    fetch_with_cookie_header(client, &cookie_header).await
}

async fn load_account_cookie_header(
    account: &ManagedCursorAccountConfig,
) -> Result<String, CursorError> {
    #[cfg(debug_assertions)]
    if let Ok(header) =
        load_imported_cookie_header(&debug_expired_cookie_override_path(&account.account_root))
    {
        return Ok(header);
    }

    if let Ok(header) =
        load_imported_cookie_header(&imported_cookie_header_path(&account.account_root))
    {
        return Ok(header);
    }

    match account.credential_source {
        CursorCredentialSource::ManagedProfile => {
            let browser = account.browser.ok_or(CursorError::Unauthorized)?;
            let cookies_db = profile_dir(&account.account_root)
                .join("Default")
                .join("Cookies");
            if !cookies_db.exists() {
                return Err(CursorError::Unauthorized);
            }
            cookie_header_from_db(browser, &cookies_db).await
        }
        CursorCredentialSource::ImportedBrowserProfile => Err(CursorError::Unauthorized),
    }
}

pub fn load_imported_cookie_header(path: &Path) -> Result<String, CursorError> {
    let header = std::fs::read_to_string(path).map_err(|_| CursorError::Unauthorized)?;
    let header = header.trim();
    if header.is_empty() {
        return Err(CursorError::Unauthorized);
    }
    Ok(header.to_string())
}

pub async fn cookie_header_from_db(
    browser: Browser,
    db_path: &Path,
) -> Result<String, CursorError> {
    match browser.keyring_application() {
        Some(application) => load_cursor_cookie_header_chromium(db_path, application)
            .await
            .map_err(CursorError::Browser),
        None => load_cursor_cookie_header_firefox(db_path).map_err(CursorError::Browser),
    }
}

pub(crate) async fn fetch_with_cookie_header(
    client: &reqwest::Client,
    cookie_header: &str,
) -> Result<UsageSnapshot, CursorError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        COOKIE,
        HeaderValue::from_str(cookie_header).map_err(CursorError::InvalidCookieHeader)?,
    );

    let usage_response = client
        .get(USAGE_ENDPOINT)
        .headers(headers.clone())
        .send()
        .await
        .map_err(CursorError::UsageRequest)?;
    if matches!(
        usage_response.status(),
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
    ) {
        return Err(CursorError::Unauthorized);
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
) -> Result<UsageSnapshot, CursorError> {
    let reset_at = DateTime::parse_from_rfc3339(&usage.billing_cycle_end)
        .map_err(|source| CursorError::InvalidBillingCycleEnd {
            value: usage.billing_cycle_end.clone(),
            source,
        })?
        .with_timezone(&Utc);
    let started_at = DateTime::parse_from_rfc3339(&usage.billing_cycle_start)
        .map_err(|source| CursorError::InvalidBillingCycleEnd {
            value: usage.billing_cycle_start.clone(),
            source,
        })?
        .with_timezone(&Utc);
    let window_seconds = (reset_at - started_at).num_seconds();

    let windows = vec![
        window(
            "Total",
            usage.individual_usage.plan.total,
            reset_at,
            window_seconds,
        ),
        window(
            "Auto + Composer",
            usage.individual_usage.plan.auto_mode,
            reset_at,
            window_seconds,
        ),
        window(
            "API",
            usage.individual_usage.plan.api,
            reset_at,
            window_seconds,
        ),
    ];

    Ok(UsageSnapshot {
        provider: ProviderId::Cursor,
        source: "Managed Account".to_string(),
        updated_at: Utc::now(),
        headline: UsageHeadline::first_available(&windows),
        windows,
        provider_cost: None,
        identity: ProviderIdentity {
            email: identity.as_ref().and_then(|value| value.email.clone()),
            account_id: None,
            plan: usage.membership_type,
            display_name: identity.and_then(|value| value.name),
        },
    })
}

fn window(
    label: &str,
    used_percent: f32,
    reset_at: DateTime<Utc>,
    window_seconds: i64,
) -> UsageWindow {
    UsageWindow {
        label: label.to_string(),
        used_percent,
        reset_at: Some(reset_at),
        window_seconds: Some(window_seconds),
        reset_description: Some(reset_at.to_rfc3339()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_fixture() {
        let usage: CursorUsageResponse =
            serde_json::from_str(include_str!("../../../fixtures/cursor/usage_summary.json"))
                .unwrap();
        let identity: CursorIdentityResponse =
            serde_json::from_str(include_str!("../../../fixtures/cursor/auth_me.json")).unwrap();
        let snapshot = normalize(usage, Some(identity)).unwrap();
        assert_eq!(snapshot.provider, ProviderId::Cursor);
        assert_eq!(snapshot.windows[0].used_percent, 68.717_95);
        assert_eq!(snapshot.windows[1].used_percent, 56.333_332);
        assert_eq!(snapshot.windows[2].used_percent, 100.0);
        assert_eq!(snapshot.identity.plan.as_deref(), Some("pro"));
    }
}
