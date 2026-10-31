// SPDX-License-Identifier: MPL-2.0

use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn server(responses: Vec<(u16, String)>) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for (status, body) in responses {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            loop {
                let mut chunk = [0; 1024];
                let size = stream.read(&mut chunk).await.unwrap();
                request.extend_from_slice(&chunk[..size]);
                if size == 0 || request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    break;
                }
            }
            requests.push(String::from_utf8(request).unwrap());
            stream.write_all(format!("HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nContent-Type: application/json\r\nRetry-After: 42\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        }
        requests
    });
    (endpoint, task)
}

fn client() -> Client {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

#[tokio::test]
async fn fetch_authenticates_and_scopes_organization_requests() {
    let (base, task) = server(vec![
        (200, json!({"user":{"id":"fixture-user","userName":"fixture"},"org":{"id":"fixture-org","login":"fixture-team"}}).to_string()),
        (200, credits().to_string()),
        (200, json!({"data":{"planId":"individual-pro-v1","status":"active"}}).to_string()),
    ]).await;
    let snapshot = fetch_at(&client(), "fixture-key", &base).await.unwrap();
    assert_eq!(
        snapshot.identity.display_name.as_deref(),
        Some("fixture-team")
    );
    assert_eq!(snapshot.identity.plan.as_deref(), Some("Pro"));
    let requests = task.await.unwrap();
    assert!(
        requests
            .iter()
            .all(|request| request.contains("authorization: Bearer fixture-key\r\n"))
    );
    assert!(requests[1].starts_with("GET /alpha/billing/credits?orgId=fixture-org "));
    assert!(requests[2].starts_with("GET /alpha/billing/subscriptions?orgId=fixture-org "));
}

#[tokio::test]
async fn maps_authentication_permission_and_rate_limit_failures() {
    for status in [401, 403, 429, 503, 302] {
        let (base, task) = server(vec![(status, "{}".to_string())]).await;
        let error = fetch_at(&client(), "fixture-key", &base).await.unwrap_err();
        match status {
            401 => assert!(error.requires_user_action()),
            403 => {
                assert!(!error.requires_user_action());
                assert!(!error.is_transient());
            }
            429 => {
                assert_eq!(error.rate_limit_retry_after_secs(), Some(42));
                assert!(error.is_transient());
            }
            503 => assert!(error.is_transient()),
            302 => assert!(matches!(error, CommandCodeError::Http { status: 302 })),
            _ => unreachable!(),
        }
        task.await.unwrap();
    }
}

#[tokio::test]
async fn optional_subscription_failure_keeps_balance_and_quotas() {
    let (base, task) = server(vec![
        (200, json!({"user":{"id":"fixture-user"}}).to_string()),
        (200, credits().to_string()),
        (404, "{}".to_string()),
    ])
    .await;
    let snapshot = fetch_at(&client(), "fixture-key", &base).await.unwrap();
    assert_eq!(snapshot.windows.len(), 2);
    assert!(snapshot.identity.plan.is_none());
    task.await.unwrap();
}

#[tokio::test]
async fn invalid_json_is_reported_without_returning_raw_body() {
    let (base, task) = server(vec![(200, "sensitive-fixture-data".to_string())]).await;
    let error = fetch_at(&client(), "fixture-key", &base).await.unwrap_err();
    assert!(!format!("{error:?}").contains("sensitive-fixture-data"));
    task.await.unwrap();
}

#[tokio::test]
#[ignore = "Run separately with YAPCAP_DEBUG_OFFLINE=1 to avoid caching the offline client in other tests"]
async fn debug_offline_blocks_provider_authentication() {
    assert_eq!(std::env::var("YAPCAP_DEBUG_OFFLINE").as_deref(), Ok("1"));
    let error = fetch_key("fixture-offline-key").await.unwrap_err();
    assert!(matches!(error, CommandCodeError::Request(error) if error.is_connect()));
}

#[tokio::test]
#[ignore = "Requires YAPCAP_COMMAND_CODE_SMOKE_KEY; makes read-only requests to Command Code"]
async fn live_read_only_usage_smoke_test() {
    let key = std::env::var("YAPCAP_COMMAND_CODE_SMOKE_KEY")
        .expect("provide a temporary test credential");
    let snapshot = fetch_key(&key)
        .await
        .expect("Command Code usage fetch failed");
    assert_eq!(snapshot.provider, ProviderId::CommandCode);
    assert!(snapshot.provider_cost.is_some() || !snapshot.windows.is_empty());
    if let Some(balance) = snapshot.provider_cost {
        assert!(balance.used.is_finite() && balance.used >= 0.0);
    }
    assert!(
        snapshot
            .windows
            .iter()
            .all(|window| (0.0..=100.0).contains(&window.used_percent))
    );
}
