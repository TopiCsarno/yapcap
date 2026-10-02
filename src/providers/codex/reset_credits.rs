// SPDX-License-Identifier: MPL-2.0

use crate::account_storage::ProviderAccountStorage;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Duration;

mod redemption;
pub(crate) use redemption::{ResetOption, ResetOutcome, consume, prepare};

const ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits";
const STATE_FILE: &str = "reset-notifications.json";

#[derive(Default, Deserialize)]
struct Credits {
    available_count: Option<usize>,
    credits: Option<Vec<Credit>>,
    rate_limit_reset_credits: Option<Box<Credits>>,
}

#[derive(Deserialize)]
struct Credit {
    id: Option<String>,
    status: Option<String>,
    granted_at: Option<Value>,
    expires_at: Option<Value>,
}

#[derive(Default, Deserialize, Serialize, PartialEq, Eq)]
struct SeenCredits {
    fingerprints: Vec<String>,
    unidentified_count: usize,
    #[serde(default)]
    available_count: usize,
    #[serde(default)]
    earliest_expiry: Option<DateTime<Utc>>,
    #[serde(default)]
    initialized: bool,
}

struct AvailableCredit {
    fingerprint: String,
    expires_at: Option<DateTime<Utc>>,
}

struct Availability {
    credits: Vec<AvailableCredit>,
    count: usize,
}

fn timestamp(value: &Value) -> Option<DateTime<Utc>> {
    match value {
        Value::String(text) => DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|date| date.with_timezone(&Utc)),
        Value::Number(number) => {
            let epoch = number.as_i64()?;
            let seconds = if epoch.unsigned_abs() > 10_000_000_000 {
                epoch / 1000
            } else {
                epoch
            };
            DateTime::from_timestamp(seconds, 0)
        }
        _ => None,
    }
}

fn parse(body: &str, now: DateTime<Utc>) -> Option<Availability> {
    let response: Credits = serde_json::from_str(body).ok()?;
    let payload = response
        .rate_limit_reset_credits
        .as_deref()
        .unwrap_or(&response);
    if payload.available_count.is_none() && payload.credits.is_none() {
        return None;
    }
    let credits = payload
        .credits
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter(|credit| credit.status.as_deref() == Some("available"))
        .filter_map(|credit| {
            let expires_at = credit.expires_at.as_ref().and_then(timestamp);
            if expires_at.is_some_and(|expiry| expiry <= now) {
                return None;
            }
            let issued = credit
                .granted_at
                .as_ref()
                .map_or(String::new(), Value::to_string);
            let expires = credit
                .expires_at
                .as_ref()
                .map_or(String::new(), Value::to_string);
            let id = credit.id.as_deref().unwrap_or("");
            if id.is_empty() && issued.is_empty() && expires.is_empty() {
                return None;
            }
            let identity = format!("{id}:{issued}:{expires}");
            Some(AvailableCredit {
                fingerprint: format!("{:x}", Sha256::digest(identity.as_bytes())),
                expires_at,
            })
        })
        .collect::<Vec<_>>();
    let count = payload.available_count.unwrap_or(credits.len());
    Some(Availability { credits, count })
}

fn new_notification(
    previous: &SeenCredits,
    available: &Availability,
) -> (SeenCredits, usize, Option<DateTime<Utc>>) {
    let mut next = SeenCredits {
        fingerprints: previous.fingerprints.clone(),
        unidentified_count: previous.unidentified_count,
        available_count: available.count,
        earliest_expiry: available
            .credits
            .iter()
            .filter_map(|credit| credit.expires_at)
            .min(),
        initialized: true,
    };
    let mut new_count = 0;
    let mut expiry: Option<DateTime<Utc>> = None;
    for credit in &available.credits {
        if !next.fingerprints.contains(&credit.fingerprint) {
            next.fingerprints.push(credit.fingerprint.clone());
            new_count += 1;
            expiry = match (expiry, credit.expires_at) {
                (Some(first), Some(second)) => Some(first.min(second)),
                (first, second) => first.or(second),
            };
        }
    }
    let unidentified = available.count.saturating_sub(available.credits.len());
    let new_unidentified = unidentified.saturating_sub(previous.unidentified_count);
    next.unidentified_count = unidentified;
    (next, new_count + new_unidentified, expiry)
}

pub(crate) fn latest_availability(account_id: &str) -> Option<(usize, Option<DateTime<Utc>>)> {
    let storage = ProviderAccountStorage::new(crate::config::paths().codex_accounts_dir);
    let state: SeenCredits = storage
        .read_optional_json_file(account_id, STATE_FILE)
        .ok()??;
    state
        .initialized
        .then_some((state.available_count, state.earliest_expiry))
}

