// SPDX-License-Identifier: MPL-2.0

use super::*;
use crate::account_storage::NewProviderAccount;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

struct MockResponse {
    method: &'static str,
    path: &'static str,
    status: u16,
    body: String,
}

async fn server(responses: Vec<MockResponse>) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let mut requests = Vec::new();
        for response in responses {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = vec![0; 8192];
            let bytes = stream.read(&mut buffer).await.unwrap();
            let request = String::from_utf8_lossy(&buffer[..bytes]).to_string();
            assert!(request.starts_with(&format!(
                "{} {} HTTP/1.1\r\n",
                response.method, response.path
            )));
            let status_text = if response.status == 200 { "OK" } else { "ERR" };
            let raw = format!(
                "HTTP/1.1 {} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                response.status,
                status_text,
                response.body.len(),
                response.body
            );
            stream.write_all(raw.as_bytes()).await.unwrap();
            requests.push(request);
        }
        requests
    });
    (format!("http://{addr}"), handle)
}

fn usage_body() -> String {
    let reset_at = (Utc::now() + Duration::hours(1)).timestamp();
    format!(
        r#"{{
            "email": "user@example.com",
            "account_id": "acct_123",
            "plan_type": "pro",
            "rate_limit": {{
                "primary_window": {{
                    "used_percent": 12.5,
                    "limit_window_seconds": 18000,
                    "reset_at": {reset_at}
                }}
            }}
        }}"#
    )
}

fn token_body(access_token: &str, refresh_token: &str) -> String {
    format!(
        r#"{{
            "access_token": "{access_token}",
            "refresh_token": "{refresh_token}",
            "expires_in": 3600
        }}"#
    )
}

fn create_account(
    storage: &ProviderAccountStorage,
    expires_at: DateTime<Utc>,
) -> (String, PathBuf) {
    let stored = storage
        .create_account(NewProviderAccount {
            provider: ProviderId::Codex,
            email: "user@example.com".to_string(),
            provider_account_id: Some("acct_123".to_string()),
            organization_id: None,
            organization_name: None,
            tokens: ProviderAccountTokens {
                access_token: "old-access".to_string(),
                refresh_token: "old-refresh".to_string(),
                expires_at,
                scope: Vec::new(),
                token_id: None,
            },
            snapshot: None,
        })
        .unwrap();
    (stored.account_ref.account_id, stored.account_dir)
}

#[tokio::test]
async fn valid_access_token_fetches_usage_without_refresh() {
    let temp = tempfile::tempdir().unwrap();
    let storage = ProviderAccountStorage::new(temp.path());
    let (account_id, account_dir) = create_account(&storage, Utc::now() + Duration::hours(1));
    let (base_url, handle) = server(vec![MockResponse {
        method: "GET",
        path: "/usage",
        status: 200,
        body: usage_body(),
    }])
    .await;

    let snapshot = fetch_at(
        &reqwest::Client::new(),
        &account_id,
        account_dir,
        &format!("{base_url}/usage"),
        &format!("{base_url}/token"),
    )
    .await
    .unwrap();

    let requests = handle.await.unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("authorization: Bearer old-access"));
    assert_eq!(snapshot.identity.email.as_deref(), Some("user@example.com"));
    assert_eq!(
        storage.load_tokens(&account_id).unwrap().access_token,
        "old-access"
    );
}

#[tokio::test]
async fn expired_access_token_refreshes_before_usage_and_persists_rotation() {
    let temp = tempfile::tempdir().unwrap();
    let storage = ProviderAccountStorage::new(temp.path());
    let (account_id, account_dir) = create_account(&storage, Utc::now() - Duration::minutes(1));
    let (base_url, handle) = server(vec![
        MockResponse {
            method: "POST",
            path: "/token",
            status: 200,
            body: token_body("new-access", "new-refresh"),
        },
        MockResponse {
            method: "GET",
            path: "/usage",
            status: 200,
            body: usage_body(),
        },
    ])
    .await;

    fetch_at(
        &reqwest::Client::new(),
        &account_id,
        account_dir,
        &format!("{base_url}/usage"),
        &format!("{base_url}/token"),
    )
    .await
    .unwrap();

    let requests = handle.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].contains("refresh_token=old-refresh"));
    assert!(requests[1].contains("authorization: Bearer new-access"));
    let tokens = storage.load_tokens(&account_id).unwrap();
    assert_eq!(tokens.access_token, "new-access");
    assert_eq!(tokens.refresh_token, "new-refresh");
    assert!(tokens.expires_at > Utc::now());
}

