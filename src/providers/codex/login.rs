// SPDX-License-Identifier: MPL-2.0

use crate::account_storage::{
    NewProviderAccount, ProviderAccountStorage, ProviderAccountTokens, StoredProviderAccount,
};
use crate::auth::{CodexAuth, email_from_id_token};
use crate::config::{Config, ManagedCodexAccountConfig};
use crate::model::UsageSnapshot;
use crate::providers::codex::account::{
    create_private_dir, find_matching_account, new_account_id, normalized_email,
};
use crate::providers::codex::fetch_oauth;
use crate::providers::codex::oauth::{
    DEFAULT_CALLBACK_PORT, FALLBACK_CALLBACK_PORT, ISSUER, TOKEN_ENDPOINT, authorization_url,
    exchange_code, new_pkce, new_state, percent_decode,
};
use chrono::Utc;
use cosmic::iced::Task;
use cosmic::iced::futures::SinkExt;
#[cfg(test)]
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Debug, Clone)]
pub struct CodexLoginState {
    pub flow_id: String,
    pub status: CodexLoginStatus,
    pub login_url: Option<String>,
    pub output: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexLoginStatus {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone)]
pub enum CodexLoginEvent {
    Output {
        flow_id: String,
        line: String,
        login_url: Option<String>,
    },
    Finished {
        flow_id: String,
        result: Box<Result<CodexLoginSuccess, String>>,
    },
}

#[derive(Debug, Clone)]
pub struct CodexLoginSuccess {
    pub account: ManagedCodexAccountConfig,
    pub snapshot: Option<UsageSnapshot>,
}

pub fn prepare(config: Config) -> Result<(CodexLoginState, Task<CodexLoginEvent>), String> {
    let flow_id = new_account_id();
    let account_root = crate::config::paths().codex_accounts_dir;
    create_private_dir(&account_root)?;

    let state = CodexLoginState {
        flow_id: flow_id.clone(),
        status: CodexLoginStatus::Running,
        login_url: None,
        output: Vec::new(),
        error: None,
    };
    let stream = cosmic::iced::stream::channel(100, move |mut output| async move {
        run_login(flow_id, config, &mut output).await;
    });

    Ok((state, Task::stream(stream)))
}

async fn run_login(
    flow_id: String,
    config: Config,
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<CodexLoginEvent>,
) {
    let result = run_login_inner(&flow_id, &config, output, TOKEN_ENDPOINT).await;
    let _ = output
        .send(CodexLoginEvent::Finished {
            flow_id,
            result: Box::new(result),
        })
        .await;
}

async fn run_login_inner(
    flow_id: &str,
    config: &Config,
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<CodexLoginEvent>,
    token_endpoint: &str,
) -> Result<CodexLoginSuccess, String> {
    let auth = run_oauth_flow(flow_id, output, token_endpoint).await?;
    let snapshot = fetch_oauth(&crate::runtime::http_client(), &auth)
        .await
        .map_err(|error| tracing::warn!("Codex usage validation failed after login: {error}"))
        .ok();

    commit_login(flow_id, config, auth, snapshot)
}

fn commit_login(
    flow_id: &str,
    config: &Config,
    auth: CodexAuth,
    snapshot: Option<UsageSnapshot>,
) -> Result<CodexLoginSuccess, String> {
    let provider_account_id = snapshot
        .as_ref()
        .and_then(|s| s.identity.account_id.clone())
        .or_else(|| auth.account_id.clone());
    let login_email = snapshot
        .as_ref()
        .and_then(|s| s.identity.email.clone())
        .or_else(|| auth.id_token.as_deref().and_then(email_from_id_token));

    let login_email = login_email.ok_or_else(|| {
        "Codex login did not expose an account email; cannot create explicit account".to_string()
    })?;
    let login_email = normalized_email(&login_email);
    let existing = find_matching_account(config, Some(&login_email)).cloned();
    let storage = ProviderAccountStorage::new(crate::config::paths().codex_accounts_dir);
    let new_account =
        new_provider_account(auth, login_email, provider_account_id, snapshot.clone());
    let stored = if let Some(existing) = &existing {
        storage
            .replace_account(existing.id.clone(), new_account)
            .map_err(|error| format!("failed to update Codex account: {error}"))?
    } else {
        storage
            .replace_account(flow_id.to_string(), new_account)
            .map_err(|error| format!("failed to store Codex account: {error}"))?
    };

    let account = managed_account_from_stored(existing.as_ref(), stored);

    Ok(CodexLoginSuccess { account, snapshot })
}

