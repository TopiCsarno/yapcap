// SPDX-License-Identifier: MPL-2.0

use super::account::{find_matching_account, normalized_email};
use super::oauth::parse_token_response;
use crate::account_storage::{NewProviderAccount, ProviderAccountStorage, StoredProviderAccount};
use crate::config::{Config, ManagedClaudeAccountConfig, paths};
use crate::model::UsageSnapshot;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::Utc;
use cosmic::iced::Task;
use reqwest::Client;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::io::Read as _;

const AUTHORIZE_ENDPOINT: &str = "https://claude.ai/oauth/authorize";
const TOKEN_ENDPOINT: &str = "https://console.anthropic.com/v1/oauth/token";
const REDIRECT_URI: &str = "https://console.anthropic.com/oauth/code/callback";
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const SCOPE: &str = "user:profile";

#[derive(Debug, Clone)]
pub struct ClaudeLoginState {
    pub flow_id: String,
    pub status: ClaudeLoginStatus,
    pub login_url: Option<String>,
    pub code_input: String,
    pub output: Vec<String>,
    pub error: Option<String>,
    pub redirect_uri: String,
    pub code_verifier: String,
    pub state_token: String,
    pub target_account_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeLoginStatus {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone)]
pub enum ClaudeLoginEvent {
    Finished {
        flow_id: String,
        result: Box<Result<ClaudeLoginSuccess, String>>,
    },
}

#[derive(Debug, Clone)]
pub struct ClaudeLoginSuccess {
    pub account: ManagedClaudeAccountConfig,
    pub snapshot: Option<UsageSnapshot>,
}

pub fn prepare() -> ClaudeLoginState {
    prepare_with_target(None)
}

pub fn prepare_targeted(target_account_id: String) -> ClaudeLoginState {
    prepare_with_target(Some(target_account_id))
}

fn prepare_with_target(target_account_id: Option<String>) -> ClaudeLoginState {
    let flow_id = new_flow_id();
    let code_verifier = new_code_verifier();
    let state_token = code_verifier.clone();
    let authorization_url = authorization_url(REDIRECT_URI, &code_verifier, &state_token);
    open_browser(&authorization_url);

    ClaudeLoginState {
        flow_id: flow_id.clone(),
        status: ClaudeLoginStatus::Running,
        login_url: Some(authorization_url.clone()),
        code_input: String::new(),
        output: vec!["Paste the Claude authentication code from the browser".to_string()],
        error: None,
        redirect_uri: REDIRECT_URI.to_string(),
        code_verifier,
        state_token,
        target_account_id,
    }
}

pub fn submit_code(state: &ClaudeLoginState, config: Config) -> Task<ClaudeLoginEvent> {
    let pending = PendingOAuthLogin {
        flow_id: state.flow_id.clone(),
        config,
        redirect_uri: state.redirect_uri.clone(),
        code_verifier: state.code_verifier.clone(),
        state_token: state.state_token.clone(),
        code_input: state.code_input.clone(),
        target_account_id: state.target_account_id.clone(),
    };
    Task::perform(run_login(pending), |event| event)
}

struct PendingOAuthLogin {
    flow_id: String,
    config: Config,
    redirect_uri: String,
    code_verifier: String,
    state_token: String,
    code_input: String,
    target_account_id: Option<String>,
}

struct OAuthInput<'a> {
    redirect_uri: &'a str,
    code_verifier: &'a str,
    state_token: &'a str,
    code_input: &'a str,
}

async fn run_login(pending: PendingOAuthLogin) -> ClaudeLoginEvent {
    let input = OAuthInput {
        redirect_uri: &pending.redirect_uri,
        code_verifier: &pending.code_verifier,
        state_token: &pending.state_token,
        code_input: &pending.code_input,
    };
    let result = run_login_inner(
        &pending.config,
        &input,
        TOKEN_ENDPOINT,
        &Client::new(),
        pending.target_account_id.as_deref(),
    )
    .await;
    ClaudeLoginEvent::Finished {
        flow_id: pending.flow_id,
        result: Box::new(result),
    }
}

async fn run_login_inner(
    config: &Config,
    input: &OAuthInput<'_>,
    token_endpoint: &str,
    client: &Client,
    target_account_id: Option<&str>,
) -> Result<ClaudeLoginSuccess, String> {
    let (code, state) = parse_authorization_code_input(input.code_input, input.state_token)?;
    let raw = exchange_code(
        client,
        token_endpoint,
        input.redirect_uri,
        input.code_verifier,
        &code,
        &state,
    )
    .await?;
    let new_account = parse_token_response(&raw, Utc::now())
        .map_err(|error| error.to_string())?
        .into_new_account()
        .map_err(|error| error.to_string())?;
    commit_login(config, new_account, target_account_id)
}

