// SPDX-License-Identifier: MPL-2.0

use crate::account_storage::{NewProviderAccount, ProviderAccountTokens};
use crate::error::ClaudeError;
use crate::model::ProviderId;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::json;
use thiserror::Error;

pub(super) const TOKEN_ENDPOINT: &str = "https://console.anthropic.com/v1/oauth/token";
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClaudeTokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
    pub scope: Vec<String>,
    pub token_id: Option<String>,
    pub account_id: Option<String>,
    pub email: Option<String>,
    pub organization_id: Option<String>,
    pub organization_name: Option<String>,
}

impl ClaudeTokenResponse {
    pub(crate) fn into_new_account(self) -> Result<NewProviderAccount, ClaudeOAuthTokenError> {
        let email = self
            .email
            .as_deref()
            .map(str::trim)
            .filter(|email| !email.is_empty())
            .ok_or(ClaudeOAuthTokenError::MissingAccountEmail)?
            .to_ascii_lowercase();
        let provider_account_id = self
            .account_id
            .filter(|account_id| !account_id.trim().is_empty());

        Ok(NewProviderAccount {
            provider: ProviderId::Claude,
            email,
            provider_account_id,
            organization_id: self.organization_id,
            organization_name: self.organization_name,
            tokens: ProviderAccountTokens {
                access_token: self.access_token,
                refresh_token: self.refresh_token,
                expires_at: self.expires_at,
                scope: self.scope,
                token_id: self.token_id,
            },
            snapshot: None,
        })
    }
}

pub(crate) fn parse_token_response(
    raw: &str,
    now: DateTime<Utc>,
) -> Result<ClaudeTokenResponse, ClaudeOAuthTokenError> {
    RawClaudeTokenResponse::parse(raw, now)
}

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum ClaudeOAuthTokenError {
    #[error("failed to decode Claude OAuth token response")]
    Decode,
    #[error("Claude OAuth token response missing {0}")]
    MissingField(&'static str),
    #[error("Claude OAuth token response has invalid expires_in")]
    InvalidExpiresIn,
    #[error("Claude OAuth token response missing account email")]
    MissingAccountEmail,
}

#[derive(Debug, Deserialize)]
struct RawClaudeTokenResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    token_uuid: Option<String>,
    #[serde(default)]
    account: Option<RawClaudeAccount>,
    #[serde(default)]
    organization: Option<RawClaudeOrganization>,
}

#[derive(Debug, Deserialize)]
struct RawClaudeAccount {
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    email_address: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawClaudeOrganization {
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

impl RawClaudeTokenResponse {
    fn parse(raw: &str, now: DateTime<Utc>) -> Result<ClaudeTokenResponse, ClaudeOAuthTokenError> {
        let parsed: Self = serde_json::from_str(raw).map_err(|_| ClaudeOAuthTokenError::Decode)?;
        let expires_in = parsed
            .expires_in
            .ok_or(ClaudeOAuthTokenError::MissingField("expires_in"))?;
        if expires_in <= 0 {
            return Err(ClaudeOAuthTokenError::InvalidExpiresIn);
        }

        Ok(ClaudeTokenResponse {
            access_token: required_string(parsed.access_token, "access_token")?,
            refresh_token: required_string(parsed.refresh_token, "refresh_token")?,
            expires_at: now + Duration::seconds(expires_in),
            scope: normalize_scope(&required_string(parsed.scope, "scope")?),
            token_id: non_empty(parsed.token_uuid),
            account_id: parsed
                .account
                .as_ref()
                .and_then(|account| non_empty(account.uuid.clone())),
            email: parsed
                .account
                .as_ref()
                .and_then(|account| non_empty(account.email_address.clone())),
            organization_id: parsed
                .organization
                .as_ref()
                .and_then(|organization| non_empty(organization.uuid.clone())),
            organization_name: parsed
                .organization
                .as_ref()
                .and_then(|organization| non_empty(organization.name.clone())),
        })
    }
}

fn required_string(
    value: Option<String>,
    field: &'static str,
) -> Result<String, ClaudeOAuthTokenError> {
    non_empty(value).ok_or(ClaudeOAuthTokenError::MissingField(field))
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) async fn refresh_access_token_at(
    client: &reqwest::Client,
    endpoint: &str,
    refresh_token: &str,
    now: DateTime<Utc>,
) -> Result<ClaudeTokenResponse, ClaudeError> {
    let payload = json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "client_id": CLIENT_ID,
    });
    let response = client
        .post(endpoint)
        .header("User-Agent", "claude-code/2.0.32")
        .json(&payload)
        .send()
        .await
        .map_err(ClaudeError::TokenRefreshRequest)?;
    let status = response.status();
    if !status.is_success() {
        return Err(ClaudeError::TokenRefreshHttp {
            status: status.as_u16(),
        });
    }
    let body = response
        .text()
        .await
        .map_err(ClaudeError::TokenRefreshDecode)?;
    parse_token_response(&body, now).map_err(|e| ClaudeError::TokenRefreshParse(e.to_string()))
}

