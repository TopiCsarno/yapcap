// SPDX-License-Identifier: MPL-2.0

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use chrono::Utc;
use cosmic::iced::Task;
use cosmic::iced::futures::SinkExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::account::{self, find_matching_account, new_account_id, normalized_email};
use super::oauth::{self, GrokTokenResponse};
use super::{DEFAULT_BILLING_URL, fetch_at};
use crate::account_storage::{
    NewProviderAccount, ProviderAccountStorage, ProviderAccountTokens, StoredProviderAccount,
};
use crate::config::{Config, ManagedGrokAccountConfig, paths};
use crate::model::ProviderId;

const SUCCESS_PAGE_BODY: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>YapCap: Grok sign-in complete</title></head>\
     <body style=\"font-family: sans-serif; padding: 32px;\">\
     <h1>YapCap: Grok sign-in complete</h1>\
     <p>You can close this tab and return to YapCap.</p>\
     </body></html>";

#[derive(Debug, Clone)]
pub struct GrokLoginState {
    pub flow_id: String,
    pub status: GrokLoginStatus,
    pub login_url: Option<String>,
    pub error: Option<String>,
    pub importing_from_host_cli: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrokLoginStatus {
    Running,
    Failed,
}

#[derive(Debug, Clone)]
pub enum GrokLoginEvent {
    LoginUrl {
        flow_id: String,
        url: String,
    },
    Finished {
        flow_id: String,
        result: Box<Result<GrokLoginSuccess, String>>,
    },
}

#[derive(Debug, Clone)]
pub struct GrokLoginSuccess {
    pub account: ManagedGrokAccountConfig,
}

pub fn prepare(config: Config) -> Result<(GrokLoginState, Task<GrokLoginEvent>), String> {
    prepare_with_target(config, None)
}

pub fn prepare_targeted(
    account_id: String,
    config: Config,
) -> Result<(GrokLoginState, Task<GrokLoginEvent>), String> {
    if !config
        .grok_managed_accounts
        .iter()
        .any(|account| account.id == account_id)
    {
        return Err(format!("Grok account {account_id} no longer exists"));
    }
    prepare_with_target(config, Some(account_id))
}

fn prepare_with_target(
    config: Config,
    target_account_id: Option<String>,
) -> Result<(GrokLoginState, Task<GrokLoginEvent>), String> {
    let flow_id = new_account_id();
    let state = GrokLoginState {
        flow_id: flow_id.clone(),
        status: GrokLoginStatus::Running,
        login_url: None,
        error: None,
        importing_from_host_cli: false,
    };
    let stream = cosmic::iced::stream::channel(100, move |mut output| async move {
        run_login(flow_id, config, target_account_id, &mut output).await;
    });
    Ok((state, Task::stream(stream)))
}

pub fn prepare_host_import(
    config: Config,
    target_account_id: Option<String>,
) -> Result<(GrokLoginState, Task<GrokLoginEvent>), String> {
    if let Some(ref target_id) = target_account_id
        && !config
            .grok_managed_accounts
            .iter()
            .any(|account| account.id == target_id.as_str())
    {
        return Err(format!("Grok account {target_id} no longer exists"));
    }
    let flow_id = new_account_id();
    let state = GrokLoginState {
        flow_id: flow_id.clone(),
        status: GrokLoginStatus::Running,
        login_url: None,
        error: None,
        importing_from_host_cli: true,
    };
    let task = Task::perform(
        run_host_import(flow_id.clone(), config, target_account_id),
        move |result| GrokLoginEvent::Finished {
            flow_id,
            result: Box::new(result),
        },
    );
    Ok((state, task))
}

async fn run_login(
    flow_id: String,
    config: Config,
    target_account_id: Option<String>,
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<GrokLoginEvent>,
) {
    let result = run_login_inner(
        &flow_id,
        &config,
        target_account_id.as_deref(),
        output,
        oauth::TOKEN_URL,
        DEFAULT_BILLING_URL,
    )
    .await;
    let _ = output
        .send(GrokLoginEvent::Finished {
            flow_id,
            result: Box::new(result),
        })
        .await;
}

pub(crate) async fn run_login_inner(
    flow_id: &str,
    config: &Config,
    target_account_id: Option<&str>,
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<GrokLoginEvent>,
    token_url: &str,
    billing_url: &str,
) -> Result<GrokLoginSuccess, String> {
    let tokens = run_oauth_flow(flow_id, output, token_url).await?;
    let claims = oauth::decode_jwt_claims(&tokens.access_token).or_else(|| {
        tokens
            .id_token
            .as_deref()
            .and_then(oauth::decode_jwt_claims)
    });
    let email = claims
        .as_ref()
        .and_then(|c| c.email.clone())
        .filter(|e| !e.trim().is_empty());
    let provider_account_id = claims
        .as_ref()
        .and_then(|c| c.sub.clone())
        .filter(|s| !s.trim().is_empty());
    let team_id = claims
        .as_ref()
        .and_then(|c| c.team_id.clone())
        .filter(|t| !t.trim().is_empty());
    let plan = claims
        .as_ref()
        .and_then(|c| c.tier.clone())
        .filter(|p| !p.trim().is_empty());

    let provider_tokens = ProviderAccountTokens {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        expires_at: tokens.expires_at,
        scope: tokens
            .scope
            .map(|s| s.split_whitespace().map(String::from).collect())
            .unwrap_or_default(),
        token_id: tokens.id_token,
    };

    let client = crate::runtime::http_client();
    let ctx = CommitContext {
        flow_id,
        config,
        target_account_id,
        client: &client,
        billing_url,
        token_url,
    };
    let identity = CommitIdentity {
        tokens: provider_tokens,
        email,
        provider_account_id,
        team_id,
        plan,
    };

    commit_and_fetch(&ctx, identity).await
}

async fn run_oauth_flow(
    flow_id: &str,
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<GrokLoginEvent>,
    token_url: &str,
) -> Result<GrokTokenResponse, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|error| format!("failed to bind Grok OAuth callback listener: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("failed to inspect Grok callback listener: {error}"))?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let pkce = oauth::new_pkce();
    let state = oauth::new_state();
    let url = oauth::authorization_url(&redirect_uri, &pkce, &state);
    send_login_url(flow_id, url.clone(), output).await;
    open_browser(&url);

    let client = crate::runtime::http_client();
    let timeout_duration = Duration::from_secs(300);
    tokio::time::timeout(timeout_duration, async {
        loop {
            let (mut stream, _) = listener
                .accept()
                .await
                .map_err(|error| format!("failed to receive Grok OAuth callback: {error}"))?;
            if let Some(tokens) = handle_callback(
                &mut stream,
                &client,
                token_url,
                &redirect_uri,
                &pkce.code_verifier,
                &state,
            )
            .await?
            {
                return Ok(tokens);
            }
        }
    })
    .await
    .map_err(|_| "Grok OAuth login timed out waiting for browser callback".to_string())?
}

pub(crate) async fn handle_callback(
    stream: &mut TcpStream,
    client: &reqwest::Client,
    token_url: &str,
    redirect_uri: &str,
    code_verifier: &str,
    expected_state: &str,
) -> Result<Option<GrokTokenResponse>, String> {
    let request = read_http_request(stream).await?;
    let Some(target) = request_target(&request) else {
        write_response(stream, 400, "text/plain; charset=utf-8", "Bad Request").await?;
        return Ok(None);
    };
    let Some((path, query)) = target.split_once('?') else {
        write_response(stream, 404, "text/plain; charset=utf-8", "Not Found").await?;
        return Ok(None);
    };
    if path != "/callback" {
        write_response(stream, 404, "text/plain; charset=utf-8", "Not Found").await?;
        return Ok(None);
    }
    let params = parse_query(query);
    if params.get("state").map(String::as_str) != Some(expected_state) {
        write_response(stream, 400, "text/plain; charset=utf-8", "State mismatch").await?;
        return Err("Grok OAuth state nonce did not match".to_string());
    }
    if let Some(error) = params.get("error").filter(|error| !error.is_empty()) {
        write_response(
            stream,
            400,
            "text/plain; charset=utf-8",
            "Grok sign-in failed",
        )
        .await?;
        return Err(format!("Grok OAuth returned {error}"));
    }
    let code = params
        .get("code")
        .filter(|code| !code.is_empty())
        .cloned()
        .ok_or_else(|| "Grok OAuth code was missing".to_string())?;
    let tokens = oauth::exchange_code(client, &code, code_verifier, redirect_uri, token_url)
        .await
        .map_err(|error| format!("Grok OAuth code exchange failed: {error}"))?;

    write_response(stream, 200, "text/html; charset=utf-8", SUCCESS_PAGE_BODY).await?;
    Ok(Some(tokens))
}

async fn read_http_request(stream: &mut TcpStream) -> Result<String, String> {
    let mut buffer = vec![0; 8192];
    let bytes = stream
        .read(&mut buffer)
        .await
        .map_err(|error| format!("failed to read Grok OAuth callback: {error}"))?;
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
        .map_err(|error| format!("failed to write Grok OAuth callback response: {error}"))
}

async fn run_host_import(
    flow_id: String,
    config: Config,
    target_account_id: Option<String>,
) -> Result<GrokLoginSuccess, String> {
    run_host_import_with(
        flow_id,
        config,
        target_account_id,
        None,
        &crate::runtime::http_client(),
        DEFAULT_BILLING_URL,
        oauth::TOKEN_URL,
    )
    .await
}

pub(crate) async fn run_host_import_with(
    flow_id: String,
    config: Config,
    target_account_id: Option<String>,
    custom_auth_path: Option<&Path>,
    client: &reqwest::Client,
    billing_url: &str,
    token_url: &str,
) -> Result<GrokLoginSuccess, String> {
    let auth_path = match custom_auth_path {
        Some(path) => path.to_path_buf(),
        None => account::host_auth_file_path()
            .ok_or_else(|| "Could not determine host Grok auth.json path".to_string())?,
    };

    let creds = account::read_host_credentials(&auth_path)
        .ok_or_else(|| "No valid Grok host credentials found in auth.json".to_string())?;

    let claims = oauth::decode_jwt_claims(&creds.access_token);

    let email = creds
        .email
        .or_else(|| claims.as_ref().and_then(|c| c.email.clone()))
        .filter(|e| !e.trim().is_empty());
    let provider_account_id = creds
        .user_id
        .or_else(|| claims.as_ref().and_then(|c| c.sub.clone()))
        .filter(|s| !s.trim().is_empty());
    let team_id = creds
        .team_id
        .or_else(|| claims.as_ref().and_then(|c| c.team_id.clone()))
        .filter(|t| !t.trim().is_empty());
    let plan = claims
        .as_ref()
        .and_then(|c| c.tier.clone())
        .filter(|p| !p.trim().is_empty());

    let expires_at = creds
        .expires_at
        .unwrap_or_else(|| Utc::now() + chrono::Duration::hours(1));

    let tokens = ProviderAccountTokens {
        access_token: creds.access_token,
        refresh_token: creds.refresh_token.unwrap_or_default(),
        expires_at,
        scope: Vec::new(),
        token_id: None,
    };

    let ctx = CommitContext {
        flow_id: &flow_id,
        config: &config,
        target_account_id: target_account_id.as_deref(),
        client,
        billing_url,
        token_url,
    };
    let identity = CommitIdentity {
        tokens,
        email,
        provider_account_id,
        team_id,
        plan,
    };

    commit_and_fetch(&ctx, identity).await
}

struct CommitContext<'a> {
    flow_id: &'a str,
    config: &'a Config,
    target_account_id: Option<&'a str>,
    client: &'a reqwest::Client,
    billing_url: &'a str,
    token_url: &'a str,
}

