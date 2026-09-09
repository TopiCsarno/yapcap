// SPDX-License-Identifier: MPL-2.0

#[allow(dead_code)]
pub mod account;
#[allow(dead_code)]
pub mod login;
#[allow(dead_code)]
pub mod oauth;
pub mod usage;

#[cfg(test)]
mod tests;

#[allow(unused_imports, dead_code)]
pub use login::{
    GrokLoginEvent, GrokLoginState, GrokLoginStatus, GrokLoginSuccess, prepare,
    prepare_host_import, prepare_targeted,
};

use std::path::PathBuf;

use chrono::{Duration, Utc};
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, RETRY_AFTER};

use crate::account_storage::{AccountStorageError, ProviderAccountStorage, ProviderAccountTokens};
use crate::error::GrokError;
use crate::model::UsageSnapshot;

#[allow(dead_code)]
pub const DEFAULT_BILLING_URL: &str = "https://cli-chat-proxy.grok.com/v1/billing?format=credits";
const REFRESH_BEFORE_EXPIRY: Duration = Duration::minutes(5);

#[allow(dead_code)]
pub async fn fetch(
    client: &reqwest::Client,
    account_id: &str,
    account_dir: PathBuf,
) -> Result<UsageSnapshot, GrokError> {
    fetch_at(
        client,
        account_id,
        account_dir,
        DEFAULT_BILLING_URL,
        oauth::TOKEN_URL,
    )
    .await
}

#[allow(dead_code)]
pub async fn fetch_at(
    client: &reqwest::Client,
    account_id: &str,
    account_dir: PathBuf,
    billing_url: &str,
    token_url: &str,
) -> Result<UsageSnapshot, GrokError> {
    let root = account_dir
        .parent()
        .ok_or_else(|| GrokError::AccountStorage("invalid account directory".to_string()))?;
    let storage = ProviderAccountStorage::new(root);
    let metadata = storage
        .load_metadata(account_id)
        .map_err(map_account_storage_error)?;
    let mut tokens = storage
        .load_tokens(account_id)
        .map_err(map_account_storage_error)?;

    if tokens.expires_at <= Utc::now() + REFRESH_BEFORE_EXPIRY {
        refresh_account_tokens(client, &storage, account_id, &mut tokens, token_url).await?;
    }

    let mut response = send_billing_request(client, billing_url, &tokens.access_token).await?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        refresh_account_tokens(client, &storage, account_id, &mut tokens, token_url).await?;
        response = send_billing_request(client, billing_url, &tokens.access_token).await?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(GrokError::Unauthorized);
        }
    }

    let status = response.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let retry_after_secs = parse_retry_after(response.headers());
        return Err(GrokError::RateLimited { retry_after_secs });
    }

    let response = response
        .error_for_status()
        .map_err(|source| GrokError::UsageEndpoint {
            status: status.as_u16(),
            source,
        })?;

    let body = response.text().await.map_err(GrokError::UsageRequest)?;
    let claims = oauth::decode_jwt_claims(&tokens.access_token).or_else(|| {
        tokens
            .token_id
            .as_deref()
            .and_then(oauth::decode_jwt_claims)
    });
    let email = (!metadata.email.trim().is_empty())
        .then_some(metadata.email.as_str())
        .or_else(|| {
            claims
                .as_ref()
                .and_then(|c| c.email.as_deref())
                .filter(|s| !s.trim().is_empty())
        });
    let display_name = claims
        .as_ref()
        .and_then(|c| c.name.as_deref())
        .filter(|s| !s.trim().is_empty());
    let provider_account_id = metadata
        .provider_account_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| claims.as_ref().and_then(|c| c.sub.as_deref()));

    let snapshot = usage::parse_billing_snapshot(&body, email, display_name, provider_account_id)?;
    let _ = storage.save_snapshot(account_id, &snapshot);
    Ok(snapshot)
}

async fn refresh_account_tokens(
    client: &reqwest::Client,
    storage: &ProviderAccountStorage,
    account_id: &str,
    tokens: &mut ProviderAccountTokens,
    token_url: &str,
) -> Result<(), GrokError> {
    if tokens.refresh_token.trim().is_empty() {
        return Err(GrokError::RefreshUnavailable);
    }
    let refreshed = oauth::refresh_token(client, &tokens.refresh_token, token_url).await?;
    tokens.access_token = refreshed.access_token;
    tokens.refresh_token = refreshed.refresh_token;
    tokens.expires_at = refreshed.expires_at;
    if let Some(scope_str) = refreshed.scope {
        tokens.scope = scope_str.split_whitespace().map(String::from).collect();
    }
    if let Some(id_token) = refreshed.id_token {
        tokens.token_id = Some(id_token);
    }
    storage
        .save_tokens(account_id, tokens)
        .map_err(map_account_storage_error)?;
    Ok(())
}

async fn send_billing_request(
    client: &reqwest::Client,
    billing_url: &str,
    access_token: &str,
) -> Result<reqwest::Response, GrokError> {
    let bearer = format!("Bearer {access_token}");
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&bearer).map_err(GrokError::InvalidBearerHeader)?,
    );
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    client
        .get(billing_url)
        .headers(headers)
        .send()
        .await
        .map_err(GrokError::UsageRequest)
}

fn parse_retry_after(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
}

fn map_account_storage_error(error: AccountStorageError) -> GrokError {
    if error.is_missing() {
        GrokError::CredentialsMissing
    } else {
        GrokError::AccountStorage(error.to_string())
    }
}
