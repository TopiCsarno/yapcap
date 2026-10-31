// SPDX-License-Identifier: MPL-2.0

use super::*;

#[tokio::test]
async fn duplicate_origins_cannot_masquerade_as_an_absent_origin() {
    let body = "state=fixture-state&apiKey=fixture";
    let request = format!(
        "POST /callback HTTP/1.1\r\nOrigin: {ORIGIN}\r\norigin: null\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let (response, result) = exchange_action(request, false, false).await;
    assert!(response.starts_with("HTTP/1.1 400 "));
    assert!(matches!(result.unwrap(), CallbackAction::Ignored));
}

#[tokio::test]
async fn rejects_duplicate_json_fields_and_unsupported_media_types() {
    for body in [
        r#"{"state":"fixture-state","state":"wrong","apiKey":"fixture"}"#,
        r#"{"state":"fixture-state","apiKey":"first","apiKey":"second"}"#,
    ] {
        let (response, result) = exchange(post(ORIGIN, body), false).await;
        assert!(response.starts_with("HTTP/1.1 400 "));
        assert!(result.unwrap().is_none());
    }
    let request = post(ORIGIN, "{}").replace("application/json", "text/plain");
    let (response, result) = exchange(request, false).await;
    assert!(response.starts_with("HTTP/1.1 415 "));
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn rejects_malformed_headers_lengths_and_request_lines() {
    for request in [
        "POST /callback HTTP/1.1\r\nContent-Length: +0\r\n\r\n",
        "POST /callback HTTP/1.1\r\nContent-Length : 0\r\n\r\n",
        "POST /callback HTTP/1.1\r\nContent-Length: 0\r\n folded: value\r\n\r\n",
        "POST /callback HTTP/1.1 extra\r\nContent-Length: 0\r\n\r\n",
        "POST //other/callback HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
    ] {
        let (response, result) = exchange(request.to_string(), false).await;
        assert!(response.starts_with("HTTP/1.1 400 "));
        assert!(result.unwrap().is_none());
    }
}
