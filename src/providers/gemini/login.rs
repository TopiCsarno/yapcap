// SPDX-License-Identifier: MPL-2.0

use crate::account_storage::{
    NewProviderAccount, ProviderAccountStorage, ProviderAccountTokens, StoredProviderAccount,
};
use crate::config::{Config, ManagedGeminiAccountConfig};
use crate::model::ProviderId;
use crate::providers::gemini::account::{
    create_private_dir, find_matching_account, new_account_id, normalized_email,
};
use crate::providers::gemini::code_assist::{LoadCodeAssist, load_code_assist};
use crate::providers::gemini::id_token::{IdTokenClaims, decode};
use crate::providers::gemini::oauth::{
    GeminiOAuthTokens, TOKEN_ENDPOINT, authorization_url_with_hint, exchange_code, new_pkce,
    new_state, percent_decode,
};
use chrono::Utc;
use cosmic::iced::Task;
use cosmic::iced::futures::SinkExt;
use std::collections::HashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const SUCCESS_PAGE_BODY: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>Gemini sign-in</title></head>\
     <body style=\"font-family: sans-serif; padding: 32px;\">\
     <h1>Signed in to Gemini</h1>\
     <p>You can close this tab and return to YapCap.</p>\
     </body></html>";

#[derive(Debug, Clone)]
pub struct GeminiLoginState {
    pub flow_id: String,
    pub status: GeminiLoginStatus,
    pub login_url: Option<String>,
    pub output: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeminiLoginStatus {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone)]
pub enum GeminiLoginEvent {
    Output {
        flow_id: String,
        line: String,
        login_url: Option<String>,
    },
    Finished {
        flow_id: String,
        result: Box<Result<GeminiLoginSuccess, String>>,
    },
}

#[derive(Debug, Clone)]
pub struct GeminiLoginSuccess {
    pub account: ManagedGeminiAccountConfig,
}

pub fn prepare(config: Config) -> Result<(GeminiLoginState, Task<GeminiLoginEvent>), String> {
    prepare_with_options(config, ReauthOptions::default())
}

pub fn prepare_for_reauth(
    config: Config,
    account_id: &str,
) -> Result<(GeminiLoginState, Task<GeminiLoginEvent>), String> {
    let account = config
        .gemini_managed_accounts
        .iter()
        .find(|account| account.id == account_id)
        .ok_or_else(|| format!("Gemini account {account_id} no longer exists"))?;
    let expected_email = normalized_email(&account.email);
    if expected_email.is_empty() {
        return Err("Gemini account has no email on record".to_string());
    }
    prepare_with_options(
        config.clone(),
        ReauthOptions {
            login_hint: Some(expected_email.clone()),
            expected_email: Some(expected_email),
        },
    )
}

#[derive(Debug, Clone, Default)]
struct ReauthOptions {
    login_hint: Option<String>,
    expected_email: Option<String>,
}

fn prepare_with_options(
    config: Config,
    options: ReauthOptions,
) -> Result<(GeminiLoginState, Task<GeminiLoginEvent>), String> {
    let flow_id = new_account_id();
    let account_root = crate::config::paths().gemini_accounts_dir;
    create_private_dir(&account_root)?;

    let state = GeminiLoginState {
        flow_id: flow_id.clone(),
        status: GeminiLoginStatus::Running,
        login_url: None,
        output: Vec::new(),
        error: None,
    };
    let stream = cosmic::iced::stream::channel(100, move |mut output| async move {
        run_login(flow_id, config, options, &mut output).await;
    });
    Ok((state, Task::stream(stream)))
}

async fn run_login(
    flow_id: String,
    config: Config,
    options: ReauthOptions,
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<GeminiLoginEvent>,
) {
    let result = run_login_inner(&flow_id, &config, &options, output, TOKEN_ENDPOINT).await;
    let _ = output
        .send(GeminiLoginEvent::Finished {
            flow_id,
            result: Box::new(result),
        })
        .await;
}

