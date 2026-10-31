// SPDX-License-Identifier: MPL-2.0

use super::ORIGIN;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

pub(super) async fn json(stream: &mut TcpStream, status: u16, success: bool) {
    let body = if status == 204 {
        ""
    } else if success {
        r#"{"success":true}"#
    } else {
        r#"{"success":false}"#
    };
    send(stream, status, "application/json", "", body).await;
}

pub(super) async fn redirect(stream: &mut TcpStream, state: &str) {
    let headers = format!("Location: /callback/complete?state={state}\r\n");
    send(stream, 303, "text/html; charset=utf-8", &headers, "").await;
}

pub(super) async fn complete(stream: &mut TcpStream, success: bool) {
    let heading = if success {
        "Command Code sign-in received"
    } else {
        "Command Code sign-in was denied or failed"
    };
    let body = format!(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>Command Code sign-in</title><h1>{heading}</h1><p>Return to YapCap. You can close this tab.</p></html>"
    );
    send(stream, 200, "text/html; charset=utf-8", "", &body).await;
}

async fn send(stream: &mut TcpStream, status: u16, content_type: &str, headers: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status} Response\r\nContent-Type: {content_type}\r\nAccess-Control-Allow-Origin: {ORIGIN}\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nAccess-Control-Allow-Private-Network: true\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; frame-ancestors 'none'; base-uri 'none'\r\nX-Content-Type-Options: nosniff\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}