fn normalize_scope(scope: &str) -> Vec<String> {
    let mut scopes = scope
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    scopes.sort();
    scopes.dedup();
    scopes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ProviderId;
    use chrono::{TimeZone, Utc};

    fn now() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 4, 30, 10, 0, 0).unwrap()
    }

    #[test]
    fn parses_oauth_token_probe_fixture() {
        let envelope: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/claude/oauth_token_response.json"
        ))
        .unwrap();
        let body = envelope["body_json"].clone();
        let raw = serde_json::to_string(&body).unwrap();
        let n = now();
        let parsed = parse_token_response(&raw, n).unwrap();

        assert_eq!(
            parsed.access_token,
            "sk-ant-oat01-redacted-access-token-for-fixtures"
        );
        assert_eq!(
            parsed.refresh_token,
            "sk-ant-ort01-redacted-refresh-token-for-fixtures"
        );
        assert_eq!(parsed.expires_at, n + chrono::Duration::seconds(28800));
        assert_eq!(
            parsed.token_id.as_deref(),
            Some("33333333-3333-3333-3333-333333333333")
        );
        assert_eq!(
            parsed.account_id.as_deref(),
            Some("22222222-2222-2222-2222-222222222222")
        );
        assert_eq!(parsed.email.as_deref(), Some("user@example.com"));
        assert_eq!(
            parsed.organization_id.as_deref(),
            Some("11111111-1111-1111-1111-111111111111")
        );
        assert_eq!(
            parsed.organization_name.as_deref(),
            Some("Example Organization")
        );
        assert_eq!(parsed.scope, ["user:profile"]);
    }

    #[test]
    fn missing_required_token_field_fails_parsing() {
        let error = parse_token_response(
            r#"{
                "token_type": "Bearer",
                "access_token": "access",
                "expires_in": 28800,
                "scope": "user:profile",
                "account": {"uuid": "account-id", "email_address": "user@example.com"}
            }"#,
            now(),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            ClaudeOAuthTokenError::MissingField("refresh_token")
        ));
    }

    #[test]
    fn missing_email_fails_account_creation_validation() {
        let parsed = parse_token_response(
            r#"{
                "token_type": "Bearer",
                "access_token": "access",
                "refresh_token": "refresh",
                "expires_in": 28800,
                "scope": "user:profile",
                "token_uuid": "token-id",
                "account": {"uuid": "account-id"},
                "organization": {"uuid": "org-id", "name": "Org"}
            }"#,
            now(),
        )
        .unwrap();

        let error = parsed.into_new_account().unwrap_err();

        assert!(matches!(error, ClaudeOAuthTokenError::MissingAccountEmail));
    }

    #[test]
    fn normalizes_scope_string_for_account_tokens() {
        let parsed = parse_token_response(
            r#"{
                "token_type": "Bearer",
                "access_token": "access",
                "refresh_token": "refresh",
                "expires_in": 60,
                "scope": " user:profile  user:profile user:sessions:claude_code ",
                "token_uuid": "token-id",
                "account": {"uuid": "account-id", "email_address": "user@example.com"}
            }"#,
            now(),
        )
        .unwrap();

        let account = parsed.into_new_account().unwrap();

        assert_eq!(account.provider, ProviderId::Claude);
        assert_eq!(account.email, "user@example.com");
        assert_eq!(account.provider_account_id.as_deref(), Some("account-id"));
        assert_eq!(
            account.tokens.scope,
            ["user:profile", "user:sessions:claude_code"]
        );
    }
}
