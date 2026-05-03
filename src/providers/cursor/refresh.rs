// SPDX-License-Identifier: MPL-2.0

use crate::account_storage::ProviderAccountStorage;
use crate::config::ManagedCursorAccountConfig;
use crate::error::CursorError;
use crate::model::{ProviderId, ProviderIdentity, UsageHeadline, UsageSnapshot, UsageWindow};
use crate::providers::cursor::scan::{
    REFRESH_ENDPOINT, build_session_cookie, refresh_access_token_at,
};
use chrono::{DateTime, Duration, Utc};
use reqwest::header::{COOKIE, HeaderMap, HeaderValue};
use serde::Deserialize;

const USAGE_ENDPOINT: &str = "https://cursor.com/api/usage-summary";
const IDENTITY_ENDPOINT: &str = "https://cursor.com/api/auth/me";

#[derive(Debug, Deserialize)]
struct CursorUsageResponse {
    #[serde(rename = "billingCycleStart")]
    pub billing_cycle_start: String,
    #[serde(rename = "billingCycleEnd")]
    pub billing_cycle_end: String,
    #[serde(rename = "membershipType")]
    pub membership_type: Option<String>,
    #[serde(rename = "individualUsage")]
    pub individual_usage: CursorIndividualUsage,
}

#[derive(Debug, Deserialize)]
struct CursorIndividualUsage {
    pub plan: CursorPlanUsage,
}

#[derive(Debug, Deserialize)]
struct CursorPlanUsage {
    #[serde(rename = "totalPercentUsed")]
    pub total: f32,
    #[serde(rename = "autoPercentUsed")]
    pub auto_mode: f32,
    #[serde(rename = "apiPercentUsed")]
    pub api: f32,
}

#[derive(Debug, Deserialize)]
struct CursorIdentityResponse {
    pub email: Option<String>,
    pub name: Option<String>,
}

pub async fn fetch(
    client: &reqwest::Client,
    account: &ManagedCursorAccountConfig,
) -> Result<UsageSnapshot, CursorError> {
    fetch_at(
        client,
        account,
        USAGE_ENDPOINT,
        IDENTITY_ENDPOINT,
        REFRESH_ENDPOINT,
    )
    .await
}

async fn fetch_at(
    client: &reqwest::Client,
    account: &ManagedCursorAccountConfig,
    usage_endpoint: &str,
    identity_endpoint: &str,
    token_endpoint: &str,
) -> Result<UsageSnapshot, CursorError> {
    let storage = ProviderAccountStorage::new(crate::config::paths().cursor_accounts_dir);
    let mut tokens = storage
        .load_tokens(&account.id)
        .ok()
        .filter(|t| !t.access_token.trim().is_empty())
        .ok_or(CursorError::Unauthorized)?;

    let Some(token_id) = tokens.token_id.clone() else {
        return fetch_with_cookie_at(
            client,
            tokens.access_token.trim(),
            usage_endpoint,
            identity_endpoint,
        )
        .await;
    };

    if tokens.expires_at <= Utc::now() + Duration::minutes(5) {
        match refresh_access_token_at(client, &tokens.refresh_token, token_endpoint).await {
            Ok((new_token, new_expiry)) => {
                tokens.access_token = new_token;
                tokens.expires_at = new_expiry;
                let _ = storage.save_tokens(&account.id, &tokens);
            }
            Err(ref e) if is_permanent_refresh_failure(e) => return Err(CursorError::Unauthorized),
            Err(_) => {}
        }
    }

    let cookie = build_session_cookie(&token_id, &tokens.access_token);
    match fetch_with_cookie_at(client, &cookie, usage_endpoint, identity_endpoint).await {
        Err(CursorError::Unauthorized) => {
            match refresh_access_token_at(client, &tokens.refresh_token, token_endpoint).await {
                Ok((new_token, new_expiry)) => {
                    tokens.access_token = new_token;
                    tokens.expires_at = new_expiry;
                    let _ = storage.save_tokens(&account.id, &tokens);
                    let new_cookie = build_session_cookie(&token_id, &tokens.access_token);
                    fetch_with_cookie_at(client, &new_cookie, usage_endpoint, identity_endpoint)
                        .await
                }
                Err(ref e) if is_permanent_refresh_failure(e) => Err(CursorError::Unauthorized),
                Err(e) => Err(e),
            }
        }
        result => result,
    }
}