pub(crate) struct CommitIdentity {
    pub tokens: ProviderAccountTokens,
    pub email: Option<String>,
    pub provider_account_id: Option<String>,
    pub team_id: Option<String>,
    pub plan: Option<String>,
}

fn resolve_target_account(
    ctx: &CommitContext<'_>,
    identity: &CommitIdentity,
) -> Result<(Option<ManagedGrokAccountConfig>, String), String> {
    let existing = if let Some(target_id) = ctx.target_account_id {
        let target = ctx
            .config
            .grok_managed_accounts
            .iter()
            .find(|account| account.id == target_id)
            .ok_or_else(|| format!("Grok account {target_id} no longer exists"))?;
        if let Some(ref target_email) = target.email
            && let Some(ref new_email) = identity.email
            && normalized_email(target_email) != normalized_email(new_email)
        {
            return Err(
                "This is a different Grok account. The existing account was not updated."
                    .to_string(),
            );
        }
        if let Some(ref target_uid) = target.provider_account_id
            && let Some(ref new_uid) = identity.provider_account_id
            && target_uid != new_uid
        {
            return Err(
                "This is a different Grok account. The existing account was not updated."
                    .to_string(),
            );
        }
        Some(target.clone())
    } else {
        find_matching_account(
            ctx.config,
            identity.email.as_deref(),
            identity.provider_account_id.as_deref(),
        )
        .cloned()
    };

    let target_id = existing
        .as_ref()
        .map(|a| a.id.clone())
        .unwrap_or_else(|| ctx.flow_id.to_string());

    Ok((existing, target_id))
}

