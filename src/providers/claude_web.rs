use crate::browser::{load_claude_cookie_chromium, load_claude_cookie_firefox};
use crate::config::CursorBrowser;
use crate::error::{ClaudeError, Result};
use crate::model::{
    ProviderCost, ProviderId, ProviderIdentity, UsageHeadline, UsageSnapshot, UsageWindow,
};
use chrono::{DateTime, Utc};
use reqwest::header::{COOKIE, HeaderMap, HeaderValue};
use serde::Deserialize;

const ORGANIZATIONS_ENDPOINT: &str = "https://claude.ai/api/organizations";
const ACCOUNT_ENDPOINT: &str = "https://claude.ai/api/account";

#[derive(Debug, Deserialize)]
struct ClaudeOrganization {
    uuid: String,
}

#[derive(Debug, Deserialize)]
struct ClaudeWebUsageResponse {
    five_hour: Option<ClaudeWebWindow>,
    seven_day: Option<ClaudeWebWindow>,
    extra_usage: Option<ClaudeWebExtraUsage>,
}

#[derive(Debug, Deserialize)]
struct ClaudeWebWindow {
    utilization: f64,
    resets_at: String,
}

#[derive(Debug, Deserialize)]
struct ClaudeWebExtraUsage {
    monthly_limit: f64,
    used_credits: f64,
    utilization: f64,
}

#[derive(Debug, Deserialize)]
struct ClaudeAccountResponse {
    email_address: Option<String>,
    full_name: Option<String>,
    display_name: Option<String>,
    memberships: Vec<ClaudeAccountMembership>,
}

#[derive(Debug, Deserialize)]
struct ClaudeAccountMembership {
    organization: ClaudeAccountOrganization,
}

#[derive(Debug, Deserialize)]
struct ClaudeAccountOrganization {
    capabilities: Vec<String>,
    rate_limit_tier: Option<String>,
}

pub async fn fetch(client: &reqwest::Client, browser: CursorBrowser) -> Result<UsageSnapshot> {
    let cookie_db = browser.cookie_db_path()?;
    let cookie_header = match browser.keyring_application() {
        Some(application) => load_claude_cookie_chromium(&cookie_db, application).await?,
        None => load_claude_cookie_firefox(&cookie_db)?,
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        COOKIE,
        HeaderValue::from_str(&cookie_header).map_err(ClaudeError::InvalidCookieHeader)?,
    );

    let organizations = client
        .get(ORGANIZATIONS_ENDPOINT)
        .headers(headers.clone())
        .send()
        .await
        .map_err(ClaudeError::WebOrganizationsRequest)?;
    if organizations.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(ClaudeError::WebUnauthorized.into());
    }
    let organizations = organizations
        .error_for_status()
        .map_err(ClaudeError::WebOrganizationsEndpoint)?;
    let organizations_json: Vec<ClaudeOrganization> = organizations
        .json()
        .await
        .map_err(ClaudeError::DecodeWebOrganizations)?;
    let organization_id = organizations_json
        .first()
        .map(|organization| organization.uuid.as_str())
        .ok_or(ClaudeError::WebOrganizationMissing)?;

    let usage_endpoint = format!("https://claude.ai/api/organizations/{organization_id}/usage");
    let usage_response = client
        .get(&usage_endpoint)
        .headers(headers.clone())
        .send()
        .await
        .map_err(ClaudeError::WebUsageRequest)?;
    if usage_response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(ClaudeError::WebUnauthorized.into());
    }
    let usage_response = usage_response
        .error_for_status()
        .map_err(ClaudeError::WebUsageEndpoint)?;
    let usage_json: ClaudeWebUsageResponse = usage_response
        .json()
        .await
        .map_err(ClaudeError::DecodeWebUsage)?;

    let account = client
        .get(ACCOUNT_ENDPOINT)
        .headers(headers)
        .send()
        .await
        .map_err(ClaudeError::WebAccountRequest)?;
    let account = if account.status().is_success() {
        Some(
            account
                .json::<ClaudeAccountResponse>()
                .await
                .map_err(ClaudeError::DecodeWebAccount)?,
        )
    } else {
        None
    };

    normalize(usage_json, account)
}