fn is_permanent_refresh_failure(e: &CursorError) -> bool {
    matches!(e, CursorError::TokenRefreshLogout)
        || matches!(e, CursorError::TokenRefreshFailed { status }
            if *status >= 400 && *status < 500 && *status != 429)
}

async fn fetch_with_cookie_at(
    client: &reqwest::Client,
    cookie_header: &str,
    usage_endpoint: &str,
    identity_endpoint: &str,
) -> Result<UsageSnapshot, CursorError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        COOKIE,
        HeaderValue::from_str(cookie_header).map_err(CursorError::InvalidCookieHeader)?,
    );

    let usage_response = client
        .get(usage_endpoint)
        .headers(headers.clone())
        .send()
        .await
        .map_err(CursorError::UsageRequest)?;
    if matches!(
        usage_response.status(),
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
    ) {
        return Err(CursorError::Unauthorized);
    }
    let usage_response = usage_response
        .error_for_status()
        .map_err(CursorError::UsageEndpoint)?;
    let usage: CursorUsageResponse = usage_response
        .json()
        .await
        .map_err(CursorError::DecodeUsage)?;

    let identity_response = client
        .get(identity_endpoint)
        .headers(headers)
        .send()
        .await
        .map_err(CursorError::IdentityRequest)?;
    let identity = if identity_response.status().is_success() {
        Some(
            identity_response
                .json::<CursorIdentityResponse>()
                .await
                .map_err(CursorError::DecodeIdentity)?,
        )
    } else {
        None
    };

    normalize(usage, identity)
}

fn normalize(
    usage: CursorUsageResponse,
    identity: Option<CursorIdentityResponse>,
) -> Result<UsageSnapshot, CursorError> {
    let reset_at = DateTime::parse_from_rfc3339(&usage.billing_cycle_end)
        .map_err(|source| CursorError::InvalidBillingCycleEnd {
            value: usage.billing_cycle_end.clone(),
            source,
        })?
        .with_timezone(&Utc);
    let started_at = DateTime::parse_from_rfc3339(&usage.billing_cycle_start)
        .map_err(|source| CursorError::InvalidBillingCycleEnd {
            value: usage.billing_cycle_start.clone(),
            source,
        })?
        .with_timezone(&Utc);
    let window_seconds = (reset_at - started_at).num_seconds();

    let windows = vec![
        window(
            "Total",
            usage.individual_usage.plan.total,
            reset_at,
            window_seconds,
        ),
        window(
            "Auto + Composer",
            usage.individual_usage.plan.auto_mode,
            reset_at,
            window_seconds,
        ),
        window(
            "API",
            usage.individual_usage.plan.api,
            reset_at,
            window_seconds,
        ),
    ];

    Ok(UsageSnapshot {
        provider: ProviderId::Cursor,
        source: "Managed Account".to_string(),
        updated_at: Utc::now(),
        headline: UsageHeadline::first_available(&windows),
        windows,
        provider_cost: None,
        identity: ProviderIdentity {
            email: identity.as_ref().and_then(|value| value.email.clone()),
            account_id: None,
            plan: usage.membership_type,
            display_name: identity.and_then(|value| value.name),
        },
    })
}