fn store_provider_account(
    target_id: &str,
    existing: Option<&ManagedGrokAccountConfig>,
    identity: &CommitIdentity,
) -> Result<StoredProviderAccount, String> {
    let storage_email = identity
        .email
        .clone()
        .or_else(|| existing.and_then(|a| a.email.clone()))
        .unwrap_or_else(|| "Grok account".to_string());

    let new_account = NewProviderAccount {
        provider: ProviderId::Grok,
        email: storage_email,
        provider_account_id: identity
            .provider_account_id
            .clone()
            .or_else(|| existing.and_then(|a| a.provider_account_id.clone())),
        organization_id: None,
        organization_name: identity
            .team_id
            .clone()
            .or_else(|| existing.and_then(|a| a.team_id.clone())),
        tokens: identity.tokens.clone(),
        snapshot: None,
    };

    let storage = ProviderAccountStorage::new(paths().grok_accounts_dir);
    storage
        .replace_account(target_id.to_string(), new_account)
        .map_err(|error| format!("failed to store Grok account: {error}"))
}

async fn commit_and_fetch(
    ctx: &CommitContext<'_>,
    mut identity: CommitIdentity,
) -> Result<GrokLoginSuccess, String> {
    let (existing, target_id) = resolve_target_account(ctx, &identity)?;
    let stored = store_provider_account(&target_id, existing.as_ref(), &identity)?;

    let account_dir = stored.account_dir.clone();

    let snapshot = fetch_at(
        ctx.client,
        &target_id,
        account_dir,
        ctx.billing_url,
        ctx.token_url,
    )
    .await
    .map_err(|error| tracing::warn!("Grok initial snapshot fetch failed after login: {error}"))
    .ok();

    identity.plan = snapshot
        .as_ref()
        .and_then(|s| s.identity.plan.clone())
        .or(identity.plan);

    let account = managed_account_from_stored(existing.as_ref(), &stored, &identity);

    Ok(GrokLoginSuccess { account })
}