#[tokio::test]
async fn usage_auth_failure_refreshes_and_retries_once() {
    let temp = tempfile::tempdir().unwrap();
    let storage = ProviderAccountStorage::new(temp.path());
    let (account_id, account_dir) = create_account(&storage, Utc::now() + Duration::hours(1));
    let (base_url, handle) = server(vec![
        MockResponse {
            method: "GET",
            path: "/usage",
            status: 401,
            body: "{}".to_string(),
        },
        MockResponse {
            method: "POST",
            path: "/token",
            status: 200,
            body: token_body("retry-access", "retry-refresh"),
        },
        MockResponse {
            method: "GET",
            path: "/usage",
            status: 200,
            body: usage_body(),
        },
    ])
    .await;

    fetch_at(
        &reqwest::Client::new(),
        &account_id,
        account_dir,
        &format!("{base_url}/usage"),
        &format!("{base_url}/token"),
    )
    .await
    .unwrap();

    let requests = handle.await.unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].contains("authorization: Bearer old-access"));
    assert!(requests[2].contains("authorization: Bearer retry-access"));
    assert_eq!(
        storage.load_tokens(&account_id).unwrap().refresh_token,
        "retry-refresh"
    );
}

#[tokio::test]
async fn permanent_refresh_failure_does_not_call_usage() {
    let temp = tempfile::tempdir().unwrap();
    let storage = ProviderAccountStorage::new(temp.path());
    let (account_id, account_dir) = create_account(&storage, Utc::now() - Duration::minutes(1));
    let (base_url, handle) = server(vec![MockResponse {
        method: "POST",
        path: "/token",
        status: 400,
        body: "{}".to_string(),
    }])
    .await;

    let error = fetch_at(
        &reqwest::Client::new(),
        &account_id,
        account_dir,
        &format!("{base_url}/usage"),
        &format!("{base_url}/token"),
    )
    .await
    .unwrap_err();

    assert!(error.requires_user_action());
    assert!(!error.is_transient());
    assert_eq!(handle.await.unwrap().len(), 1);
    let tokens = storage.load_tokens(&account_id).unwrap();
    assert_eq!(tokens.access_token, "old-access");
    assert_eq!(tokens.refresh_token, "old-refresh");
}

#[test]
fn system_active_account_id_matches_provider_account_id() {
    use crate::config::ManagedCodexAccountConfig;
    use chrono::Utc;
    use std::io::Write;
    use tempfile::NamedTempFile;

    let id_token = "eyJhbGciOiJSUzI1NiJ9.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOiB7ImNoYXRncHRfdXNlcl9pZCI6ICJ1c2VyLWFiYy0xMjMifX0.fakesig";
    let auth_json = format!(r#"{{"tokens":{{"id_token":"{id_token}"}}}}"#);
    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(auth_json.as_bytes()).unwrap();

    let accounts = vec![ManagedCodexAccountConfig {
        id: "acct-1".to_string(),
        label: "Test".to_string(),
        codex_home: std::path::PathBuf::from("/tmp"),
        email: Some("user@example.com".to_string()),
        provider_account_id: Some("user-abc-123".to_string()),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: None,
    }];

    let result = system_active_account_id(&accounts, tmp.path());
    assert_eq!(result.as_deref(), Some("acct-1"));
}

#[test]
fn system_active_account_id_returns_none_when_no_match() {
    use crate::config::ManagedCodexAccountConfig;
    use chrono::Utc;
    use std::io::Write;
    use tempfile::NamedTempFile;

    let id_token = "eyJhbGciOiJSUzI1NiJ9.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOiB7ImNoYXRncHRfdXNlcl9pZCI6ICJ1c2VyLWRpZmZlcmVudC00NTYifX0.fakesig";
    let auth_json = format!(r#"{{"tokens":{{"id_token":"{id_token}"}}}}"#);
    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(auth_json.as_bytes()).unwrap();

    let accounts = vec![ManagedCodexAccountConfig {
        id: "acct-1".to_string(),
        label: "Test".to_string(),
        codex_home: std::path::PathBuf::from("/tmp"),
        email: None,
        provider_account_id: Some("user-abc-123".to_string()),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: None,
    }];

    let result = system_active_account_id(&accounts, tmp.path());
    assert!(result.is_none());
}

#[test]
fn system_active_account_id_returns_none_on_missing_file() {
    use crate::config::ManagedCodexAccountConfig;
    use chrono::Utc;

    let accounts = vec![ManagedCodexAccountConfig {
        id: "acct-1".to_string(),
        label: "Test".to_string(),
        codex_home: std::path::PathBuf::from("/tmp"),
        email: None,
        provider_account_id: Some("prov-abc-123".to_string()),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: None,
    }];

    let result =
        system_active_account_id(&accounts, std::path::Path::new("/nonexistent/auth.json"));
    assert!(result.is_none());
}
