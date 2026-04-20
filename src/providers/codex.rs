// SPDX-License-Identifier: MPL-2.0

use crate::auth::load_codex_auth;
use crate::error::{CodexError, Result};
use crate::model::{
    ProviderCost, ProviderId, ProviderIdentity, UsageHeadline, UsageSnapshot, UsageWindow,
};
use chrono::{DateTime, Utc};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Deserialize;
use std::cmp::Reverse;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";
const RPC_TIMEOUT: Duration = Duration::from_secs(8);

pub async fn fetch(client: &reqwest::Client) -> Result<UsageSnapshot, CodexError> {
    match fetch_oauth(client).await {
        Ok(snapshot) => Ok(snapshot),
        Err(error) => {
            tracing::warn!(error = %error, "codex OAuth failed; trying CLI RPC");
            let binary = find_codex_binary().ok_or(CodexError::CliUnavailable)?;
            fetch_rpc(&binary).await
        }
    }
}

async fn fetch_oauth(client: &reqwest::Client) -> Result<UsageSnapshot, CodexError> {
    let auth = load_codex_auth()?;
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
    let response = response
        .error_for_status()
        .map_err(CodexError::UsageEndpoint)?;
    let payload: CodexUsageResponse = response.json().await.map_err(CodexError::DecodeUsage)?;
    normalize_oauth(payload)
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
    pub used_percent: f64,
    pub reset_at: i64,
}

#[derive(Debug, Deserialize)]
struct CodexCredits {
    pub balance: String,
}

fn normalize_oauth(payload: CodexUsageResponse) -> Result<UsageSnapshot, CodexError> {
    let primary = payload
        .rate_limit
        .as_ref()
        .and_then(|r| r.primary_window.as_ref())
        .map(|w| normalize_window("Session", w.used_percent, w.reset_at));
    let secondary = payload
        .rate_limit
        .as_ref()
        .and_then(|r| r.secondary_window.as_ref())
        .map(|w| normalize_window("Weekly", w.used_percent, w.reset_at));

    let provider_cost = payload.credits.as_ref().and_then(|c| {
        match c.balance.parse::<f64>() {
            Ok(used) => Some(ProviderCost {
                used,
                limit: None,
                units: "credits".to_string(),
            }),
            Err(source) => {
                tracing::warn!(
                    balance = %c.balance,
                    error = %CodexError::InvalidCreditBalance { balance: c.balance.clone(), source },
                    "failed to parse codex credit balance"
                );
                None
            }
        }
    });

    if primary.is_none() && secondary.is_none() && provider_cost.is_none() {
        return Err(CodexError::NoUsageData);
    }

    Ok(UsageSnapshot {
        provider: ProviderId::Codex,
        source: "OAuth".to_string(),
        updated_at: Utc::now(),
        headline: UsageHeadline::primary_first(primary.as_ref(), secondary.as_ref(), None),
        primary,
        secondary,
        tertiary: None,
        provider_cost,
        identity: ProviderIdentity {
            email: payload.email,
            account_id: payload.account_id,
            plan: payload.plan_type,
            display_name: None,
        },
    })
}

async fn fetch_rpc(binary: &Path) -> Result<UsageSnapshot, CodexError> {
    let binary = binary.to_path_buf();
    tokio::task::spawn_blocking(move || fetch_rpc_blocking(&binary))
        .await
        .map_err(|_| CodexError::CliParse)?
}