fn commit_login(
    config: &Config,
    new_account: NewProviderAccount,
    target_account_id: Option<&str>,
) -> Result<ClaudeLoginSuccess, String> {
    let email = new_account.email.clone();
    let storage = ProviderAccountStorage::new(paths().claude_accounts_dir);
    if let Some(target_id) = target_account_id {
        let target = config
            .claude_managed_accounts
            .iter()
            .find(|a| a.id == target_id)
            .ok_or_else(|| "target Claude account not found".to_string())?;
        if let Some(target_email) = &target.email
            && normalized_email(&email) != normalized_email(target_email)
        {
            return Err(format!(
                "Re-authentication failed: signed in as {email} but this account is {target_email}"
            ));
        }
        let stored = storage
            .replace_account(target_id.to_string(), new_account)
            .map_err(|error| format!("failed to update Claude account: {error}"))?;
        return Ok(ClaudeLoginSuccess {
            account: managed_account_from_stored(Some(target), stored),
            snapshot: None,
        });
    }
    let existing = find_matching_account(config, Some(&email)).cloned();
    let stored = if let Some(existing) = &existing {
        storage
            .replace_account(existing.id.clone(), new_account)
            .map_err(|error| format!("failed to update Claude account: {error}"))?
    } else {
        storage
            .create_account(new_account)
            .map_err(|error| format!("failed to store Claude account: {error}"))?
    };
    Ok(ClaudeLoginSuccess {
        account: managed_account_from_stored(existing.as_ref(), stored),
        snapshot: None,
    })
}

fn managed_account_from_stored(
    existing: Option<&ManagedClaudeAccountConfig>,
    stored: StoredProviderAccount,
) -> ManagedClaudeAccountConfig {
    let now = Utc::now();
    ManagedClaudeAccountConfig {
        id: stored.account_ref.account_id,
        label: stored.metadata.email.clone(),
        config_dir: stored.account_dir,
        email: Some(stored.metadata.email),
        organization: stored.metadata.organization_name,
        subscription_type: existing.and_then(|account| account.subscription_type.clone()),
        created_at: existing.map_or(now, |account| account.created_at),
        updated_at: now,
        last_authenticated_at: Some(now),
    }
}

fn parse_authorization_code_input(
    input: &str,
    expected_state: &str,
) -> Result<(String, String), String> {
    let input = input.trim();
    if input.starts_with("http://")
        || input.starts_with("https://")
        || input.contains("code=")
        || input.contains("state=")
    {
        return Err("Paste the authentication code from your browser.".to_string());
    }
    let Some((code, state)) = input.split_once('#') else {
        return Err("Paste the authentication code from your browser.".to_string());
    };
    let code = percent_decode(code.trim());
    let state = percent_decode(state.trim());
    if state != expected_state {
        return Err("Claude OAuth state did not match".to_string());
    }
    if code.is_empty() {
        return Err("Claude OAuth code was missing".to_string());
    }
    Ok((code, state))
}

async fn exchange_code(
    client: &Client,
    token_endpoint: &str,
    redirect_uri: &str,
    code_verifier: &str,
    code: &str,
    state: &str,
) -> Result<String, String> {
    let payload = json!({
        "code": code,
        "state": state,
        "grant_type": "authorization_code",
        "client_id": CLIENT_ID,
        "redirect_uri": redirect_uri,
        "code_verifier": code_verifier,
    });
    let response = client
        .post(token_endpoint)
        .header("User-Agent", "claude-code/2.0.32")
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("Claude OAuth token exchange failed: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("failed to read Claude OAuth token response: {error}"))?;
    if !status.is_success() {
        if status == reqwest::StatusCode::BAD_REQUEST {
            return Err("invalid-code".to_string());
        }
        return Err(format!(
            "Claude OAuth token exchange returned {status} \
            (grant_type=authorization_code redirect_uri={redirect_uri} \
            code_length={} state_length={} verifier_length={})",
            code.len(),
            state.len(),
            code_verifier.len()
        ));
    }
    Ok(body)
}

fn authorization_url(redirect_uri: &str, code_verifier: &str, state_token: &str) -> String {
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()));
    let params = [
        ("code", "true"),
        ("client_id", CLIENT_ID),
        ("response_type", "code"),
        ("redirect_uri", redirect_uri),
        ("scope", SCOPE),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("state", state_token),
    ];
    let query = params
        .into_iter()
        .map(|(key, value)| format!("{}={}", percent_encode(key), percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{AUTHORIZE_ENDPOINT}?{query}")
}

fn new_flow_id() -> String {
    format!("claude-{}", Utc::now().timestamp_millis())
}

fn new_code_verifier() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes())
}