fn window(
    label: &str,
    used_percent: f32,
    reset_at: DateTime<Utc>,
    window_seconds: i64,
) -> UsageWindow {
    UsageWindow {
        label: label.to_string(),
        used_percent,
        reset_at: Some(reset_at),
        window_seconds: Some(window_seconds),
        reset_description: Some(reset_at.to_rfc3339()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_storage::{
        NewProviderAccount, ProviderAccountStorage, ProviderAccountTokens,
    };
    use crate::config::paths;
    use crate::model::ProviderId;
    use crate::test_support;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    struct MockResponse {
        method: &'static str,
        path: &'static str,
        status: u16,
        body: String,
    }

    async fn server(
        responses: Vec<MockResponse>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buffer = vec![0u8; 8192];
                let n = stream.read(&mut buffer).await.unwrap();
                let req = String::from_utf8_lossy(&buffer[..n]).to_string();
                let first_line = &req[..req.find('\n').unwrap_or(req.len())];
                assert!(
                    req.starts_with(&format!(
                        "{} {} HTTP/1.1\r\n",
                        response.method, response.path
                    )),
                    "expected {} {}, got: {first_line}",
                    response.method,
                    response.path,
                );
                let raw = format!(
                    "HTTP/1.1 {} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    response.status,
                    if response.status == 200 { "OK" } else { "ERR" },
                    response.body.len(),
                    response.body,
                );
                stream.write_all(raw.as_bytes()).await.unwrap();
                requests.push(req);
            }
            requests
        });
        (format!("http://{addr}"), handle)
    }

    fn make_test_jwt(exp: i64) -> String {
        use base64::Engine;
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"HS256\"}");
        let payload = URL_SAFE_NO_PAD
            .encode(format!("{{\"sub\":\"auth0|user_test\",\"exp\":{exp}}}").as_bytes());
        format!("{header}.{payload}.fakesig")
    }

    fn test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("yapcap-cursor-refresh-{name}-{nanos}"))
    }

    fn create_test_account(
        access_token: &str,
        refresh_token: &str,
        expires_at: DateTime<Utc>,
    ) -> ManagedCursorAccountConfig {
        let id = "test-cursor-refresh-001".to_string();
        let storage = ProviderAccountStorage::new(paths().cursor_accounts_dir);
        storage
            .replace_account(
                id.clone(),
                NewProviderAccount {
                    provider: ProviderId::Cursor,
                    email: "user@example.com".to_string(),
                    provider_account_id: None,
                    organization_id: None,
                    organization_name: None,
                    tokens: ProviderAccountTokens {
                        access_token: access_token.to_string(),
                        refresh_token: refresh_token.to_string(),
                        expires_at,
                        scope: Vec::new(),
                        token_id: Some("user_test".to_string()),
                    },
                    snapshot: None,
                },
            )
            .unwrap();
        ManagedCursorAccountConfig {
            id,
            email: "user@example.com".to_string(),
            label: "user@example.com".to_string(),
            account_root: PathBuf::from("/tmp"),
            display_name: None,
            plan: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_authenticated_at: Some(Utc::now()),
        }
    }

    fn usage_body() -> String {
        r#"{"billingCycleStart":"2026-04-01T00:00:00Z","billingCycleEnd":"2026-05-01T00:00:00Z","membershipType":"pro","individualUsage":{"plan":{"totalPercentUsed":50.0,"autoPercentUsed":30.0,"apiPercentUsed":20.0}}}"#.to_string()
    }

    fn identity_body() -> String {
        r#"{"email":"user@example.com","name":"Test User"}"#.to_string()
    }

    fn refresh_body(access_token: &str) -> String {
        format!(r#"{{"access_token":"{access_token}","shouldLogout":false}}"#)
    }

    #[test]
    fn normalizes_fixture() {
        let usage: CursorUsageResponse =
            serde_json::from_str(include_str!("../../../fixtures/cursor/usage_summary.json"))
                .unwrap();
        let identity: CursorIdentityResponse =
            serde_json::from_str(include_str!("../../../fixtures/cursor/auth_me.json")).unwrap();
        let snapshot = normalize(usage, Some(identity)).unwrap();
        assert_eq!(snapshot.provider, ProviderId::Cursor);
        assert!((snapshot.windows[0].used_percent - 56.933_333_f32).abs() < 0.001);
        assert!((snapshot.windows[1].used_percent - 29.828_571_f32).abs() < 0.001);
        assert_eq!(snapshot.windows[2].used_percent, 100.0);
        assert_eq!(snapshot.identity.plan.as_deref(), Some("pro"));
    }

    #[tokio::test]
    async fn reactive_refresh_recovers_from_unauthorized() {
        let _guard = test_support::env_lock();
        let state_root = test_dir("reactive-ok");
        unsafe { std::env::set_var("XDG_STATE_HOME", &state_root) };

        let old_token = make_test_jwt(Utc::now().timestamp() + 3600);
        let new_token = make_test_jwt(Utc::now().timestamp() + 7200);
        let account =
            create_test_account(&old_token, "refresh_tok", Utc::now() + Duration::hours(1));

        let (base, handle) = server(vec![
            MockResponse {
                method: "GET",
                path: "/api/usage-summary",
                status: 401,
                body: "{}".to_string(),
            },
            MockResponse {
                method: "POST",
                path: "/oauth/token",
                status: 200,
                body: refresh_body(&new_token),
            },
            MockResponse {
                method: "GET",
                path: "/api/usage-summary",
                status: 200,
                body: usage_body(),
            },
            MockResponse {
                method: "GET",
                path: "/api/auth/me",
                status: 200,
                body: identity_body(),
            },
        ])
        .await;

        fetch_at(
            &reqwest::Client::new(),
            &account,
            &format!("{base}/api/usage-summary"),
            &format!("{base}/api/auth/me"),
            &format!("{base}/oauth/token"),
        )
        .await
        .unwrap();

        let requests = handle.await.unwrap();
        assert_eq!(requests.len(), 4);
        assert!(requests[1].contains("\"refresh_token\":\"refresh_tok\""));

        let storage = ProviderAccountStorage::new(paths().cursor_accounts_dir);
        let saved = storage.load_tokens(&account.id).unwrap();
        assert_eq!(saved.access_token, new_token);
        assert_eq!(saved.refresh_token, "refresh_tok");

        unsafe { std::env::remove_var("XDG_STATE_HOME") };
    }

    #[tokio::test]
    async fn reactive_permanent_refresh_failure_returns_unauthorized() {
        let _guard = test_support::env_lock();
        let state_root = test_dir("reactive-perm");
        unsafe { std::env::set_var("XDG_STATE_HOME", &state_root) };

        let token = make_test_jwt(Utc::now().timestamp() + 3600);
        let account = create_test_account(&token, "refresh_tok", Utc::now() + Duration::hours(1));

        let (base, handle) = server(vec![
            MockResponse {
                method: "GET",
                path: "/api/usage-summary",
                status: 401,
                body: "{}".to_string(),
            },
            MockResponse {
                method: "POST",
                path: "/oauth/token",
                status: 400,
                body: "{}".to_string(),
            },
        ])
        .await;

        let error = fetch_at(
            &reqwest::Client::new(),
            &account,
            &format!("{base}/api/usage-summary"),
            &format!("{base}/api/auth/me"),
            &format!("{base}/oauth/token"),
        )
        .await
        .unwrap_err();

        assert!(matches!(error, CursorError::Unauthorized));
        assert_eq!(handle.await.unwrap().len(), 2);

        unsafe { std::env::remove_var("XDG_STATE_HOME") };
    }

    #[tokio::test]
    async fn proactive_refresh_updates_tokens_before_fetch() {
        let _guard = test_support::env_lock();
        let state_root = test_dir("proactive-ok");
        unsafe { std::env::set_var("XDG_STATE_HOME", &state_root) };

        let new_token = make_test_jwt(Utc::now().timestamp() + 7200);
        let account = create_test_account(
            &make_test_jwt(Utc::now().timestamp() - 60),
            "refresh_tok",
            Utc::now() - Duration::minutes(1),
        );

        let (base, handle) = server(vec![
            MockResponse {
                method: "POST",
                path: "/oauth/token",
                status: 200,
                body: refresh_body(&new_token),
            },
            MockResponse {
                method: "GET",
                path: "/api/usage-summary",
                status: 200,
                body: usage_body(),
            },
            MockResponse {
                method: "GET",
                path: "/api/auth/me",
                status: 200,
                body: identity_body(),
            },
        ])
        .await;

        fetch_at(
            &reqwest::Client::new(),
            &account,
            &format!("{base}/api/usage-summary"),
            &format!("{base}/api/auth/me"),
            &format!("{base}/oauth/token"),
        )
        .await
        .unwrap();

        let requests = handle.await.unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests[0].contains("\"refresh_token\":\"refresh_tok\""));

        let storage = ProviderAccountStorage::new(paths().cursor_accounts_dir);
        let saved = storage.load_tokens(&account.id).unwrap();
        assert_eq!(saved.access_token, new_token);

        unsafe { std::env::remove_var("XDG_STATE_HOME") };
    }
}
