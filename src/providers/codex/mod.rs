// SPDX-License-Identifier: MPL-2.0

mod account;
mod login;
mod refresh;

use crate::auth::{CodexAuth, load_codex_auth_from_home, update_codex_auth_tokens};
use crate::error::{CodexError, Result};
use crate::model::{
    ProviderCost, ProviderId, ProviderIdentity, UsageHeadline, UsageSnapshot, UsageWindow,
};
use chrono::{DateTime, Utc};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Deserialize;
use std::path::PathBuf;

pub use account::{apply_login_account, discover_accounts, sync_imported_account};
pub use login::{CodexLoginEvent, CodexLoginState, CodexLoginStatus, prepare};

const ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";

pub async fn fetch(
    client: &reqwest::Client,
    codex_home: PathBuf,
) -> Result<UsageSnapshot, CodexError> {
    let auth = load_codex_auth_from_home(&codex_home)?;
    match fetch_oauth(client, &auth).await {
        Ok(snapshot) => Ok(snapshot),
        Err(error) => {
            if should_refresh_on(&error) {
                let refresh_token = auth
                    .refresh_token
                    .as_deref()
                    .ok_or(CodexError::RefreshUnavailable)?;
                let refreshed = refresh::refresh_access_token(client, refresh_token).await?;
                let path = codex_home.join("auth.json");
                let now_iso = Utc::now().to_rfc3339();
                let refresh_token = refreshed
                    .refresh_token
                    .as_deref()
                    .or(auth.refresh_token.as_deref());
                update_codex_auth_tokens(
                    &path,
                    &refreshed.access_token,
                    refresh_token,
                    Some(&now_iso),
                )?;
                let refreshed_auth = CodexAuth {
                    access_token: refreshed.access_token,
                    account_id: auth.account_id.clone(),
                    refresh_token: refresh_token.map(str::to_string),
                    id_token: auth.id_token.clone(),
                };
                return fetch_oauth(client, &refreshed_auth).await;
            }
            Err(error)
        }
    }
}

async fn fetch_oauth(
    client: &reqwest::Client,
    auth: &CodexAuth,
) -> Result<UsageSnapshot, CodexError> {
    let mut headers = HeaderMap::new();
    let bearer = format!("Bearer {}", auth.access_token);
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&bearer).map_err(CodexError::InvalidBearerHeader)?,
    );
    if let Some(account_id) = &auth.account_id {
        headers.insert(
            "ChatGPT-Account-Id",
            HeaderValue::from_str(account_id).map_err(CodexError::InvalidAccountIdHeader)?,
        );
    }
    let response = client
        .get(ENDPOINT)
        .headers(headers)
        .send()
        .await
        .map_err(CodexError::UsageRequest)?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(CodexError::Unauthorized);
    }
    let status = response.status();
    if !status.is_success() {
        let snippet = response
            .text()
            .await
            .ok()
            .and_then(|body| {
                let trimmed = body.trim();
                (!trimmed.is_empty()).then(|| trimmed.chars().take(512).collect::<String>())
            })
            .map(|body| format!(" (body: {body})"));
        return Err(CodexError::UsageHttp {
            status: status.as_u16(),
            details: snippet.unwrap_or_default(),
        });
    }
    let body = response.text().await.map_err(CodexError::UsageRequest)?;
    let payload: CodexUsageResponse = serde_json::from_str(&body).map_err(|e| {
        tracing::warn!(body = %body.chars().take(512).collect::<String>(), error = %e, "failed to decode codex usage response");
        CodexError::DecodeUsageJson(e)
    })?;
    normalize_oauth(payload)
}

fn should_refresh_on(error: &CodexError) -> bool {
    match error {
        CodexError::Unauthorized => true,
        CodexError::UsageHttp { status, .. } => *status == 401 || *status == 403,
        _ => false,
    }
}

#[derive(Debug, Deserialize)]
struct CodexUsageResponse {
    pub account_id: Option<String>,
    pub email: Option<String>,
    pub plan_type: Option<String>,
    pub rate_limit: Option<CodexRateLimit>,
    pub credits: Option<CodexCredits>,
}

#[derive(Debug, Deserialize)]
struct CodexRateLimit {
    pub primary_window: Option<CodexWindow>,
    pub secondary_window: Option<CodexWindow>,
}

#[derive(Debug, Deserialize)]
struct CodexWindow {
    pub used_percent: f32,
    pub limit_window_seconds: Option<i64>,
    pub reset_at: i64,
}

#[derive(Debug, Deserialize)]
struct CodexCredits {
    pub balance: Option<BalanceValue>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum BalanceValue {
    Str(String),
    Num(f64),
}

impl BalanceValue {
    fn as_f64_str(&self) -> String {
        match self {
            Self::Str(s) => s.clone(),
            Self::Num(n) => n.to_string(),
        }
    }
}

fn normalize_oauth(payload: CodexUsageResponse) -> Result<UsageSnapshot, CodexError> {
    let mut windows = Vec::new();
    if let Some(w) = payload
        .rate_limit
        .as_ref()
        .and_then(|r| r.primary_window.as_ref())
    {
        windows.push(normalize_window(
            "Session",
            w.used_percent,
            w.reset_at,
            w.limit_window_seconds,
        ));
    }
    if let Some(w) = payload
        .rate_limit
        .as_ref()
        .and_then(|r| r.secondary_window.as_ref())
    {
        windows.push(normalize_window(
            "Weekly",
            w.used_percent,
            w.reset_at,
            w.limit_window_seconds,
        ));
    }

    let provider_cost = payload.credits.as_ref().and_then(|c| {
        let raw = c.balance.as_ref()?.as_f64_str();
        match raw.parse::<f64>() {
            Ok(used) => Some(ProviderCost {
                used,
                limit: None,
                units: "credits".to_string(),
            }),
            Err(source) => {
                tracing::warn!(
                    balance = %raw,
                    error = %CodexError::InvalidCreditBalance { balance: raw.clone(), source },
                    "failed to parse codex credit balance"
                );
                None
            }
        }
    });

    if windows.is_empty() && provider_cost.is_none() {
        return Err(CodexError::NoUsageData);
    }

    Ok(UsageSnapshot {
        provider: ProviderId::Codex,
        source: "OAuth".to_string(),
        updated_at: Utc::now(),
        headline: UsageHeadline::first_available(&windows),
        windows,
        provider_cost,
        identity: ProviderIdentity {
            email: payload.email,
            account_id: payload.account_id,
            plan: payload.plan_type,
            display_name: None,
        },
    })
}

fn normalize_window(
    label: &str,
    used_percent: f32,
    reset_at_epoch: i64,
    window_seconds: Option<i64>,
) -> UsageWindow {
    UsageWindow {
        label: label.to_string(),
        used_percent,
        reset_at: DateTime::from_timestamp(reset_at_epoch, 0),
        window_seconds,
        reset_description: None,
    }
}
