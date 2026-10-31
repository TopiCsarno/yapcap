// SPDX-License-Identifier: MPL-2.0

use super::*;

fn form(origin: Option<&str>, body: &str) -> String {
    let origin = origin
        .map(|value| format!("Origin: {value}\r\n"))
        .unwrap_or_default();
    format!(
        "POST /callback HTTP/1.1\r\nHost: 127.0.0.1\r\n{origin}Content-Type: application/x-www-form-urlencoded; charset=UTF-8\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

#[tokio::test]
async fn accepts_nonce_authenticated_forms_with_opaque_or_missing_origins() {
    for origin in [Some("null"), None, Some(ORIGIN)] {
        let (response, result) = exchange_action(
            form(
                origin,
                "state=fixture-state&apiKey=fixture%2Bkey%3D&userId=fixture",
            ),
            true,
            false,
        )
        .await;
        assert!(response.starts_with("HTTP/1.1 303 "));
        assert!(response.contains("Location: /callback/complete?state=fixture-state\r\n"));
        assert!(!response.contains("fixture+key"));
        assert!(matches!(result.unwrap(), CallbackAction::Redirect(key) if key == "fixture+key="));
    }
}

#[tokio::test]
async fn rejects_foreign_form_origins_and_invalid_nonce_fields() {
    for (origin, body, status) in [
        (
            Some("https://attacker.example"),
            "state=fixture-state&apiKey=fixture",
            403,
        ),
        (Some("null"), "state=wrong&apiKey=fixture", 403),
        (None, "state=wrong&error=access_denied", 403),
        (None, "apiKey=fixture", 400),
        (None, "state=fixture-state&state=wrong&apiKey=fixture", 400),
        (None, "state=fixture-state&apiKey=first&apiKey=second", 400),
        (None, "state=fixture-state&apiKey=bad%0Akey", 400),
        (None, "state=fixture-\nstate&apiKey=fixture", 400),
        (None, "sta\tte=fixture-state&apiKey=fixture", 400),
    ] {
        let (response, result) = exchange_action(form(origin, body), false, false).await;
        assert!(response.starts_with(&format!("HTTP/1.1 {status} ")));
        assert!(matches!(result.unwrap(), CallbackAction::Ignored));
    }
}

#[tokio::test]
async fn form_denial_requires_the_nonce_and_does_not_echo_errors() {
    let (response, result) = exchange_action(
        form(
            Some("null"),
            "state=fixture-state&error=private-provider-error",
        ),
        false,
        false,
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 200 "));
    assert!(response.contains("text/html"));
    assert!(!response.contains("private-provider-error"));
    assert!(result.err().unwrap().contains("denied"));
}

#[tokio::test]
async fn completion_requires_both_pending_credentials_and_matching_state() {
    for (query, pending, status) in [
        ("state=fixture-state", false, 403),
        ("state=wrong", true, 403),
        ("state=fixture-state&state=wrong", true, 403),
        ("state=fixture-state&apiKey=fixture", true, 403),
        ("state=fixture-state", true, 200),
    ] {
        let request = format!("GET /callback/complete?{query} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        let (response, result) = exchange_action(request, false, pending).await;
        assert!(response.starts_with(&format!("HTTP/1.1 {status} ")));
        if status == 200 {
            assert!(matches!(result.unwrap(), CallbackAction::Complete));
            assert!(response.contains("text/html"));
            assert!(response.contains("Referrer-Policy: no-referrer"));
        } else {
            assert!(matches!(result.unwrap(), CallbackAction::Ignored));
        }
    }
}

#[tokio::test]
async fn a_second_callback_cannot_replace_pending_credentials() {
    let (response, result) = exchange_action(
        form(None, "state=fixture-state&apiKey=replacement"),
        false,
        true,
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 409 "));
    assert!(matches!(result.unwrap(), CallbackAction::Ignored));
}

#[tokio::test]
async fn opaque_origins_remain_forbidden_for_legacy_json() {
    let (response, result) = exchange(
        post("null", r#"{"state":"fixture-state","apiKey":"fixture"}"#),
        false,
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 403 "));
    assert!(result.unwrap().is_none());
}
