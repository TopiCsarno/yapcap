// SPDX-License-Identifier: MPL-2.0

use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

mod form;
mod request;

async fn exchange(request: String, fragment: bool) -> (String, Result<Option<String>, String>) {
    let (response, result) = exchange_action(request, fragment, false).await;
    let result = result.map(|action| match action {
        CallbackAction::Credential(key) => Some(key),
        _ => None,
    });
    (response, result)
}

async fn exchange_action(
    request: String,
    fragment: bool,
    can_complete: bool,
) -> (String, Result<CallbackAction, String>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        handle(&mut stream, "fixture-state", can_complete).await
    });
    let mut stream = TcpStream::connect(address).await.unwrap();
    if fragment {
        for chunk in request.as_bytes().chunks(7) {
            stream.write_all(chunk).await.unwrap();
            tokio::task::yield_now().await;
        }
    } else {
        stream.write_all(request.as_bytes()).await.unwrap();
    }
    stream.shutdown().await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    (response, task.await.unwrap())
}

fn post(origin: &str, body: &str) -> String {
    format!(
        "POST /callback HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: {origin}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

#[tokio::test]
async fn accepts_fragmented_browser_callback_without_echoing_credentials() {
    let (response, result) = exchange(
        post(
            ORIGIN,
            r#"{"state":"fixture-state","apiKey":"fixture-private-key"}"#,
        ),
        true,
    )
    .await;
    assert_eq!(result.unwrap().as_deref(), Some("fixture-private-key"));
    assert!(response.starts_with("HTTP/1.1 200 "));
    assert!(response.contains("Cache-Control: no-store"));
    assert!(!response.contains("fixture-private-key"));
}

#[tokio::test]
async fn rejects_foreign_browser_origins() {
    let (response, result) = exchange(
        post(
            "https://attacker.example",
            r#"{"state":"fixture-state","apiKey":"fixture-key"}"#,
        ),
        false,
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 403 "));
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn invalid_state_cannot_finish_or_cancel_the_login() {
    for body in [
        r#"{"state":"wrong-state","apiKey":"fixture-key"}"#,
        r#"{"state":"wrong-state","error":"access_denied"}"#,
    ] {
        let (response, result) = exchange(post(ORIGIN, body), false).await;
        assert!(response.starts_with("HTTP/1.1 403 "));
        assert!(result.unwrap().is_none());
    }
}

#[tokio::test]
async fn validates_state_before_accepting_authorization_denial() {
    let (_, result) = exchange(
        post(
            ORIGIN,
            r#"{"state":"fixture-state","error":"access_denied"}"#,
        ),
        false,
    )
    .await;
    assert!(result.unwrap_err().contains("denied"));
}

#[tokio::test]
async fn handles_private_network_preflight() {
    let (response, result) = exchange(format!("OPTIONS /callback HTTP/1.1\r\nOrigin: {ORIGIN}\r\nAccess-Control-Request-Private-Network: true\r\n\r\n"), false).await;
    assert!(response.starts_with("HTTP/1.1 204 "));
    assert!(response.contains("Access-Control-Allow-Private-Network: true"));
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn rejects_oversized_and_ambiguous_requests() {
    for headers in [
        "Content-Length: 999999",
        "Content-Length: 0\r\nContent-Length: 1",
        "Content-Length: 0\r\nTransfer-Encoding: chunked",
    ] {
        let (response, result) = exchange(
            format!("POST /callback HTTP/1.1\r\nOrigin: {ORIGIN}\r\n{headers}\r\n\r\n"),
            false,
        )
        .await;
        assert!(response.starts_with("HTTP/1.1 400 "));
        assert!(result.unwrap().is_none());
    }
}

#[tokio::test]
async fn rejects_invalid_keys_and_ignores_unrelated_paths() {
    let (response, result) = exchange(
        post(ORIGIN, r#"{"state":"fixture-state","apiKey":"bad\nkey"}"#),
        false,
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 400 "));
    assert!(result.unwrap().is_none());
    let (response, result) =
        exchange(post(ORIGIN, "{}").replace("/callback", "/other"), false).await;
    assert!(response.starts_with("HTTP/1.1 404 "));
    assert!(result.unwrap().is_none());
}