fn managed_account_from_stored(
    existing: Option<&ManagedGrokAccountConfig>,
    stored: &StoredProviderAccount,
    identity: &CommitIdentity,
) -> ManagedGrokAccountConfig {
    let now = Utc::now();
    let email = identity
        .email
        .clone()
        .or_else(|| existing.and_then(|a| a.email.clone()));
    let provider_account_id = identity
        .provider_account_id
        .clone()
        .or_else(|| existing.and_then(|a| a.provider_account_id.clone()));
    let team_id = identity
        .team_id
        .clone()
        .or_else(|| existing.and_then(|a| a.team_id.clone()));
    let plan = identity
        .plan
        .clone()
        .or_else(|| existing.and_then(|a| a.plan.clone()));
    let label = existing
        .map(|a| a.label.clone())
        .filter(|l| !l.trim().is_empty() && l != "Grok account")
        .or_else(|| email.clone())
        .unwrap_or_else(|| "Grok account".to_string());

    ManagedGrokAccountConfig {
        id: stored.account_ref.account_id.clone(),
        label,
        config_dir: stored.account_dir.clone(),
        email,
        provider_account_id,
        team_id,
        plan,
        created_at: existing.map_or(now, |a| a.created_at),
        updated_at: now,
        last_authenticated_at: Some(now),
    }
}

async fn send_login_url(
    flow_id: &str,
    url: String,
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<GrokLoginEvent>,
) {
    let _ = output
        .send(GrokLoginEvent::LoginUrl {
            flow_id: flow_id.to_string(),
            url,
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
        tracing::warn!(url = %url, error = %error, "failed to open Grok OAuth URL");
    }
}