async fn run_oauth_flow(
    flow_id: &str,
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<CodexLoginEvent>,
    token_endpoint: &str,
) -> Result<CodexAuth, String> {
    let listener = bind_callback_listener().await?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("failed to inspect Codex callback listener: {error}"))?
        .port();
    let redirect_uri = format!("http://localhost:{port}/auth/callback");
    let pkce = new_pkce();
    let state = new_state();
    let url = authorization_url(ISSUER, &redirect_uri, &pkce, &state);
    send_output(flow_id, format!("Open {url}"), output).await;
    open_browser(&url);

    loop {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|error| format!("failed to receive Codex OAuth callback: {error}"))?;
        if let Some(auth) = handle_callback(
            &mut stream,
            token_endpoint,
            &redirect_uri,
            &pkce.code_verifier,
            &state,
        )
        .await?
        {
            return Ok(auth);
        }
    }
}

async fn bind_callback_listener() -> Result<TcpListener, String> {
    match TcpListener::bind(("127.0.0.1", DEFAULT_CALLBACK_PORT)).await {
        Ok(listener) => Ok(listener),
        Err(primary) => TcpListener::bind(("127.0.0.1", FALLBACK_CALLBACK_PORT))
            .await
            .map_err(|fallback| {
                format!(
                    "failed to bind Codex OAuth callback ports {DEFAULT_CALLBACK_PORT} \
                    ({primary}) and {FALLBACK_CALLBACK_PORT} ({fallback})"
                )
            }),
    }
}

async fn handle_callback(
    stream: &mut TcpStream,
    token_endpoint: &str,
    redirect_uri: &str,
    code_verifier: &str,
    expected_state: &str,
) -> Result<Option<CodexAuth>, String> {
    let request = read_http_request(stream).await?;
    let Some(target) = request_target(&request) else {
        write_response(stream, 400, "Bad Request").await?;
        return Ok(None);
    };
    let Some((path, query)) = target.split_once('?') else {
        write_response(stream, 404, "Not Found").await?;
        return Ok(None);
    };
    if path != "/auth/callback" {
        write_response(stream, 404, "Not Found").await?;
        return Ok(None);
    }
    let params = parse_query(query);
    if params.get("state").map(String::as_str) != Some(expected_state) {
        write_response(stream, 400, "State mismatch").await?;
        return Ok(None);
    }
    if let Some(error) = params.get("error").filter(|error| !error.is_empty()) {
        write_response(stream, 400, "Codex sign-in failed").await?;
        return Err(format!("Codex OAuth returned {error}"));
    }
    let code = params
        .get("code")
        .filter(|code| !code.is_empty())
        .cloned()
        .ok_or_else(|| "Codex OAuth code was missing".to_string())?;
    let tokens = exchange_code(
        &crate::runtime::http_client(),
        token_endpoint,
        redirect_uri,
        code_verifier,
        &code,
    )
    .await?;
    write_response(
        stream,
        200,
        "Codex sign-in complete. You can close this tab.",
    )
    .await?;
    Ok(Some(tokens.into_auth()))
}

async fn read_http_request(stream: &mut TcpStream) -> Result<String, String> {
    let mut buffer = vec![0; 8192];
    let bytes = stream
        .read(&mut buffer)
        .await
        .map_err(|error| format!("failed to read Codex OAuth callback: {error}"))?;
    Ok(String::from_utf8_lossy(&buffer[..bytes]).to_string())
}

fn request_target(request: &str) -> Option<&str> {
    let line = request.lines().next()?;
    let mut parts = line.split_whitespace();
    match (parts.next(), parts.next()) {
        (Some("GET"), Some(target)) => Some(target),
        _ => None,
    }
}