async fn run_login_inner(
    flow_id: &str,
    config: &Config,
    options: &ReauthOptions,
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<GeminiLoginEvent>,
    token_endpoint: &str,
) -> Result<GeminiLoginSuccess, String> {
    let tokens = run_oauth_flow(
        flow_id,
        options.login_hint.as_deref(),
        output,
        token_endpoint,
    )
    .await?;
    let claims = decode(&tokens.id_token)
        .map_err(|error| format!("Gemini id_token decode failed: {error}"))?;
    if let Some(expected) = options.expected_email.as_deref() {
        verify_reauth_email_match(expected, &claims.email)?;
    }
    let client = crate::runtime::http_client();
    let code_assist = load_code_assist(&client, &tokens.access_token)
        .await
        .unwrap_or_else(|error| {
            tracing::warn!("Gemini loadCodeAssist failed after login: {error}");
            LoadCodeAssist::default()
        });
    commit_login(flow_id, config, tokens, claims, code_assist)
}

pub(super) fn verify_reauth_email_match(expected: &str, actual: &str) -> Result<(), String> {
    let expected = normalized_email(expected);
    let actual = normalized_email(actual);
    if expected == actual {
        Ok(())
    } else {
        Err("Re-authentication must use the same email as the original account.".to_string())
    }
}

fn commit_login(
    flow_id: &str,
    config: &Config,
    tokens: GeminiOAuthTokens,
    claims: IdTokenClaims,
    code_assist: LoadCodeAssist,
) -> Result<GeminiLoginSuccess, String> {
    let login_email = normalized_email(&claims.email);
    let existing = find_matching_account(config, &login_email).cloned();
    let storage = ProviderAccountStorage::new(crate::config::paths().gemini_accounts_dir);
    let new_account = new_provider_account(&tokens, &claims, &login_email);
    let stored = if let Some(existing) = &existing {
        storage
            .replace_account(existing.id.clone(), new_account)
            .map_err(|error| format!("failed to update Gemini account: {error}"))?
    } else {
        storage
            .replace_account(flow_id.to_string(), new_account)
            .map_err(|error| format!("failed to store Gemini account: {error}"))?
    };

    let account = managed_account_from_stored(existing.as_ref(), &stored, &claims, &code_assist);
    Ok(GeminiLoginSuccess { account })
}

async fn run_oauth_flow(
    flow_id: &str,
    login_hint: Option<&str>,
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<GeminiLoginEvent>,
    token_endpoint: &str,
) -> Result<GeminiOAuthTokens, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|error| format!("failed to bind Gemini OAuth callback: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("failed to inspect Gemini callback listener: {error}"))?
        .port();
    let redirect_uri = format!("http://localhost:{port}/oauth/callback");
    let pkce = new_pkce();
    let state = new_state();
    let url = authorization_url_with_hint(&redirect_uri, &pkce, &state, login_hint);
    send_output(flow_id, format!("Open {url}"), Some(url.clone()), output).await;
    open_browser(&url);

    loop {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|error| format!("failed to receive Gemini OAuth callback: {error}"))?;
        if let Some(tokens) = handle_callback(
            &mut stream,
            token_endpoint,
            &redirect_uri,
            &pkce.code_verifier,
            &state,
        )
        .await?
        {
            return Ok(tokens);
        }
    }
}