fn random_bytes() -> [u8; 32] {
    let mut bytes = [0; 32];
    if let Ok(mut file) = std::fs::File::open("/dev/urandom")
        && file.read_exact(&mut bytes).is_ok()
    {
        return bytes;
    }
    let fallback = format!(
        "{}:{}:{}",
        Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        std::process::id(),
        std::thread::current().name().unwrap_or("thread")
    );
    let digest = Sha256::digest(fallback.as_bytes());
    bytes.copy_from_slice(&digest);
    bytes
}

fn percent_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            write!(out, "%{byte:02X}").expect("writing to a string cannot fail");
        }
    }
    out
}

fn percent_decode(value: &str) -> String {
    let mut out = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Ok(hex) = u8::from_str_radix(&value[index + 1..index + 3], 16)
        {
            out.push(hex);
            index += 3;
        } else if bytes[index] == b'+' {
            out.push(b' ');
            index += 1;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn open_browser(url: &str) {
    if let Err(error) = std::process::Command::new("xdg-open").arg(url).spawn() {
        tracing::warn!(url = %url, error = %error, "failed to open Claude OAuth URL");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn temp_state(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("yapcap-claude-login-{name}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn token_response(email: &str, access: &str) -> String {
        format!(
            r#"{{
                "access_token": "{access}",
                "refresh_token": "refresh",
                "expires_in": 28800,
                "scope": "user:profile",
                "token_uuid": "token-id",
                "account": {{"uuid": "account-id", "email_address": "{email}"}},
                "organization": {{"uuid": "org-id", "name": "Org"}}
            }}"#
        )
    }

    async fn token_server(body: String) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = vec![0; 4096];
            let bytes = stream.read(&mut buffer).await.unwrap();
            let request = String::from_utf8_lossy(&buffer[..bytes]).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            request
        });
        (format!("http://{addr}/token"), handle)
    }

    #[test]
    fn authorization_url_uses_pkce_and_supported_console_redirect() {
        let url = authorization_url(REDIRECT_URI, "verifier", "state");

        assert!(url.starts_with(AUTHORIZE_ENDPOINT));
        assert!(url.contains("code=true"));
        assert!(url.contains("client_id=9d1c250a-e61b-44d9-88ed-5944d1962f5e"));
        assert!(url.contains(
            "redirect_uri=https%3A%2F%2Fconsole.anthropic.com%2Foauth%2Fcode%2Fcallback"
        ));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(!url.contains("verifier"));
    }

    #[test]
    fn parses_console_code_and_rejects_wrong_state() {
        assert_eq!(
            parse_authorization_code_input("abc%20123#ok", "ok").unwrap(),
            ("abc 123".to_string(), "ok".to_string())
        );
        assert!(parse_authorization_code_input("abc#bad", "ok").is_err());
    }

    #[test]
    fn rejects_callback_urls_and_raw_queries() {
        let url_error = parse_authorization_code_input(
            "https://console.anthropic.com/oauth/code/callback?code=abc%20123&state=ok",
            "ok",
        )
        .unwrap_err();
        let fragment_url_error = parse_authorization_code_input(
            "https://console.anthropic.com/oauth/code/callback?code=abc%20123#ok",
            "ok",
        )
        .unwrap_err();
        let query_error =
            parse_authorization_code_input("code=abc%20123&state=ok", "ok").unwrap_err();
        let query_fragment_error =
            parse_authorization_code_input("code=abc%20123#ok", "ok").unwrap_err();

        assert_eq!(
            url_error,
            "Paste the authentication code from your browser.".to_string()
        );
        assert_eq!(
            fragment_url_error,
            "Paste the authentication code from your browser.".to_string()
        );
        assert_eq!(
            query_error,
            "Paste the authentication code from your browser.".to_string()
        );
        assert_eq!(
            query_fragment_error,
            "Paste the authentication code from your browser.".to_string()
        );
    }

    #[test]
    fn malformed_authorization_input_uses_code_focused_error() {
        assert_eq!(
            parse_authorization_code_input("not-a-code", "ok").unwrap_err(),
            "Paste the authentication code from your browser.".to_string()
        );
    }

    #[tokio::test]
    async fn exchange_sends_json_with_state() {
        let body = token_response("user@example.com", "tok");
        let (token_url, request_handle) = token_server(body).await;

        let _ = exchange_code(
            &Client::new(),
            &token_url,
            REDIRECT_URI,
            "my-verifier",
            "my-code",
            "my-state",
        )
        .await
        .unwrap();

        let raw = request_handle.await.unwrap();
        assert!(
            raw.contains("application/json"),
            "Content-Type must be application/json"
        );
        assert!(raw.contains("\"code\":\"my-code\""));
        assert!(raw.contains("\"state\":\"my-state\""));
        assert!(raw.contains("\"code_verifier\":\"my-verifier\""));
        assert!(raw.contains("\"grant_type\":\"authorization_code\""));
    }

    #[tokio::test]
    async fn exchanges_console_code_and_commits_account() {
        let _guard = test_support::env_lock();
        let state_root = temp_state("commit");
        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }
        let (token_url, request_handle) =
            token_server(token_response("User@Example.com", "access-1")).await;

        let input = OAuthInput {
            redirect_uri: REDIRECT_URI,
            code_verifier: "verifier",
            state_token: "state-ok",
            code_input: "code-1#state-ok",
        };
        let success = run_login_inner(&Config::default(), &input, &token_url, &Client::new(), None)
            .await
            .unwrap();

        let token_request = request_handle.await.unwrap();
        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }

        assert!(token_request.contains("\"grant_type\":\"authorization_code\""));
        assert!(token_request.contains("\"code\":\"code-1\""));
        assert!(token_request.contains("\"code_verifier\":\"verifier\""));
        assert!(token_request.contains("\"state\":\"state-ok\""));
        assert_eq!(success.account.email.as_deref(), Some("user@example.com"));
        assert_eq!(success.account.label, "user@example.com");
        assert!(success.account.config_dir.join("metadata.json").exists());
        assert!(success.account.config_dir.join("tokens.json").exists());
        assert!(
            !success
                .account
                .config_dir
                .join(".credentials.json")
                .exists()
        );
    }

    #[tokio::test]
    async fn duplicate_email_login_updates_existing_account() {
        let _guard = test_support::env_lock();
        let state_root = temp_state("duplicate");
        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }
        let first = commit_login(
            &Config::default(),
            parse_token_response(&token_response("user@example.com", "access-1"), Utc::now())
                .unwrap()
                .into_new_account()
                .unwrap(),
            None,
        )
        .unwrap();
        let mut config = Config::default();
        config.claude_managed_accounts.push(first.account.clone());
        let second = commit_login(
            &config,
            parse_token_response(&token_response("USER@example.com", "access-2"), Utc::now())
                .unwrap()
                .into_new_account()
                .unwrap(),
            None,
        )
        .unwrap();
        let storage = ProviderAccountStorage::new(paths().claude_accounts_dir);
        let tokens = storage.load_tokens(&first.account.id).unwrap();
        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }

        assert_eq!(second.account.id, first.account.id);
        assert_eq!(tokens.access_token, "access-2");
    }

    #[tokio::test]
    async fn targeted_reauth_updates_same_account() {
        let _guard = test_support::env_lock();
        let state_root = temp_state("targeted-reauth");
        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }
        let first = commit_login(
            &Config::default(),
            parse_token_response(&token_response("user@example.com", "access-1"), Utc::now())
                .unwrap()
                .into_new_account()
                .unwrap(),
            None,
        )
        .unwrap();
        let mut config = Config::default();
        config.claude_managed_accounts.push(first.account.clone());

        let second = commit_login(
            &config,
            parse_token_response(&token_response("user@example.com", "access-2"), Utc::now())
                .unwrap()
                .into_new_account()
                .unwrap(),
            Some(first.account.id.as_str()),
        )
        .unwrap();
        let storage = ProviderAccountStorage::new(paths().claude_accounts_dir);
        let tokens = storage.load_tokens(&first.account.id).unwrap();
        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }

        assert_eq!(second.account.id, first.account.id);
        assert_eq!(tokens.access_token, "access-2");
    }

    #[tokio::test]
    async fn targeted_reauth_rejects_mismatched_email() {
        let _guard = test_support::env_lock();
        let state_root = temp_state("reauth-mismatch");
        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }
        let first = commit_login(
            &Config::default(),
            parse_token_response(&token_response("user@example.com", "access-1"), Utc::now())
                .unwrap()
                .into_new_account()
                .unwrap(),
            None,
        )
        .unwrap();
        let mut config = Config::default();
        config.claude_managed_accounts.push(first.account.clone());

        let result = commit_login(
            &config,
            parse_token_response(&token_response("other@example.com", "access-2"), Utc::now())
                .unwrap()
                .into_new_account()
                .unwrap(),
            Some(first.account.id.as_str()),
        );
        let storage = ProviderAccountStorage::new(paths().claude_accounts_dir);
        let tokens = storage.load_tokens(&first.account.id).unwrap();
        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }

        assert!(result.is_err());
        assert_eq!(tokens.access_token, "access-1");
    }
}
