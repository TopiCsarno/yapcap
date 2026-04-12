use crate::auth::load_codex_auth;
use crate::error::{CodexError, Result};
use crate::model::{
    ProviderCost, ProviderId, ProviderIdentity, UsageHeadline, UsageSnapshot, UsageWindow,
};
use chrono::{DateTime, Utc};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Deserialize;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use tracing::warn;

const ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";
const RPC_TIMEOUT: Duration = Duration::from_secs(8);
const PTY_STARTUP: Duration = Duration::from_millis(2000);
const PTY_TIMEOUT: Duration = Duration::from_secs(15);
const PTY_STATUS_SETTLE: Duration = Duration::from_millis(2500);
const FORCE_SOURCE_ENV: &str = "YAPCAP_CODEX_FORCE_SOURCE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexSource {
    Oauth,
    Rpc,
    Pty,
}

// === Public entry point — OAuth → CLI-RPC → CLI-PTY ===

pub async fn fetch(client: &reqwest::Client) -> Result<UsageSnapshot> {
    if let Some(source) = forced_source() {
        warn!(source = ?source, "forcing codex source via env var");
        return fetch_forced_source(client, source).await;
    }

    match fetch_oauth(client).await {
        Ok(snapshot) => Ok(snapshot),
        Err(error) => {
            warn!(error = %error, "codex OAuth failed; trying CLI");
            fetch_cli_fallback().await
        }
    }
}

async fn fetch_forced_source(
    client: &reqwest::Client,
    source: CodexSource,
) -> Result<UsageSnapshot> {
    match source {
        CodexSource::Oauth => fetch_oauth(client).await,
        CodexSource::Rpc => {
            let binary = find_codex_binary().ok_or(CodexError::CliUnavailable)?;
            fetch_rpc(&binary).await
        }
        CodexSource::Pty => {
            let binary = find_codex_binary().ok_or(CodexError::CliUnavailable)?;
            fetch_pty(&binary).await
        }
    }
}

async fn fetch_cli_fallback() -> Result<UsageSnapshot> {
    let binary = find_codex_binary().ok_or(CodexError::CliUnavailable)?;

    match fetch_rpc(&binary).await {
        Ok(snapshot) => Ok(snapshot),
        Err(error) => {
            warn!(error = %error, "codex RPC failed; trying PTY");
            fetch_pty(&binary).await
        }
    }
}

// === OAuth ===

