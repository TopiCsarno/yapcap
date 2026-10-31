// SPDX-License-Identifier: MPL-2.0

pub mod account;
mod browser;
mod callback;
mod error;
pub mod login;
mod usage;

pub use error::CommandCodeError;

use crate::config::ManagedCommandCodeAccountConfig;
use crate::model::{ProviderIdentity, UsageSnapshot};
use reqwest::{Client, StatusCode};
use serde_json::Value;
use std::time::Duration;

const API_BASE: &str = "https://api.commandcode.ai";

pub async fn fetch(
    _client: &Client,
    account: &ManagedCommandCodeAccountConfig,
) -> Result<UsageSnapshot, CommandCodeError> {
    let key = account::load_key(&account.id).map_err(|_| CommandCodeError::Storage)?;
    fetch_key(&key).await
}

pub(crate) async fn fetch_key(key: &str) -> Result<UsageSnapshot, CommandCodeError> {
    let client = crate::runtime::http_client_without_redirects();
    tokio::time::timeout(Duration::from_secs(30), fetch_at(&client, key, API_BASE))
        .await
        .map_err(|_| CommandCodeError::Timeout)?
}

async fn fetch_at(
    client: &Client,
    key: &str,
    base: &str,
) -> Result<UsageSnapshot, CommandCodeError> {
    let whoami = request(client, base, "/alpha/whoami", key, None).await?;
    let identity = identity(&whoami).ok_or(CommandCodeError::InvalidResponse)?;
    let org_id = whoami
        .get("org")
        .and_then(|org| org.get("id"))
        .and_then(Value::as_str);
    let credits = request(client, base, "/alpha/billing/credits", key, org_id).await?;
    let subscription =
        match request(client, base, "/alpha/billing/subscriptions", key, org_id).await {
            Ok(value) => Some(value),
            Err(error @ (CommandCodeError::LoginRequired | CommandCodeError::AccessDenied)) => {
                return Err(error);
            }
            Err(_) => None,
        };
    usage::parse(
        &credits,
        subscription.as_ref(),
        identity,
        chrono::Utc::now(),
    )
}

async fn request(
    client: &Client,
    base: &str,
    endpoint: &str,
    key: &str,
    org_id: Option<&str>,
) -> Result<Value, CommandCodeError> {
    let mut request = client
        .get(format!("{base}{endpoint}"))
        .header(reqwest::header::ACCEPT, "application/json")
        .bearer_auth(key);
    if let Some(org_id) = org_id {
        request = request.query(&[("orgId", org_id)]);
    }
    let response = request.send().await.map_err(CommandCodeError::Request)?;
    match response.status() {
        StatusCode::UNAUTHORIZED => return Err(CommandCodeError::LoginRequired),
        StatusCode::FORBIDDEN => return Err(CommandCodeError::AccessDenied),
        StatusCode::TOO_MANY_REQUESTS => {
            return Err(CommandCodeError::RateLimited {
                retry_after_secs: response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse().ok()),
            });
        }
        status if !status.is_success() => {
            return Err(CommandCodeError::Http {
                status: status.as_u16(),
            });
        }
        _ => {}
    }
    response
        .json()
        .await
        .map_err(|_| CommandCodeError::InvalidResponse)
}

fn identity(value: &Value) -> Option<ProviderIdentity> {
    let user = value.get("user")?;
    let account_id = user.get("id")?.as_str()?.to_string();
    if account_id.is_empty() {
        return None;
    }
    Some(ProviderIdentity {
        account_id: Some(account_id),
        email: user
            .get("email")
            .and_then(Value::as_str)
            .map(str::to_string),
        display_name: value
            .get("org")
            .and_then(|org| org.get("login"))
            .or_else(|| user.get("userName"))
            .or_else(|| user.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string),
        plan: None,
    })
}

#[cfg(test)]
mod tests;