pub(super) async fn check_and_notify(
    account_id: &str,
    account_dir: &Path,
) -> Result<(), &'static str> {
    let root = account_dir.parent().ok_or("missing account root")?;
    let storage = ProviderAccountStorage::new(root);
    let tokens = storage
        .load_tokens(account_id)
        .map_err(|_| "no Codex credentials")?;
    let metadata = storage
        .load_metadata(account_id)
        .map_err(|_| "no Codex metadata")?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "unable to create HTTP client")?;
    let mut request = client
        .get(ENDPOINT)
        .bearer_auth(&tokens.access_token)
        .header(reqwest::header::ACCEPT, "application/json");
    if let Some(provider_account_id) = metadata.provider_account_id.as_deref() {
        request = request.header("ChatGPT-Account-Id", provider_account_id);
    }
    let response = request.send().await.map_err(|_| "reset request failed")?;
    if !response.status().is_success() {
        return Err("reset endpoint unavailable");
    }
    let body = response
        .text()
        .await
        .map_err(|_| "unable to read reset response")?;
    let available = parse(&body, Utc::now()).ok_or("invalid reset response")?;
    tracing::info!(
        available_count = available.count,
        "Codex banked resets checked"
    );
    let previous: SeenCredits = storage
        .read_optional_json_file(account_id, STATE_FILE)
        .map_err(|_| "unable to read reset state")?
        .unwrap_or_default();
    let (next, count, expiry) = new_notification(&previous, &available);
    if count == 0 {
        if next != previous {
            storage
                .write_json_file(account_id, STATE_FILE, &next)
                .map_err(|_| "unable to save reset state")?;
        }
        return Ok(());
    }
    let mut message = crate::fl!(
        "codex-reset-alert-body",
        count = count,
        account = metadata.email.as_str()
    );
    if let Some(expiry) = expiry {
        message.push(' ');
        let date = expiry.format("%Y-%m-%d %H:%M UTC").to_string();
        message.push_str(&crate::fl!(
            "codex-reset-alert-expiry",
            date = date.as_str()
        ));
    }
    let status = tokio::process::Command::new("notify-send")
        .args([
            "--app-name=YapCap",
            "--urgency=critical",
            "--expire-time=0",
            &crate::fl!("codex-reset-alert-title"),
            &message,
        ])
        .status()
        .await
        .map_err(|_| "desktop notifications unavailable")?;
    if !status.success() {
        return Err("desktop notification failed");
    }
    storage
        .write_json_file(account_id, STATE_FILE, &next)
        .map_err(|_| "unable to save reset state")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifies_new_available_reset_only_once_per_credit() {
        let now = Utc::now();
        let body = r#"{"available_count":1,"credits":[{"id":"secret-a","status":"available","granted_at":"2026-09-01T00:00:00Z","expires_at":"2027-09-01T00:00:00Z"},{"id":"secret-b","status":"redeemed"}]}"#;
        let availability = parse(body, now).unwrap();
        let (seen, count, expiry) = new_notification(&SeenCredits::default(), &availability);
        assert_eq!(count, 1);
        assert!(expiry.is_some());
        assert!(!serde_json::to_string(&seen).unwrap().contains("secret-a"));
        assert_eq!(new_notification(&seen, &availability).1, 0);
    }

    #[test]
    fn distinguishes_new_credit_when_total_count_is_unchanged() {
        let now = Utc::now();
        let first = parse(r#"{"credits":[{"id":"first","status":"available"}]}"#, now).unwrap();
        let (seen, _, _) = new_notification(&SeenCredits::default(), &first);
        let second = parse(r#"{"credits":[{"id":"second","status":"available"}]}"#, now).unwrap();
        assert_eq!(new_notification(&seen, &second).1, 1);
    }

    #[test]
    fn rejects_invalid_responses_and_expired_credits() {
        assert!(parse("{}", Utc::now()).is_none());
        let expired = parse(r#"{"available_count":0,"credits":[{"id":"old","status":"available","expires_at":"2020-01-01T00:00:00Z"}]}"#, Utc::now()).unwrap();
        assert_eq!(new_notification(&SeenCredits::default(), &expired).1, 0);
    }

    #[test]
    fn count_only_response_rearms_after_balance_returns_to_zero() {
        let now = Utc::now();
        let one = parse(r#"{"available_count":1}"#, now).unwrap();
        let none = parse(r#"{"available_count":0}"#, now).unwrap();
        let (seen, count, _) = new_notification(&SeenCredits::default(), &one);
        assert_eq!(count, 1);
        let (empty, count, _) = new_notification(&seen, &none);
        assert_eq!(count, 0);
        assert_eq!(empty.unidentified_count, 0);
        assert_eq!(new_notification(&empty, &one).1, 1);
    }

    #[test]
    fn nested_response_and_repeated_credit_are_not_reported_twice() {
        let body = r#"{"rate_limit_reset_credits":{"available_count":1,"credits":[{"id":"same","status":"available"},{"id":"same","status":"available"}]}}"#;
        let available = parse(body, Utc::now()).unwrap();
        let (seen, count, _) = new_notification(&SeenCredits::default(), &available);
        assert_eq!(count, 1);
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        storage
            .write_json_file("codex-test", STATE_FILE, &seen)
            .unwrap();
        let restored: SeenCredits = storage.read_json_file("codex-test", STATE_FILE).unwrap();
        assert_eq!(new_notification(&restored, &available).1, 0);
    }

    #[test]
    fn available_count_and_expiry_are_kept_for_the_popup() {
        let now = Utc::now();
        let available = parse(r#"{"credits":[{"id":"first","status":"available","expires_at":"2027-09-01T00:00:00Z"}]}"#, now).unwrap();
        let (first, _, _) = new_notification(&SeenCredits::default(), &available);
        assert_eq!(first.available_count, 1);
        assert!(first.earliest_expiry.is_some());
        let empty = parse(r#"{"available_count":0,"credits":[]}"#, now).unwrap();
        let (cleared, count, _) = new_notification(&first, &empty);
        assert_eq!(count, 0);
        assert_eq!(cleared.available_count, 0);
        assert_eq!(cleared.earliest_expiry, None);
        assert!(cleared.initialized);
    }
}
