// SPDX-License-Identifier: MPL-2.0

use super::{Credits, ENDPOINT, timestamp};
use crate::account_storage::ProviderAccountStorage;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::time::Duration;

const CONSUME_ENDPOINT: &str =
    "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits/consume";

#[derive(Debug, Clone)]
pub(crate) struct ResetOption {
    pub credit_id: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub available_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResetOutcome {
    Reset,
    NothingToReset,
    NoCredit,
    AlreadyRedeemed,
}

fn reset_option(body: &str, now: DateTime<Utc>) -> Option<ResetOption> {
    let response: Credits = serde_json::from_str(body).ok()?;
    let payload = response
        .rate_limit_reset_credits
        .as_deref()
        .unwrap_or(&response);
    let count = payload.available_count?;
    if count == 0 {
        return None;
    }
    let mut credits = payload
        .credits
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter(|credit| credit.status.as_deref() == Some("available"))
        .filter_map(|credit| {
            let expiry = credit.expires_at.as_ref().and_then(timestamp);
            if expiry.is_some_and(|expiry| expiry <= now) {
                None
            } else {
                Some((credit.id.clone().filter(|id| !id.is_empty()), expiry))
            }
        })
        .collect::<Vec<_>>();
    credits.sort_by_key(|(_, expiry)| (expiry.is_none(), *expiry));
    let (credit_id, expires_at) = credits.into_iter().next().unwrap_or((None, None));
    Some(ResetOption {
        credit_id,
        expires_at,
        available_count: count,
    })
}

fn parse_outcome(body: &str) -> Option<ResetOutcome> {
    #[derive(Deserialize)]
    struct Response {
        code: String,
    }
    match serde_json::from_str::<Response>(body).ok()?.code.as_str() {
        "reset" => Some(ResetOutcome::Reset),
        "nothing_to_reset" => Some(ResetOutcome::NothingToReset),
        "no_credit" => Some(ResetOutcome::NoCredit),
        "already_redeemed" => Some(ResetOutcome::AlreadyRedeemed),
        _ => None,
    }
}

async fn request(
    account_id: &str,
    consume: Option<(&str, Option<&str>)>,
) -> Result<String, &'static str> {
    let storage = ProviderAccountStorage::new(crate::config::paths().codex_accounts_dir);
    request_at(&storage, account_id, consume, ENDPOINT, CONSUME_ENDPOINT).await
}

async fn request_at(
    storage: &ProviderAccountStorage,
    account_id: &str,
    consume: Option<(&str, Option<&str>)>,
    details_url: &str,
    consume_url: &str,
) -> Result<String, &'static str> {
    let tokens = storage
        .load_tokens(account_id)
        .map_err(|_| "Codex credentials unavailable")?;
    let metadata = storage
        .load_metadata(account_id)
        .map_err(|_| "Codex account unavailable")?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Unable to create HTTP client")?;
    let mut builder = if consume.is_some() {
        client.post(consume_url)
    } else {
        client.get(details_url)
    }
    .bearer_auth(&tokens.access_token)
    .header(reqwest::header::ACCEPT, "application/json");
    if let Some(provider_account_id) = metadata.provider_account_id.as_deref() {
        builder = builder.header("ChatGPT-Account-Id", provider_account_id);
    }
    if let Some((idempotency_key, credit_id)) = consume {
        let mut payload = serde_json::json!({"redeem_request_id": idempotency_key});
        if let Some(credit_id) = credit_id {
            payload["credit_id"] = serde_json::json!(credit_id);
        }
        builder = builder.json(&payload);
    }
    let response = builder
        .send()
        .await
        .map_err(|_| "Codex reset request failed")?;
    if !response.status().is_success() {
        return Err("Codex reset endpoint rejected the request");
    }
    response
        .text()
        .await
        .map_err(|_| "Unable to read Codex reset response")
}

pub(crate) async fn prepare(account_id: &str) -> Result<ResetOption, &'static str> {
    let body = request(account_id, None).await?;
    reset_option(&body, Utc::now()).ok_or("No usable Codex resets available")
}

pub(crate) async fn consume(
    account_id: &str,
    credit_id: Option<&str>,
    idempotency_key: &str,
) -> Result<ResetOutcome, &'static str> {
    if idempotency_key.is_empty() {
        return Err("Missing reset request identifier");
    }
    let body = request(account_id, Some((idempotency_key, credit_id))).await?;
    parse_outcome(&body).ok_or("Unrecognized Codex reset response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_storage::{NewProviderAccount, ProviderAccountTokens};
    use crate::model::ProviderId;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn selects_earliest_credit_without_using_it_and_parses_outcomes() {
        let response = r#"{"available_count":2,"credits":[{"id":"later","status":"available","expires_at":"2027-09-01T00:00:00Z"},{"id":"earlier","status":"available","expires_at":"2026-10-22T00:00:00Z"}]}"#;
        let selected = reset_option(response, Utc::now()).unwrap();
        assert_eq!(selected.credit_id.as_deref(), Some("earlier"));
        assert_eq!(selected.available_count, 2);
        assert!(reset_option(r#"{"available_count":0,"credits":[]}"#, Utc::now()).is_none());
        assert_eq!(
            parse_outcome(r#"{"code":"reset"}"#),
            Some(ResetOutcome::Reset)
        );
        assert_eq!(
            parse_outcome(r#"{"code":"nothing_to_reset"}"#),
            Some(ResetOutcome::NothingToReset)
        );
        assert_eq!(
            parse_outcome(r#"{"code":"no_credit"}"#),
            Some(ResetOutcome::NoCredit)
        );
        assert_eq!(
            parse_outcome(r#"{"code":"already_redeemed"}"#),
            Some(ResetOutcome::AlreadyRedeemed)
        );
        assert!(parse_outcome(r#"{"code":"unknown"}"#).is_none());
    }

    #[tokio::test]
    async fn explicit_consume_sends_the_selected_credit_and_reusable_request_key() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let stored = storage
            .create_account(NewProviderAccount {
                provider: ProviderId::Codex,
                email: "test@example.invalid".into(),
                provider_account_id: Some("fake-provider-account".into()),
                organization_id: None,
                organization_name: None,
                tokens: ProviderAccountTokens {
                    access_token: "fake-access".into(),
                    refresh_token: "fake-refresh".into(),
                    expires_at: Utc::now() + chrono::Duration::hours(1),
                    scope: Vec::new(),
                    token_id: None,
                },
                snapshot: None,
            })
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let capture = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = vec![0; 4096];
            let read = stream.read(&mut bytes).await.unwrap();
            let request = String::from_utf8_lossy(&bytes[..read]).to_string();
            let reply = r#"{"code":"nothing_to_reset"}"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                        reply.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            request
        });
        let body = request_at(
            &storage,
            &stored.account_ref.account_id,
            Some(("test-request-id", Some("test-credit-id"))),
            &url,
            &url,
        )
        .await
        .unwrap();
        assert_eq!(parse_outcome(&body), Some(ResetOutcome::NothingToReset));
        let request = capture.await.unwrap();
        assert!(request.starts_with("POST / HTTP/1.1"));
        assert!(request.contains("authorization: Bearer fake-access"));
        assert!(request.contains("chatgpt-account-id: fake-provider-account"));
        assert!(request.contains("\"redeem_request_id\":\"test-request-id\""));
        assert!(request.contains("\"credit_id\":\"test-credit-id\""));
    }
}