fn normalize(
    usage: ClaudeWebUsageResponse,
    account: Option<ClaudeAccountResponse>,
) -> Result<UsageSnapshot> {
    let primary = usage
        .five_hour
        .as_ref()
        .map(|window| normalize_window("5h", window))
        .transpose()?;
    let secondary = usage
        .seven_day
        .as_ref()
        .map(|window| normalize_window("7d", window))
        .transpose()?;
    let tertiary = usage.extra_usage.as_ref().map(|extra| UsageWindow {
        label: "Extra".to_string(),
        used_percent: extra.utilization,
        reset_at: None,
        reset_description: None,
    });
    let provider_cost = usage.extra_usage.map(|extra| ProviderCost {
        used: extra.used_credits / 100.0,
        limit: Some(extra.monthly_limit / 100.0),
        units: "$".to_string(),
    });

    if primary.is_none() && secondary.is_none() && tertiary.is_none() && provider_cost.is_none() {
        return Err(ClaudeError::NoUsageData.into());
    }

    Ok(UsageSnapshot {
        provider: ProviderId::Claude,
        source: "Web Cookie".to_string(),
        updated_at: Utc::now(),
        headline: UsageHeadline::primary_first(
            primary.as_ref(),
            secondary.as_ref(),
            tertiary.as_ref(),
        ),
        primary,
        secondary,
        tertiary,
        provider_cost,
        identity: ProviderIdentity {
            email: account
                .as_ref()
                .and_then(|value| value.email_address.clone()),
            account_id: None,
            plan: account.as_ref().and_then(extract_plan_label),
            display_name: account
                .as_ref()
                .and_then(|value| value.display_name.clone().or(value.full_name.clone())),
        },
    })
}

fn normalize_window(label: &str, window: &ClaudeWebWindow) -> Result<UsageWindow> {
    let reset_at = DateTime::parse_from_rfc3339(&window.resets_at)
        .map_err(|source| ClaudeError::InvalidResetTimestamp {
            value: window.resets_at.clone(),
            source,
        })?
        .with_timezone(&Utc);
    Ok(UsageWindow {
        label: label.to_string(),
        used_percent: window.utilization,
        reset_at: Some(reset_at),
        reset_description: Some(reset_at.to_rfc3339()),
    })
}

fn extract_plan_label(account: &ClaudeAccountResponse) -> Option<String> {
    let organization = account
        .memberships
        .first()
        .map(|membership| &membership.organization)?;
    if organization
        .capabilities
        .iter()
        .any(|capability| capability == "claude_pro")
    {
        Some("pro".to_string())
    } else {
        organization.rate_limit_tier.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_web_fixture_shape() {
        let usage: ClaudeWebUsageResponse =
            serde_json::from_str(include_str!("../../fixtures/claude/web_usage.json")).unwrap();
        let account = Some(
            serde_json::from_str::<ClaudeAccountResponse>(include_str!(
                "../../fixtures/claude/web_account.json"
            ))
            .unwrap(),
        );

        let snapshot = normalize(usage, account).unwrap();
        assert_eq!(snapshot.source, "Web Cookie");
        assert_eq!(snapshot.primary.as_ref().unwrap().used_percent, 91.0);
        assert_eq!(snapshot.secondary.as_ref().unwrap().used_percent, 16.0);
        assert_eq!(snapshot.tertiary.as_ref().unwrap().used_percent, 48.8);
        assert_eq!(snapshot.provider_cost.as_ref().unwrap().used, 2.44);
        assert_eq!(
            snapshot.identity.email.as_deref(),
            Some("topi2236@gmail.com")
        );
        assert_eq!(
            snapshot.identity.display_name.as_deref(),
            Some("Why  do you need my full")
        );
        assert_eq!(snapshot.identity.plan.as_deref(), Some("pro"));
    }

    #[test]
    fn extracts_first_organization_id() {
        let payload: Vec<ClaudeOrganization> =
            serde_json::from_str(include_str!("../../fixtures/claude/web_organizations.json"))
                .unwrap();
        assert_eq!(
            payload
                .first()
                .map(|organization| organization.uuid.as_str()),
            Some("b8b005d7-2019-4a81-a7f1-55a73737e6c3")
        );
    }
}