async fn write_response(stream: &mut TcpStream, status: u16, body: &str) -> Result<(), String> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Error",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(|error| format!("failed to write Codex OAuth callback response: {error}"))
}

fn new_provider_account(
    auth: CodexAuth,
    email: String,
    provider_account_id: Option<String>,
    snapshot: Option<UsageSnapshot>,
) -> NewProviderAccount {
    NewProviderAccount {
        provider: crate::model::ProviderId::Codex,
        email,
        provider_account_id,
        organization_id: None,
        organization_name: None,
        tokens: ProviderAccountTokens {
            access_token: auth.access_token,
            refresh_token: auth.refresh_token.unwrap_or_default(),
            expires_at: auth.expires_at.unwrap_or_else(Utc::now),
            scope: Vec::new(),
            token_id: None,
        },
        snapshot,
    }
}

fn managed_account_from_stored(
    existing: Option<&ManagedCodexAccountConfig>,
    stored: StoredProviderAccount,
) -> ManagedCodexAccountConfig {
    let now = Utc::now();
    ManagedCodexAccountConfig {
        id: stored.account_ref.account_id,
        label: stored.metadata.email.clone(),
        codex_home: stored.account_dir,
        email: Some(stored.metadata.email),
        provider_account_id: stored.metadata.provider_account_id,
        created_at: existing.map_or(now, |account| account.created_at),
        updated_at: now,
        last_authenticated_at: Some(now),
    }
}

async fn send_output(
    flow_id: &str,
    line: String,
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<CodexLoginEvent>,
) {
    let clean = strip_ansi(&line);
    let login_url = find_url(&clean);
    let _ = output
        .send(CodexLoginEvent::Output {
            flow_id: flow_id.to_string(),
            line: clean,
            login_url,
        })
        .await;
}

fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else if ch != '\x1b' {
            result.push(ch);
        }
    }
    result
}

fn find_url(line: &str) -> Option<String> {
    line.split_whitespace()
        .find(|word| word.starts_with("https://") || word.starts_with("http://"))
        .map(|word| {
            word.trim_end_matches(['.', ',', ')', ']', '}', '"', '\''])
                .to_string()
        })
}

fn parse_query(query: &str) -> std::collections::HashMap<String, String> {
    query
        .split('&')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((percent_decode(key), percent_decode(value)))
        })
        .collect()
}