fn fetch_rpc_blocking(binary: &Path) -> Result<UsageSnapshot, CodexError> {
    let mut child = Command::new(binary)
        .args(["-s", "read-only", "-a", "untrusted", "app-server"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                CodexError::CliUnavailable
            } else {
                CodexError::CliCommand(e)
            }
        })?;

    let mut stdin = child.stdin.take().ok_or(CodexError::CliParse)?;
    let stdout = child.stdout.take().ok_or(CodexError::CliParse)?;

    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(std::result::Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    let init_req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "initialize",
        "params": {"clientInfo": {"name": "yapcap", "version": env!("CARGO_PKG_VERSION")}},
        "id": 1
    });
    writeln!(stdin, "{init_req}").map_err(CodexError::CliIo)?;
    stdin.flush().map_err(CodexError::CliIo)?;

    let deadline = Instant::now() + RPC_TIMEOUT;
    let mut init_ok = false;
    let mut rate_limits_val: Option<serde_json::Value> = None;

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CodexError::CliTimeout { timeout: RPC_TIMEOUT });
        }
        match rx.recv_timeout(remaining) {
            Ok(line) => {
                let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };
                match val.get("id").and_then(|v| v.as_u64()) {
                    Some(1) => {
                        if val.get("error").is_some() {
                            let _ = child.kill();
                            let _ = child.wait();
                            return Err(CodexError::RpcProtocol);
                        }
                        init_ok = true;
                        let rl_req = serde_json::json!({
                            "jsonrpc": "2.0",
                            "method": "account/rateLimits/read",
                            "params": {},
                            "id": 2
                        });
                        writeln!(stdin, "{rl_req}").map_err(CodexError::CliIo)?;
                        stdin.flush().map_err(CodexError::CliIo)?;
                    }
                    Some(2) => {
                        rate_limits_val = Some(val);
                        break;
                    }
                    _ => {}
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CodexError::CliTimeout { timeout: RPC_TIMEOUT });
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let _ = child.kill();
    let _ = child.wait();

    if !init_ok {
        return Err(CodexError::RpcProtocol);
    }
    let response = rate_limits_val.ok_or(CodexError::CliParse)?;
    normalize_rpc(&response)
}

fn normalize_rpc(response: &serde_json::Value) -> Result<UsageSnapshot, CodexError> {
    let limits = response
        .get("result")
        .and_then(|r| r.get("rateLimits"))
        .ok_or(CodexError::CliParse)?;

    let primary = limits.get("primary").and_then(|w| rpc_window(w, "Session"));
    let secondary = limits
        .get("secondary")
        .and_then(|w| rpc_window(w, "Weekly"));
    let credits = limits
        .get("credits")
        .and_then(|c| c.get("balance"))
        .and_then(|b| b.as_str())
        .and_then(|s| s.parse::<f64>().ok());
    let plan = limits
        .get("planType")
        .and_then(|v| v.as_str())
        .map(String::from);

    if primary.is_none() && secondary.is_none() && credits.is_none() {
        return Err(CodexError::CliParse);
    }

    Ok(UsageSnapshot {
        provider: ProviderId::Codex,
        source: "RPC".to_string(),
        updated_at: Utc::now(),
        headline: UsageHeadline::primary_first(primary.as_ref(), secondary.as_ref(), None),
        primary,
        secondary,
        tertiary: None,
        provider_cost: credits.map(|used| ProviderCost {
            used,
            limit: None,
            units: "credits".to_string(),
        }),
        identity: ProviderIdentity {
            plan,
            ..ProviderIdentity::default()
        },
    })
}

fn rpc_window(val: &serde_json::Value, label: &str) -> Option<UsageWindow> {
    let used_percent = val.get("usedPercent")?.as_f64()?;
    let resets_at = val.get("resetsAt")?.as_i64()?;
    Some(normalize_window(label, used_percent, resets_at))
}

