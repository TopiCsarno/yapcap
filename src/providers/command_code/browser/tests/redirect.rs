// SPDX-License-Identifier: MPL-2.0

use super::*;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

async fn browser() -> BrowserLogin {
    BrowserLogin {
        listener: TcpListener::bind(("127.0.0.1", 0)).await.unwrap(),
        state: "fixture-state".to_string(),
        url: String::new(),
    }
}

async fn exchange(address: SocketAddr, request: String) -> String {
    let mut stream = TcpStream::connect(address).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
}

fn form(body: &str) -> String {
    format!(
        "POST /callback HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: null\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

fn complete(state: &str) -> String {
    format!("GET /callback/complete?state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
}

#[tokio::test]
async fn form_redirect_completion_returns_the_original_key_and_closes_the_listener() {
    let browser = browser().await;
    let address = browser.listener.local_addr().unwrap();
    let task = tokio::spawn(browser.wait());
    let response = exchange(address, form("state=fixture-state&apiKey=fixture-key")).await;
    assert!(response.starts_with("HTTP/1.1 303 "));
    assert!(!task.is_finished());
    let replay = exchange(address, form("state=fixture-state&apiKey=replacement")).await;
    assert!(replay.starts_with("HTTP/1.1 409 "));
    let wrong = exchange(address, complete("wrong")).await;
    assert!(wrong.starts_with("HTTP/1.1 403 "));
    let response = exchange(address, complete("fixture-state")).await;
    assert!(response.starts_with("HTTP/1.1 200 "));
    assert!(!response.contains("fixture-key"));
    let key = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(key, "fixture-key");
    let _listener = TcpListener::bind(address).await.unwrap();
}

#[tokio::test]
async fn an_unfollowed_redirect_completes_after_a_bounded_grace_period() {
    let browser = browser().await;
    let address = browser.listener.local_addr().unwrap();
    let task = tokio::spawn(browser.receive(Duration::from_millis(30)));
    let response = exchange(address, form("state=fixture-state&apiKey=fixture-key")).await;
    assert!(response.starts_with("HTTP/1.1 303 "));
    let key = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(key, "fixture-key");
}

#[tokio::test]
async fn cancellation_after_a_form_callback_drops_the_pending_login() {
    let browser = browser().await;
    let address = browser.listener.local_addr().unwrap();
    let task = tokio::spawn(browser.wait());
    let response = exchange(address, form("state=fixture-state&apiKey=fixture-key")).await;
    assert!(response.starts_with("HTTP/1.1 303 "));
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let _listener = TcpListener::bind(address).await.unwrap();
}

#[tokio::test]
async fn a_nonce_authenticated_form_denial_finishes_without_a_redirect() {
    let browser = browser().await;
    let address = browser.listener.local_addr().unwrap();
    let task = tokio::spawn(browser.wait());
    let response = exchange(address, form("state=fixture-state&error=access_denied")).await;
    assert!(response.starts_with("HTTP/1.1 200 "));
    let result = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    assert!(result.unwrap_err().contains("denied"));
}