fn open_browser(url: &str) {
    if let Err(error) = std::process::Command::new("xdg-open").arg(url).spawn() {
        tracing::warn!(url = %url, error = %error, "failed to open Codex OAuth URL");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use chrono::TimeZone;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_state(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("yapcap-codex-login-{name}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn jwt(payload: &str) -> String {
        format!("header.{}.sig", URL_SAFE_NO_PAD.encode(payload.as_bytes()))
    }

    fn auth(email: Option<&str>, account_id: Option<&str>, access: &str) -> CodexAuth {
        let email_claim = email
            .map(|email| format!(r#""email":"{email}","#))
            .unwrap_or_default();
        let account_claim = account_id
            .map(|account_id| {
                format!(
                    r#""https://api.openai.com/auth":{{"chatgpt_account_id":"{account_id}"}}, "#
                )
            })
            .unwrap_or_default();
        CodexAuth {
            access_token: access.to_string(),
            account_id: account_id.map(str::to_string),
            refresh_token: Some("refresh".to_string()),
            id_token: Some(jwt(&format!(
                r#"{{{email_claim}{account_claim}"exp":1770000000}}"#
            ))),
            expires_at: Utc.with_ymd_and_hms(2026, 2, 2, 8, 0, 0).single(),
        }
    }

    #[test]
    fn parses_url_from_line() {
        assert_eq!(
            find_url("Open https://example.com/device and sign in."),
            Some("https://example.com/device".to_string())
        );
    }

    #[test]
    fn strips_ansi_codes() {
        assert_eq!(
            strip_ansi("\x1b[94mhttps://auth.openai.com/codex/device\x1b[0m"),
            "https://auth.openai.com/codex/device"
        );
        assert_eq!(
            strip_ansi("\x1b[90m(expires in 15 minutes)\x1b[0m"),
            "(expires in 15 minutes)"
        );
        assert_eq!(strip_ansi("plain text"), "plain text");
    }

    #[test]
    fn extracts_ansi_wrapped_url() {
        let url_line = strip_ansi("\x1b[94mhttps://auth.openai.com/codex/device\x1b[0m");
        assert_eq!(
            find_url(&url_line).as_deref(),
            Some("https://auth.openai.com/codex/device")
        );
    }

    #[test]
    fn add_flow_matches_existing_account_by_email() {
        let now = Utc::now();
        let account = ManagedCodexAccountConfig {
            id: "work".to_string(),
            label: "user@example.com".to_string(),
            codex_home: PathBuf::from("/tmp/work"),
            email: Some("user@example.com".to_string()),
            provider_account_id: Some("acct_123".to_string()),
            created_at: now,
            updated_at: now,
            last_authenticated_at: Some(now),
        };
        let config = Config {
            codex_managed_accounts: vec![account],
            ..Config::default()
        };

        let found = find_matching_account(&config, Some("USER@example.com"));

        assert_eq!(found.map(|account| account.id.as_str()), Some("work"));
    }

    #[test]
    fn commits_codex_oauth_account_to_yapcap_storage() {
        let _guard = test_support::env_lock();
        let state_root = temp_state("commit");
        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }

        let success = commit_login(
            "codex-test",
            &Config::default(),
            auth(Some("User@Example.com"), Some("acct_123"), "access-1"),
            None,
        )
        .unwrap();
        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }

        assert_eq!(success.account.id, "codex-test");
        assert_eq!(success.account.email.as_deref(), Some("user@example.com"));
        assert_eq!(
            success.account.provider_account_id.as_deref(),
            Some("acct_123")
        );
        assert!(success.account.codex_home.join("metadata.json").exists());
        assert!(success.account.codex_home.join("tokens.json").exists());

        let storage = ProviderAccountStorage::new(state_root.join("yapcap/codex-accounts"));
        let tokens = storage.load_tokens("codex-test").unwrap();
        assert_eq!(tokens.access_token, "access-1");
        assert_eq!(tokens.refresh_token, "refresh");
        assert_eq!(
            tokens.expires_at,
            Utc.with_ymd_and_hms(2026, 2, 2, 8, 0, 0).unwrap()
        );
    }

    #[test]
    fn duplicate_codex_oauth_login_updates_existing_account() {
        let _guard = test_support::env_lock();
        let state_root = temp_state("duplicate");
        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }
        let first = commit_login(
            "codex-existing",
            &Config::default(),
            auth(Some("user@example.com"), Some("acct_old"), "access-1"),
            None,
        )
        .unwrap();
        let config = Config {
            codex_managed_accounts: vec![first.account.clone()],
            selected_codex_account_ids: vec![first.account.id.clone()],
            ..Config::default()
        };

        let second = commit_login(
            "codex-new",
            &config,
            auth(Some("USER@example.com"), Some("acct_new"), "access-2"),
            None,
        )
        .unwrap();
        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }

        assert_eq!(second.account.id, "codex-existing");
        assert_eq!(
            second.account.provider_account_id.as_deref(),
            Some("acct_new")
        );
        assert!(!second.account.codex_home.ends_with("codex-new"));

        let storage = ProviderAccountStorage::new(state_root.join("yapcap/codex-accounts"));
        let tokens = storage.load_tokens("codex-existing").unwrap();
        assert_eq!(tokens.access_token, "access-2");
        assert!(storage.load_tokens("codex-new").is_err());
    }

    #[test]
    fn failed_codex_oauth_commit_leaves_accounts_unchanged() {
        let _guard = test_support::env_lock();
        let state_root = temp_state("failure");
        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }

        let result = commit_login(
            "codex-failed",
            &Config::default(),
            auth(None, Some("acct_123"), "access-1"),
            None,
        );
        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }

        assert!(result.is_err());
        assert!(
            !state_root
                .join("yapcap/codex-accounts/codex-failed")
                .exists()
        );
    }
}