async fn fetch_oauth(client: &reqwest::Client) -> Result<UsageSnapshot> {
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
        return Err(CodexError::Unauthorized.into());
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

fn normalize_oauth(payload: CodexUsageResponse) -> Result<UsageSnapshot> {
    let primary = payload
        .rate_limit
        .as_ref()
        .and_then(|r| r.primary_window.as_ref())
        .map(|w| normalize_window("5h", w.used_percent, w.reset_at));
    let secondary = payload
        .rate_limit
        .as_ref()
        .and_then(|r| r.secondary_window.as_ref())
        .map(|w| normalize_window("7d", w.used_percent, w.reset_at));

    let provider_cost = payload.credits.as_ref().and_then(|c| {
        match c.balance.parse::<f64>() {
            Ok(used) => Some(ProviderCost {
                used,
                limit: None,
                units: "credits".to_string(),
            }),
            Err(source) => {
                warn!(
                    balance = %c.balance,
                    error = %CodexError::InvalidCreditBalance { balance: c.balance.clone(), source },
                    "failed to parse codex credit balance"
                );
                None
            }
        }
    });

    if primary.is_none() && secondary.is_none() && provider_cost.is_none() {
        return Err(CodexError::NoUsageData.into());
    }

    Ok(UsageSnapshot {
        provider: ProviderId::Codex,
        source: "OAuth".to_string(),
        updated_at: Utc::now(),
        headline: if secondary.is_some() {
            UsageHeadline::Secondary
        } else {
            UsageHeadline::Primary
        },
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

// === CLI-RPC ===

async fn fetch_rpc(binary: &Path) -> Result<UsageSnapshot> {
    let binary = binary.to_path_buf();
    tokio::task::spawn_blocking(move || fetch_rpc_blocking(&binary))
        .await
        .map_err(|_| CodexError::CliParse)?
}

fn fetch_rpc_blocking(binary: &Path) -> Result<UsageSnapshot> {
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

    // Reader thread sends lines over a channel so we can apply a real timeout.
    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines().flatten() {
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
            return Err(CodexError::CliTimeout {
                timeout: RPC_TIMEOUT,
            }
            .into());
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
                            return Err(CodexError::RpcProtocol.into());
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
                return Err(CodexError::CliTimeout {
                    timeout: RPC_TIMEOUT,
                }
                .into());
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let _ = child.kill();
    let _ = child.wait();

    if !init_ok {
        return Err(CodexError::RpcProtocol.into());
    }
    let response = rate_limits_val.ok_or(CodexError::CliParse)?;
    normalize_rpc(&response)
}

fn normalize_rpc(response: &serde_json::Value) -> Result<UsageSnapshot> {
    let limits = response
        .get("result")
        .and_then(|r| r.get("rateLimits"))
        .ok_or(CodexError::CliParse)?;

    let primary = limits.get("primary").and_then(|w| rpc_window(w, "5h"));
    let secondary = limits.get("secondary").and_then(|w| rpc_window(w, "7d"));
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
        return Err(CodexError::CliParse.into());
    }

    Ok(UsageSnapshot {
        provider: ProviderId::Codex,
        source: "RPC".to_string(),
        updated_at: Utc::now(),
        headline: if secondary.is_some() {
            UsageHeadline::Secondary
        } else {
            UsageHeadline::Primary
        },
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

// === CLI-PTY ===

async fn fetch_pty(binary: &Path) -> Result<UsageSnapshot> {
    let binary = binary.to_path_buf();
    tokio::task::spawn_blocking(move || fetch_pty_blocking(&binary))
        .await
        .map_err(|_| CodexError::CliParse)?
}

fn fetch_pty_blocking(binary: &Path) -> Result<UsageSnapshot> {
    let transcript = run_pty_command(binary)?;
    parse_pty_snapshot(&transcript)
}

fn run_pty_command(binary: &Path) -> Result<String> {
    // COLUMNS/LINES avoid a wrapping-calc crash in Codex's TUI on narrow terminals.
    let cmd = format!(
        "COLUMNS=200 LINES=60 {} -s read-only -a untrusted",
        binary.display()
    );
    let mut child = Command::new("script")
        .args(["-qefc", &cmd, "/dev/null"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(CodexError::CliCommand)?;

    let mut stdin = child.stdin.take().ok_or(CodexError::CliParse)?;
    thread::sleep(PTY_STARTUP);
    stdin.write_all(b"/status\n").map_err(CodexError::CliIo)?;
    stdin.flush().map_err(CodexError::CliIo)?;

    // `/status` updates the interactive footer but does not terminate the Codex TUI.
    // Treat the PTY path as a bounded probe: allow the footer to render, then end the
    // session ourselves and parse the captured transcript.
    let deadline = Instant::now() + PTY_TIMEOUT;
    thread::sleep(PTY_STATUS_SETTLE.min(deadline.saturating_duration_since(Instant::now())));
    drop(stdin);

    if child.try_wait().map_err(CodexError::CliCommand)?.is_none() {
        let _ = child.kill();
    }

    let output = child.wait_with_output().map_err(CodexError::CliCommand)?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) fn parse_pty_snapshot(transcript: &str) -> Result<UsageSnapshot> {
    let clean = strip_ansi(transcript);

    let primary = legacy_percent_left_for_label(&clean, "5h limit")
        .or_else(|| used_percent_for_compact_label(&clean, "5h"))
        .map(|used_percent| UsageWindow {
            label: "5h".to_string(),
            used_percent,
            reset_at: None,
            reset_description: reset_desc_for_label(&clean, "5h limit"),
        });
    let secondary = legacy_percent_left_for_label(&clean, "weekly limit")
        .or_else(|| used_percent_for_compact_label(&clean, "weekly"))
        .map(|used_percent| UsageWindow {
            label: "7d".to_string(),
            used_percent,
            reset_at: None,
            reset_description: reset_desc_for_label(&clean, "weekly limit"),
        });
    let credits = credits_balance(&clean);

    if primary.is_none() && secondary.is_none() && credits.is_none() {
        return Err(CodexError::CliParse.into());
    }

    Ok(UsageSnapshot {
        provider: ProviderId::Codex,
        source: "PTY".to_string(),
        updated_at: Utc::now(),
        headline: if secondary.is_some() {
            UsageHeadline::Secondary
        } else {
            UsageHeadline::Primary
        },
        primary,
        secondary,
        tertiary: None,
        provider_cost: credits.map(|used| ProviderCost {
            used,
            limit: None,
            units: "credits".to_string(),
        }),
        identity: ProviderIdentity::default(),
    })
}

/// Finds "LABEL … N% left" and returns used_percent = 100 - N.
fn legacy_percent_left_for_label(text: &str, label: &str) -> Option<f64> {
    let lower = text.to_ascii_lowercase();
    let start = lower.find(label)?;
    let end = (start + 200).min(lower.len());
    let region = &lower[start..end];
    let left_pos = region.find("% left")?;
    let pct = last_number(&region[..left_pos])?;
    Some(100.0 - pct)
}

/// Finds compact status snippets like "· 5h 35% · weekly 48%" and returns used_percent = N.
fn used_percent_for_compact_label(text: &str, label: &str) -> Option<f64> {
    let lower = text.to_ascii_lowercase();
    let start = lower.find(label)?;
    let end = (start + 32).min(lower.len());
    let region = &lower[start + label.len()..end];
    let pct_pos = region.find('%')?;
    last_number(&region[..pct_pos])
}

/// Extracts "resets in …" text following a label's line.
fn reset_desc_for_label(text: &str, label: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let start = lower.find(label)?;
    let end = (start + 200).min(lower.len());
    let region = &text[start..end];
    let region_lower = region.to_ascii_lowercase();
    let pos = region_lower.find("resets in")?;
    let after = region[pos..].trim();
    let line_end = after.find('\n').unwrap_or(after.len());
    let desc = after[..line_end]
        .trim()
        .trim_end_matches(')')
        .trim_start_matches('(')
        .trim();
    Some(desc.to_string())
}

/// Parses "Credits: N" lines.
fn credits_balance(text: &str) -> Option<f64> {
    let lower = text.to_ascii_lowercase();
    let start = lower.find("credits:")?;
    let region = &text[start..((start + 50).min(text.len()))];
    let after = &region[region.find(':')? + 1..];
    after.trim().split_whitespace().next()?.parse().ok()
}

/// Finds the last decimal number in `text`.
fn last_number(text: &str) -> Option<f64> {
    let bytes = text.as_bytes();
    let mut end = None;
    let mut i = bytes.len();
    while i > 0 {
        i -= 1;
        if bytes[i].is_ascii_digit() {
            end = Some(i + 1);
            break;
        }
    }
    let end = end?;
    let mut start = end;
    while start > 0 {
        let b = bytes[start - 1];
        if b.is_ascii_digit() || b == b'.' {
            start -= 1;
        } else {
            break;
        }
    }
    text[start..end].parse().ok()
}

// === Binary discovery ===

fn find_codex_binary() -> Option<PathBuf> {
    // 1. Ambient PATH
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

    // 2. Login-shell PATH (needed when node version managers aren't sourced)
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

    // 3. Volta
    let p = home.join(".volta/bin/codex");
    if p.exists() {
        return Some(p);
    }

    // 4. fnm
    let p = home.join(".local/share/fnm/current/bin/codex");
    if p.exists() {
        return Some(p);
    }

    // 5. nvm — scan all installed node versions, newest first
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
        entries.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
        for entry in entries {
            let candidate = entry.path().join("bin/codex");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    // 6. npm global
    let p = home.join(".npm-global/bin/codex");
    if p.exists() {
        return Some(p);
    }

    None
}

fn forced_source() -> Option<CodexSource> {
    let raw = std::env::var(FORCE_SOURCE_ENV).ok()?;
    match raw.trim().to_ascii_lowercase().as_str() {
        "oauth" => Some(CodexSource::Oauth),
        "rpc" => Some(CodexSource::Rpc),
        "pty" => Some(CodexSource::Pty),
        _ => None,
    }
}

// === Shared helpers ===

fn normalize_window(label: &str, used_percent: f64, reset_at_epoch: i64) -> UsageWindow {
    let reset_at = DateTime::from_timestamp(reset_at_epoch, 0);
    UsageWindow {
        label: label.to_string(),
        used_percent,
        reset_at,
        reset_description: reset_at.map(|t| t.to_rfc3339()),
    }
}

fn strip_ansi(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if chars
                .peek()
                .is_some_and(|next| *next == '[' || *next == ']')
            {
                let _ = chars.next();
                while let Some(next) = chars.next() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            continue;
        }
        output.push(ch);
    }
    output
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    // ── OAuth ──────────────────────────────────────────────────────────────

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

    // ── CLI-RPC ────────────────────────────────────────────────────────────

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
        // credits balance "0" → 0.0
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

    // ── CLI-PTY ────────────────────────────────────────────────────────────

    #[test]
    fn pty_normalizes_fixture() {
        let transcript = include_str!("../../fixtures/codex/usage_cli.txt");
        let snapshot = parse_pty_snapshot(transcript).unwrap();
        assert_eq!(snapshot.provider, ProviderId::Codex);
        assert_eq!(snapshot.source, "PTY");
        // "97% left" → 100 - 97 = 3% used
        assert_eq!(snapshot.primary.as_ref().unwrap().used_percent, 3.0);
        // "76% left" → 100 - 76 = 24% used
        assert_eq!(snapshot.secondary.as_ref().unwrap().used_percent, 24.0);
        assert_eq!(snapshot.provider_cost.as_ref().unwrap().used, 0.0);
    }

    #[test]
    fn pty_same_windows_as_oauth() {
        let oauth_payload: CodexUsageResponse =
            serde_json::from_str(include_str!("../../fixtures/codex/usage_oauth.json")).unwrap();
        let oauth = normalize_oauth(oauth_payload).unwrap();
        let pty = parse_pty_snapshot(include_str!("../../fixtures/codex/usage_cli.txt")).unwrap();

        assert_eq!(
            oauth.primary.as_ref().unwrap().used_percent,
            pty.primary.as_ref().unwrap().used_percent
        );
        assert_eq!(
            oauth.secondary.as_ref().unwrap().used_percent,
            pty.secondary.as_ref().unwrap().used_percent
        );
    }

    #[test]
    fn pty_strips_ansi_before_parsing() {
        let transcript = "\u{1b}[2mCredits:\u{1b}[0m 0\n\u{1b}[1m5h limit:\u{1b}[0m 97% left (resets in 4h)\n\u{1b}[1mWeekly limit:\u{1b}[0m 76% left\n";
        let snapshot = parse_pty_snapshot(transcript).unwrap();
        assert_eq!(snapshot.primary.as_ref().unwrap().used_percent, 3.0);
        assert_eq!(snapshot.secondary.as_ref().unwrap().used_percent, 24.0);
    }

    #[test]
    fn pty_parses_compact_status_footer() {
        let transcript = "gpt-5.4 medium · yapcap · Context [     ] · 5h 35% · weekly 48%";
        let snapshot = parse_pty_snapshot(transcript).unwrap();
        assert_eq!(snapshot.primary.as_ref().unwrap().used_percent, 35.0);
        assert_eq!(snapshot.secondary.as_ref().unwrap().used_percent, 48.0);
        assert!(snapshot.provider_cost.is_none());
    }

    #[test]
    fn pty_empty_output_errors() {
        assert!(parse_pty_snapshot("").is_err());
        assert!(parse_pty_snapshot("some unrelated text\n").is_err());
    }

    #[test]
    fn pty_reset_description_extracted() {
        let transcript =
            "5h limit: 97% left (resets in 4h 22m)\nWeekly limit: 76% left (resets in 6d 12h)\n";
        let snapshot = parse_pty_snapshot(transcript).unwrap();
        assert_eq!(
            snapshot
                .primary
                .as_ref()
                .unwrap()
                .reset_description
                .as_deref(),
            Some("resets in 4h 22m")
        );
    }

    #[test]
    fn parses_forced_source_env_values() {
        unsafe {
            std::env::set_var(FORCE_SOURCE_ENV, "oauth");
        }
        assert_eq!(forced_source(), Some(CodexSource::Oauth));

        unsafe {
            std::env::set_var(FORCE_SOURCE_ENV, "rpc");
        }
        assert_eq!(forced_source(), Some(CodexSource::Rpc));

        unsafe {
            std::env::set_var(FORCE_SOURCE_ENV, "pty");
        }
        assert_eq!(forced_source(), Some(CodexSource::Pty));

        unsafe {
            std::env::set_var(FORCE_SOURCE_ENV, "unknown");
        }
        assert_eq!(forced_source(), None);

        unsafe {
            std::env::remove_var(FORCE_SOURCE_ENV);
        }
    }
}
