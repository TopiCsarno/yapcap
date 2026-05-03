// SPDX-License-Identifier: MPL-2.0

use crate::auth::{CodexAuth, account_id_from_id_token, email_from_id_token, jwt_expiration};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::io::Read as _;

pub(super) const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub(super) const ISSUER: &str = "https://auth.openai.com";
pub(super) const TOKEN_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
pub(super) const DEFAULT_CALLBACK_PORT: u16 = 1455;
pub(super) const FALLBACK_CALLBACK_PORT: u16 = 1457;

const SCOPE: &str = "openid profile email offline_access api.connectors.read api.connectors.invoke";
const ORIGINATOR: &str = "codex_cli_rs";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PkceCodes {
    pub code_verifier: String,
    pub code_challenge: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CodexOAuthTokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub email: Option<String>,
    pub account_id: Option<String>,
}

impl CodexOAuthTokenResponse {
    pub(super) fn into_auth(self) -> CodexAuth {
        CodexAuth {
            access_token: self.access_token,
            account_id: self.account_id,
            refresh_token: Some(self.refresh_token),
            id_token: Some(self.id_token),
            expires_at: self.expires_at,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawTokenResponse {
    #[serde(rename = "access_token")]
    access: String,
    #[serde(rename = "refresh_token")]
    refresh: String,
    #[serde(rename = "id_token")]
    id: String,
}

pub(super) fn parse_token_response(raw: &str) -> Result<CodexOAuthTokenResponse, String> {
    let parsed: RawTokenResponse = serde_json::from_str(raw)
        .map_err(|error| format!("failed to decode Codex OAuth token response: {error}"))?;
    Ok(CodexOAuthTokenResponse {
        expires_at: jwt_expiration(&parsed.access)
            .or_else(|| jwt_expiration(&parsed.id))
            .or_else(|| Some(Utc::now())),
        email: email_from_id_token(&parsed.id),
        account_id: account_id_from_id_token(&parsed.id),
        access_token: parsed.access,
        refresh_token: parsed.refresh,
        id_token: parsed.id,
    })
}

pub(super) fn authorization_url(
    issuer: &str,
    redirect_uri: &str,
    pkce: &PkceCodes,
    state: &str,
) -> String {
    let params = [
        ("response_type", "code"),
        ("client_id", CLIENT_ID),
        ("redirect_uri", redirect_uri),
        ("scope", SCOPE),
        ("code_challenge", pkce.code_challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("state", state),
        ("originator", ORIGINATOR),
    ];
    let query = params
        .into_iter()
        .map(|(key, value)| format!("{}={}", percent_encode(key), percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{}/oauth/authorize?{query}", issuer.trim_end_matches('/'))
}

pub(super) async fn exchange_code(
    client: &reqwest::Client,
    token_endpoint: &str,
    redirect_uri: &str,
    code_verifier: &str,
    code: &str,
) -> Result<CodexOAuthTokenResponse, String> {
    let response = client
        .post(token_endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", CLIENT_ID),
            ("code_verifier", code_verifier),
        ])
        .send()
        .await
        .map_err(|error| format!("Codex OAuth token exchange failed: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("failed to read Codex OAuth token response: {error}"))?;
    if !status.is_success() {
        let snippet = body.trim().chars().take(256).collect::<String>();
        return Err(format!(
            "Codex OAuth token exchange returned {status} (body: {snippet})"
        ));
    }
    parse_token_response(&body)
}

pub(super) fn new_pkce() -> PkceCodes {
    let bytes = random_bytes();
    let code_verifier = URL_SAFE_NO_PAD.encode(bytes);
    let code_challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()));
    PkceCodes {
        code_verifier,
        code_challenge,
    }
}

pub(super) fn new_state() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes())
}

fn random_bytes() -> [u8; 64] {
    let mut bytes = [0; 64];
    if let Ok(mut file) = std::fs::File::open("/dev/urandom")
        && file.read_exact(&mut bytes).is_ok()
    {
        return bytes;
    }
    let fallback = format!(
        "{}:{}:{}",
        Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        std::process::id(),
        std::thread::current().name().unwrap_or("thread")
    );
    let digest = Sha256::digest(fallback.as_bytes());
    bytes[..32].copy_from_slice(&digest);
    let second_digest = Sha256::digest(&bytes[..32]);
    bytes[32..].copy_from_slice(&second_digest);
    bytes
}

pub(super) fn percent_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            write!(out, "%{byte:02X}").expect("writing to a string cannot fail");
        }
    }
    out
}

pub(super) fn percent_decode(value: &str) -> String {
    let mut out = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Ok(hex) = u8::from_str_radix(&value[index + 1..index + 3], 16)
        {
            out.push(hex);
            index += 3;
        } else if bytes[index] == b'+' {
            out.push(b' ');
            index += 1;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn jwt(payload: &str) -> String {
        format!("header.{}.sig", URL_SAFE_NO_PAD.encode(payload.as_bytes()))
    }

    #[test]
    fn authorization_url_uses_codex_pkce_parameters() {
        let url = authorization_url(
            ISSUER,
            "http://localhost:1455/auth/callback",
            &PkceCodes {
                code_verifier: "verifier".to_string(),
                code_challenge: "challenge".to_string(),
            },
            "state",
        );

        assert!(url.starts_with("https://auth.openai.com/oauth/authorize?"));
        assert!(url.contains("client_id=app_EMoamEEZ73f0CkXaXp7hrann"));
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback"));
        assert!(url.contains("scope=openid%20profile%20email%20offline_access"));
        assert!(url.contains("code_challenge=challenge"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("id_token_add_organizations=true"));
        assert!(url.contains("codex_cli_simplified_flow=true"));
        assert!(url.contains("originator=codex_cli_rs"));
        assert!(!url.contains("verifier"));
    }

    #[test]
    fn parses_token_response_with_identity() {
        let id_token = jwt(r#"{
                "email": "User@Example.com",
                "exp": 1770000000,
                "https://api.openai.com/auth": {
                    "chatgpt_account_id": "acct_123"
                }
            }"#);
        let access_token = jwt(r#"{"exp": 1770000300}"#);
        let raw = format!(
            r#"{{
                "access_token": "{access_token}",
                "refresh_token": "refresh",
                "id_token": "{id_token}"
            }}"#
        );

        let parsed = parse_token_response(&raw).unwrap();

        assert_eq!(parsed.access_token, access_token);
        assert_eq!(parsed.refresh_token, "refresh");
        assert_eq!(parsed.id_token, id_token);
        assert_eq!(parsed.email.as_deref(), Some("User@Example.com"));
        assert_eq!(parsed.account_id.as_deref(), Some("acct_123"));
        assert_eq!(
            parsed.expires_at,
            Some(Utc.with_ymd_and_hms(2026, 2, 2, 2, 45, 0).unwrap())
        );
    }

    #[test]
    fn malformed_token_response_fails() {
        assert!(parse_token_response(r#"{"access_token":"access"}"#).is_err());
    }
}