async fn handle_callback(
    stream: &mut TcpStream,
    token_endpoint: &str,
    redirect_uri: &str,
    code_verifier: &str,
    expected_state: &str,
) -> Result<Option<GeminiOAuthTokens>, String> {
    let request = read_http_request(stream).await?;
    let Some(target) = request_target(&request) else {
        write_response(stream, 400, "text/plain; charset=utf-8", "Bad Request").await?;
        return Ok(None);
    };
    let Some((path, query)) = target.split_once('?') else {
        write_response(stream, 404, "text/plain; charset=utf-8", "Not Found").await?;
        return Ok(None);
    };
    if path != "/oauth/callback" {
        write_response(stream, 404, "text/plain; charset=utf-8", "Not Found").await?;
        return Ok(None);
    }
    let params = parse_query(query);
    if params.get("state").map(String::as_str) != Some(expected_state) {
        write_response(stream, 400, "text/plain; charset=utf-8", "State mismatch").await?;
        return Err("Gemini OAuth state nonce did not match".to_string());
    }
    if let Some(error) = params.get("error").filter(|error| !error.is_empty()) {
        write_response(
            stream,
            400,
            "text/plain; charset=utf-8",
            "Gemini sign-in failed",
        )
        .await?;
        return Err(format!("Gemini OAuth returned {error}"));
    }
    let code = params
        .get("code")
        .filter(|code| !code.is_empty())
        .cloned()
        .ok_or_else(|| "Gemini OAuth code was missing".to_string())?;
    let tokens = exchange_code(
        &crate::runtime::http_client(),
        token_endpoint,
        redirect_uri,
        code_verifier,
        &code,
    )
    .await?;
    write_response(stream, 200, "text/html; charset=utf-8", SUCCESS_PAGE_BODY).await?;
    Ok(Some(tokens))
}

async fn read_http_request(stream: &mut TcpStream) -> Result<String, String> {
    let mut buffer = vec![0; 8192];
    let bytes = stream
        .read(&mut buffer)
        .await
        .map_err(|error| format!("failed to read Gemini OAuth callback: {error}"))?;
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

async fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
) -> Result<(), String> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Error",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(|error| format!("failed to write Gemini OAuth callback response: {error}"))
}

fn new_provider_account(
    tokens: &GeminiOAuthTokens,
    claims: &IdTokenClaims,
    email: &str,
) -> NewProviderAccount {
    NewProviderAccount {
        provider: ProviderId::Gemini,
        email: email.to_string(),
        provider_account_id: Some(claims.sub.clone()),
        organization_id: None,
        organization_name: None,
        tokens: ProviderAccountTokens {
            access_token: tokens.access_token.clone(),
            refresh_token: tokens.refresh_token.clone(),
            expires_at: tokens.expires_at,
            scope: tokens.scope.clone(),
            token_id: None,
        },
        snapshot: None,
    }
}

fn managed_account_from_stored(
    existing: Option<&ManagedGeminiAccountConfig>,
    stored: &StoredProviderAccount,
    claims: &IdTokenClaims,
    code_assist: &LoadCodeAssist,
) -> ManagedGeminiAccountConfig {
    let now = Utc::now();
    ManagedGeminiAccountConfig {
        id: stored.account_ref.account_id.clone(),
        label: stored.metadata.email.clone(),
        account_root: stored.account_dir.clone(),
        email: stored.metadata.email.clone(),
        sub: claims.sub.clone(),
        hd: claims.hd.clone(),
        last_tier_id: code_assist.tier_id.clone(),
        last_cloudaicompanion_project: code_assist.cloudaicompanion_project.clone(),
        created_at: existing.map_or(now, |account| account.created_at),
        updated_at: now,
        last_authenticated_at: Some(now),
    }
}

async fn send_output(
    flow_id: &str,
    line: String,
    login_url: Option<String>,
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<GeminiLoginEvent>,
) {
    let _ = output
        .send(GeminiLoginEvent::Output {
            flow_id: flow_id.to_string(),
            line,
            login_url,
        })
        .await;
}

