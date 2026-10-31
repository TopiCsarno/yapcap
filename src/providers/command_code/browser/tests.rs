// SPDX-License-Identifier: MPL-2.0

use super::*;

mod redirect;

#[tokio::test]
async fn browser_login_uses_a_loopback_callback_and_unpredictable_state() {
    let first = BrowserLogin::start().await.unwrap();
    let second = BrowserLogin::start().await.unwrap();
    let url = reqwest::Url::parse(&first.url).unwrap();
    assert_eq!(url.origin().ascii_serialization(), "https://commandcode.ai");
    assert_eq!(url.path(), "/studio/auth/cli");
    assert!(
        url.query_pairs()
            .any(|(key, value)| key == "mode" && value == "redirect")
    );
    let callback = url
        .query_pairs()
        .find(|(key, _)| key == "callback")
        .unwrap()
        .1
        .into_owned();
    assert!(callback.starts_with("http://127.0.0.1:"));
    assert!(callback.ends_with("/callback"));
    assert_eq!(first.state.len(), 64);
    assert_ne!(first.state, second.state);
    assert!(!url.query_pairs().any(|(key, _)| key == "apiKey"));
}

#[tokio::test]
async fn canceling_browser_wait_closes_the_callback_listener() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let browser = BrowserLogin {
        listener,
        state: "fixture-state".to_string(),
        url: String::new(),
    };
    let task = tokio::spawn(browser.wait());
    tokio::task::yield_now().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let _listener = TcpListener::bind(address).await.unwrap();
}
