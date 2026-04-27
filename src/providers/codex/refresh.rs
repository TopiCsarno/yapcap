// SPDX-License-Identifier: MPL-2.0

use crate::error::{CodexError, Result};
use serde::Deserialize;

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const TOKEN_ENDPOINT: &str = "https://auth.openai.com/oauth/token";

#[derive(Debug, Clone)]
pub struct RefreshedCodexTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RefreshResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
}

pub async fn refresh_access_token(
    client: &reqwest::Client,
    refresh_token: &str,
) -> Result<RefreshedCodexTokens, CodexError> {
    refresh_access_token_at(client, TOKEN_ENDPOINT, refresh_token).await
}

async fn refresh_access_token_at(
    client: &reqwest::Client,
    endpoint: &str,
    refresh_token: &str,
) -> Result<RefreshedCodexTokens, CodexError> {
    let body = [
        ("grant_type", "refresh_token"),
        ("client_id", CLIENT_ID),
        ("refresh_token", refresh_token),
    ];
    let response = client
        .post(endpoint)
        .form(&body)
        .send()
        .await
        .map_err(CodexError::RefreshRequest)?;

    let status = response.status();
    if !status.is_success() {
        let snippet = response
            .text()
            .await
            .ok()
            .and_then(|body| {
                let trimmed = body.trim();
                (!trimmed.is_empty()).then(|| trimmed.chars().take(512).collect::<String>())
            })
            .map(|body| format!(" (body: {body})"))
            .unwrap_or_default();

        return Err(CodexError::RefreshHttp {
            status: status.as_u16(),
            details: snippet,
        });
    }

    let decoded: RefreshResponse = response.json().await.map_err(CodexError::RefreshDecode)?;
    Ok(RefreshedCodexTokens {
        access_token: decoded.access_token,
        refresh_token: decoded.refresh_token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[tokio::test]
    async fn refresh_decodes_access_token() {
        let listener = match TcpListener::bind(("127.0.0.1", 0)) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("{error}"),
        };
        let addr = listener.local_addr().unwrap();

        let server = tokio::task::spawn_blocking(move || {
            let (mut stream, _peer) = listener.accept().unwrap();

            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]);

            assert!(req.starts_with("POST /oauth/token HTTP/1.1\r\n"));
            assert!(req.contains("content-type: application/x-www-form-urlencoded"));
            assert!(req.contains("grant_type=refresh_token"));
            assert!(req.contains("client_id=app_EMoamEEZ73f0CkXaXp7hrann"));
            assert!(req.contains("refresh_token=test-refresh-token"));

            let body = r#"{"access_token":"new-access","refresh_token":"new-refresh"}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).unwrap();
        });

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap();

        let endpoint = format!("http://{addr}/oauth/token");
        let tokens = refresh_access_token_at(&client, &endpoint, "test-refresh-token")
            .await
            .unwrap();

        assert_eq!(tokens.access_token, "new-access");
        assert_eq!(tokens.refresh_token.as_deref(), Some("new-refresh"));

        server.await.unwrap();
    }
}