fn parse_query(query: &str) -> HashMap<String, String> {
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
        tracing::warn!(url = %url, error = %error, "failed to open Gemini OAuth URL");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_state(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("yapcap-gemini-login-{name}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn jwt(payload: &str) -> String {
        format!("header.{}.sig", URL_SAFE_NO_PAD.encode(payload.as_bytes()))
    }

    fn sample_tokens() -> GeminiOAuthTokens {
        GeminiOAuthTokens {
            access_token: "access-1".to_string(),
            refresh_token: "refresh-1".to_string(),
            id_token: jwt(r#"{"email":"User@Example.com","sub":"abc"}"#),
            expires_at: Utc::now() + chrono::Duration::hours(1),
            scope: vec!["openid".to_string()],
        }
    }

    fn sample_claims() -> IdTokenClaims {
        IdTokenClaims {
            email: "User@Example.com".to_string(),
            sub: "abc".to_string(),
            hd: None,
            name: Some("Test User".to_string()),
            email_verified: true,
        }
    }

    fn sample_code_assist() -> LoadCodeAssist {
        LoadCodeAssist {
            tier_id: Some("free-tier".to_string()),
            cloudaicompanion_project: Some("example-project".to_string()),
        }
    }

    #[test]
    fn reauth_rejects_email_mismatch() {
        let error = verify_reauth_email_match("alice@example.com", "bob@example.com").unwrap_err();
        assert!(
            error.contains("same email"),
            "expected explanatory error: {error}"
        );
    }

    #[test]
    fn reauth_accepts_case_insensitive_email_match() {
        assert!(verify_reauth_email_match("Alice@Example.com", "  ALICE@example.COM ").is_ok());
    }

    #[test]
    fn commits_login_writes_managed_account_and_tokens() {
        let _guard = test_support::env_lock();
        let state_root = temp_state("commit");
        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }

        let success = commit_login(
            "gemini-test",
            &Config::default(),
            sample_tokens(),
            sample_claims(),
            sample_code_assist(),
        )
        .unwrap();

        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }

        assert_eq!(success.account.id, "gemini-test");
        assert_eq!(success.account.email, "user@example.com");
        assert_eq!(success.account.label, "user@example.com");
        assert_eq!(success.account.sub, "abc");
        assert_eq!(success.account.last_tier_id.as_deref(), Some("free-tier"));
        assert_eq!(
            success.account.last_cloudaicompanion_project.as_deref(),
            Some("example-project")
        );
        assert!(success.account.account_root.join("metadata.json").exists());
        assert!(success.account.account_root.join("tokens.json").exists());

        let storage = ProviderAccountStorage::new(state_root.join("yapcap/gemini-accounts"));
        let tokens = storage.load_tokens("gemini-test").unwrap();
        assert_eq!(tokens.access_token, "access-1");
        assert_eq!(tokens.refresh_token, "refresh-1");
        assert_eq!(tokens.scope, vec!["openid".to_string()]);
    }

    #[test]
    fn duplicate_login_updates_existing_account_directory() {
        let _guard = test_support::env_lock();
        let state_root = temp_state("duplicate");
        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }
        let first = commit_login(
            "gemini-existing",
            &Config::default(),
            sample_tokens(),
            sample_claims(),
            sample_code_assist(),
        )
        .unwrap();
        let config = Config {
            gemini_managed_accounts: vec![first.account.clone()],
            selected_gemini_account_ids: vec![first.account.id.clone()],
            ..Config::default()
        };
        let second = commit_login(
            "gemini-new",
            &config,
            GeminiOAuthTokens {
                access_token: "access-2".to_string(),
                refresh_token: "refresh-2".to_string(),
                ..sample_tokens()
            },
            sample_claims(),
            sample_code_assist(),
        )
        .unwrap();
        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }

        assert_eq!(second.account.id, "gemini-existing");
        assert!(!second.account.account_root.ends_with("gemini-new"));
        let storage = ProviderAccountStorage::new(state_root.join("yapcap/gemini-accounts"));
        let tokens = storage.load_tokens("gemini-existing").unwrap();
        assert_eq!(tokens.access_token, "access-2");
        assert!(storage.load_tokens("gemini-new").is_err());
    }
}