fn find_codex_binary() -> Option<PathBuf> {
    if Command::new("codex")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return Some(PathBuf::from("codex"));
    }

    if let Ok(out) = Command::new("bash")
        .args(["-lc", "which codex 2>/dev/null"])
        .output()
    {
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !path.is_empty() && PathBuf::from(&path).exists() {
            return Some(PathBuf::from(path));
        }
    }

    let home = dirs::home_dir()?;

    for candidate in [
        home.join(".volta/bin/codex"),
        home.join(".local/share/fnm/current/bin/codex"),
        home.join(".npm-global/bin/codex"),
    ] {
        if candidate.exists() {
            return Some(candidate);
        }
    }

    let nvm_dir = std::env::var_os("NVM_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".nvm"));
    let nvm_versions = nvm_dir.join("versions/node");
    if nvm_versions.exists() {
        let mut entries: Vec<_> = std::fs::read_dir(&nvm_versions)
            .into_iter()
            .flatten()
            .flatten()
            .collect();
        entries.sort_by_key(|e| Reverse(e.file_name()));
        for entry in entries {
            let candidate = entry.path().join("bin/codex");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

fn normalize_window(label: &str, used_percent: f64, reset_at_epoch: i64) -> UsageWindow {
    let reset_at = DateTime::from_timestamp(reset_at_epoch, 0);
    UsageWindow {
        label: label.to_string(),
        used_percent,
        reset_at,
        reset_description: reset_at.map(|t| t.to_rfc3339()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_normalizes_fixture() {
        let payload: CodexUsageResponse =
            serde_json::from_str(include_str!("../../fixtures/codex/usage_oauth.json")).unwrap();
        let snapshot = normalize_oauth(payload).unwrap();
        assert_eq!(snapshot.provider, ProviderId::Codex);
        assert_eq!(snapshot.source, "OAuth");
        assert_eq!(snapshot.primary.as_ref().unwrap().used_percent, 3.0);
        assert_eq!(snapshot.secondary.as_ref().unwrap().used_percent, 24.0);
        assert_eq!(snapshot.identity.plan.as_deref(), Some("plus"));
    }

    #[test]
    fn oauth_keeps_credits_without_rate_windows() {
        let payload = CodexUsageResponse {
            account_id: Some("acct_123".to_string()),
            email: Some("user@example.com".to_string()),
            plan_type: Some("plus".to_string()),
            rate_limit: None,
            credits: Some(CodexCredits {
                balance: "12.5".to_string(),
            }),
        };
        let snapshot = normalize_oauth(payload).unwrap();
        assert!(snapshot.primary.is_none());
        assert!(snapshot.secondary.is_none());
        assert_eq!(snapshot.provider_cost.as_ref().unwrap().used, 12.5);
        assert_eq!(snapshot.identity.account_id.as_deref(), Some("acct_123"));
    }

    #[test]
    fn rpc_normalizes_fixture() {
        let response: serde_json::Value =
            serde_json::from_str(include_str!("../../fixtures/codex/usage_rpc.json")).unwrap();
        let snapshot = normalize_rpc(&response).unwrap();
        assert_eq!(snapshot.provider, ProviderId::Codex);
        assert_eq!(snapshot.source, "RPC");
        assert_eq!(snapshot.primary.as_ref().unwrap().used_percent, 3.0);
        assert_eq!(snapshot.secondary.as_ref().unwrap().used_percent, 24.0);
        assert_eq!(snapshot.identity.plan.as_deref(), Some("plus"));
        assert_eq!(snapshot.provider_cost.as_ref().unwrap().used, 0.0);
    }

    #[test]
    fn rpc_same_windows_as_oauth() {
        let oauth_payload: CodexUsageResponse =
            serde_json::from_str(include_str!("../../fixtures/codex/usage_oauth.json")).unwrap();
        let rpc_response: serde_json::Value =
            serde_json::from_str(include_str!("../../fixtures/codex/usage_rpc.json")).unwrap();
        let oauth = normalize_oauth(oauth_payload).unwrap();
        let rpc = normalize_rpc(&rpc_response).unwrap();
        assert_eq!(
            oauth.primary.as_ref().unwrap().used_percent,
            rpc.primary.as_ref().unwrap().used_percent
        );
        assert_eq!(
            oauth.secondary.as_ref().unwrap().used_percent,
            rpc.secondary.as_ref().unwrap().used_percent
        );
        assert_eq!(oauth.identity.plan, rpc.identity.plan);
    }

    #[test]
    fn rpc_missing_rate_limits_errors() {
        let val = serde_json::json!({"id": 2, "result": {}});
        assert!(normalize_rpc(&val).is_err());
    }
}
