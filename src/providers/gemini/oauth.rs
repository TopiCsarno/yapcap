// SPDX-License-Identifier: MPL-2.0

use crate::error::GeminiError;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::io::Read as _;

pub const OAUTH_CLIENT_ID: &str =
    "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";
pub const OAUTH_CLIENT_SECRET: &str = "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";

pub const AUTHORIZE_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

pub const SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform openid https://www.googleapis.com/auth/userinfo.profile https://www.googleapis.com/auth/userinfo.email";

pub const REFRESH_BEFORE_EXPIRY: Duration = Duration::minutes(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkceCodes {
    pub code_verifier: String,
    pub code_challenge: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeminiOAuthTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: String,
    pub expires_at: DateTime<Utc>,
    pub scope: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    scope: Option<String>,
}

pub fn parse_token_response(raw: &str) -> Result<GeminiOAuthTokens, String> {
    let parsed: RawTokenResponse = serde_json::from_str(raw)
        .map_err(|error| format!("failed to decode Gemini OAuth token response: {error}"))?;
    let access_token = parsed.access_token;
    let refresh_token = parsed
        .refresh_token
        .ok_or_else(|| "Gemini OAuth response missing refresh_token".to_string())?;
    let id_token = parsed
        .id_token
        .ok_or_else(|| "Gemini OAuth response missing id_token".to_string())?;
    let now = Utc::now();
    let expires_at = parsed
        .expires_in
        .map(|seconds| now + Duration::seconds(seconds))
        .unwrap_or(now + Duration::hours(1));
    let scope = parsed
        .scope
        .as_deref()
        .map(|scope| {
            scope
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(GeminiOAuthTokens {
        access_token,
        refresh_token,
        id_token,
        expires_at,
        scope,
    })
}

pub fn authorization_url_with_hint(
    redirect_uri: &str,
    pkce: &PkceCodes,
    state: &str,
    login_hint: Option<&str>,
) -> String {
    let mut params: Vec<(&str, &str)> = vec![
        ("response_type", "code"),
        ("client_id", OAUTH_CLIENT_ID),
        ("redirect_uri", redirect_uri),
        ("scope", SCOPE),
        ("code_challenge", pkce.code_challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("state", state),
        ("access_type", "offline"),
        ("prompt", "consent"),
    ];
    if let Some(hint) = login_hint.filter(|hint| !hint.is_empty()) {
        params.push(("login_hint", hint));
    }
    let query = params
        .into_iter()
        .map(|(key, value)| format!("{}={}", percent_encode(key), percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{AUTHORIZE_ENDPOINT}?{query}")
}

pub async fn exchange_code(
    client: &reqwest::Client,
    token_endpoint: &str,
    redirect_uri: &str,
    code_verifier: &str,
    code: &str,
) -> Result<GeminiOAuthTokens, String> {
    let response = client
        .post(token_endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", code_verifier),
            ("client_id", OAUTH_CLIENT_ID),
            ("client_secret", OAUTH_CLIENT_SECRET),
            ("redirect_uri", redirect_uri),
        ])
        .send()
        .await
        .map_err(|error| format!("Gemini OAuth token exchange failed: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("failed to read Gemini OAuth token response: {error}"))?;
    if !status.is_success() {
        let snippet = body.trim().chars().take(256).collect::<String>();
        return Err(format!(
            "Gemini OAuth token exchange returned {status} (body: {snippet})"
        ));
    }
    parse_token_response(&body)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeminiRefreshedTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
    pub scope: Vec<String>,
}

pub fn needs_refresh(expires_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    expires_at <= now + REFRESH_BEFORE_EXPIRY
}

pub async fn refresh_access_token_at(
    client: &reqwest::Client,
    endpoint: &str,
    refresh_token: &str,
    now: DateTime<Utc>,
) -> Result<GeminiRefreshedTokens, GeminiError> {
    let response = client
        .post(endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", OAUTH_CLIENT_ID),
            ("client_secret", OAUTH_CLIENT_SECRET),
        ])
        .send()
        .await
        .map_err(GeminiError::TokenRefreshRequest)?;
    let status = response.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let retry_after_secs = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        return Err(GeminiError::RateLimited { retry_after_secs });
    }
    if !status.is_success() {
        return Err(GeminiError::TokenRefreshHttp {
            status: status.as_u16(),
        });
    }
    let body = response
        .text()
        .await
        .map_err(GeminiError::TokenRefreshDecode)?;
    parse_refresh_response(&body, refresh_token, now)
}

fn parse_refresh_response(
    raw: &str,
    original_refresh_token: &str,
    now: DateTime<Utc>,
) -> Result<GeminiRefreshedTokens, GeminiError> {
    #[derive(Debug, Deserialize)]
    struct Raw {
        access_token: Option<String>,
        #[serde(default)]
        refresh_token: Option<String>,
        #[serde(default)]
        expires_in: Option<i64>,
        #[serde(default)]
        scope: Option<String>,
    }
    let parsed: Raw = serde_json::from_str(raw)
        .map_err(|error| GeminiError::TokenRefreshParse(error.to_string()))?;
    let access_token = parsed
        .access_token
        .filter(|value| !value.is_empty())
        .ok_or_else(|| GeminiError::TokenRefreshParse("missing access_token".to_string()))?;
    let expires_in = parsed.expires_in.unwrap_or(3600);
    if expires_in <= 0 {
        return Err(GeminiError::TokenRefreshParse(
            "invalid expires_in".to_string(),
        ));
    }
    let refresh_token = parsed
        .refresh_token
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| original_refresh_token.to_string());
    let scope = parsed
        .scope
        .as_deref()
        .map(|scope| {
            scope
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(GeminiRefreshedTokens {
        access_token,
        refresh_token,
        expires_at: now + Duration::seconds(expires_in),
        scope,
    })
}

pub fn new_pkce() -> PkceCodes {
    let bytes = random_bytes();
    let code_verifier = URL_SAFE_NO_PAD.encode(bytes);
    let code_challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()));
    PkceCodes {
        code_verifier,
        code_challenge,
    }
}

pub fn new_state() -> String {
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
    let second = Sha256::digest(&bytes[..32]);
    bytes[32..].copy_from_slice(&second);
    bytes
}

pub fn percent_encode(value: &str) -> String {
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

pub fn percent_decode(value: &str) -> String {
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    struct MockResponse {
        status: u16,
        body: String,
        extra_headers: Vec<(&'static str, String)>,
    }

    async fn mock_token_server(
        responses: Vec<MockResponse>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buffer = vec![0; 8192];
                let bytes = stream.read(&mut buffer).await.unwrap();
                let request = String::from_utf8_lossy(&buffer[..bytes]).to_string();
                let mut header_lines = String::new();
                for (key, value) in &response.extra_headers {
                    header_lines.push_str(&format!("{key}: {value}\r\n"));
                }
                let raw = format!(
                    "HTTP/1.1 {} OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n{header_lines}connection: close\r\n\r\n{}",
                    response.status,
                    response.body.len(),
                    response.body,
                );
                stream.write_all(raw.as_bytes()).await.unwrap();
                requests.push(request);
            }
            requests
        });
        (format!("http://{addr}/token"), handle)
    }

    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 14, 12, 0, 0).unwrap()
    }

    #[test]
    fn authorization_url_includes_pkce_and_scopes() {
        let url = authorization_url_with_hint(
            "http://localhost:12345/oauth/callback",
            &PkceCodes {
                code_verifier: "verifier".to_string(),
                code_challenge: "challenge".to_string(),
            },
            "nonce",
            None,
        );
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        assert!(url.contains("client_id=681255809395-"));
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A12345%2Foauth%2Fcallback"));
        assert!(url.contains("code_challenge=challenge"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=nonce"));
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("prompt=consent"));
        assert!(url.contains("scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fcloud-platform"));
        assert!(url.contains("openid"));
        assert!(url.contains("userinfo.email"));
        assert!(!url.contains("verifier"));
        assert!(!url.contains("login_hint"));
    }

    #[test]
    fn authorization_url_appends_login_hint_when_provided() {
        let url = authorization_url_with_hint(
            "http://localhost:1/oauth/callback",
            &PkceCodes {
                code_verifier: "v".to_string(),
                code_challenge: "c".to_string(),
            },
            "n",
            Some("user@example.com"),
        );
        assert!(url.contains("login_hint=user%40example.com"));
    }

    #[test]
    fn pkce_challenge_is_sha256_of_verifier() {
        let pkce = new_pkce();
        let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(pkce.code_verifier.as_bytes()));
        assert_eq!(pkce.code_challenge, expected);
        assert!(pkce.code_verifier.len() >= 43);
    }

    #[test]
    fn pkce_state_is_random() {
        let a = new_state();
        let b = new_state();
        assert_ne!(a, b);
    }

    #[test]
    fn parses_authorization_code_token_response() {
        let raw = r#"{
            "access_token": "ya29.access",
            "refresh_token": "1//refresh",
            "id_token": "header.payload.sig",
            "expires_in": 3599,
            "scope": "openid https://www.googleapis.com/auth/userinfo.email",
            "token_type": "Bearer"
        }"#;
        let parsed = parse_token_response(raw).expect("parsed");
        assert_eq!(parsed.access_token, "ya29.access");
        assert_eq!(parsed.refresh_token, "1//refresh");
        assert_eq!(parsed.id_token, "header.payload.sig");
        assert!(parsed.scope.iter().any(|s| s == "openid"));
        assert!(parsed.expires_at > Utc::now());
    }

    #[test]
    fn missing_refresh_token_is_an_error() {
        let raw = r#"{"access_token":"a","id_token":"h.p.s","expires_in":60}"#;
        assert!(parse_token_response(raw).is_err());
    }

    #[test]
    fn missing_id_token_is_an_error() {
        let raw = r#"{"access_token":"a","refresh_token":"r","expires_in":60}"#;
        assert!(parse_token_response(raw).is_err());
    }

    #[test]
    fn needs_refresh_when_expires_at_within_five_minutes() {
        let now = fixed_now();
        assert!(needs_refresh(now + Duration::minutes(2), now));
        assert!(needs_refresh(now + Duration::minutes(5), now));
        assert!(!needs_refresh(now + Duration::minutes(6), now));
    }

    #[test]
    fn refresh_parses_rotated_access_token_and_preserves_refresh_token() {
        let raw = r#"{
            "access_token": "ya29.new-access",
            "expires_in": 3599,
            "scope": "https://www.googleapis.com/auth/userinfo.email openid",
            "token_type": "Bearer"
        }"#;
        let now = fixed_now();
        let parsed = parse_refresh_response(raw, "original-refresh", now).unwrap();
        assert_eq!(parsed.access_token, "ya29.new-access");
        assert_eq!(parsed.refresh_token, "original-refresh");
        assert_eq!(parsed.expires_at, now + Duration::seconds(3599));
        assert!(parsed.scope.iter().any(|s| s == "openid"));
    }

    #[test]
    fn refresh_uses_rotated_refresh_token_when_provided() {
        let raw = r#"{
            "access_token": "ya29.new",
            "refresh_token": "rotated-refresh",
            "expires_in": 60
        }"#;
        let parsed = parse_refresh_response(raw, "original-refresh", fixed_now()).unwrap();
        assert_eq!(parsed.refresh_token, "rotated-refresh");
    }

    #[test]
    fn refresh_rejects_invalid_expires_in() {
        let raw = r#"{"access_token":"a","expires_in":0}"#;
        let error = parse_refresh_response(raw, "r", fixed_now()).unwrap_err();
        assert!(matches!(error, GeminiError::TokenRefreshParse(_)));
    }

    #[tokio::test]
    async fn refresh_success_preserves_refresh_token_across_http_call() {
        let (endpoint, handle) = mock_token_server(vec![MockResponse {
            status: 200,
            body: r#"{"access_token":"ya29.new","expires_in":3599,"scope":"openid"}"#.to_string(),
            extra_headers: Vec::new(),
        }])
        .await;
        let now = fixed_now();
        let tokens =
            refresh_access_token_at(&reqwest::Client::new(), &endpoint, "original-refresh", now)
                .await
                .unwrap();
        assert_eq!(tokens.access_token, "ya29.new");
        assert_eq!(tokens.refresh_token, "original-refresh");
        assert_eq!(tokens.expires_at, now + Duration::seconds(3599));
        let requests = handle.await.unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains("grant_type=refresh_token"));
        assert!(requests[0].contains("refresh_token=original-refresh"));
        assert!(requests[0].contains(&format!("client_id={OAUTH_CLIENT_ID}")));
        assert!(requests[0].contains("client_secret="));
    }

    #[tokio::test]
    async fn refresh_4xx_is_classified_permanent_action_required() {
        for status in [400_u16, 401, 403] {
            let (endpoint, handle) = mock_token_server(vec![MockResponse {
                status,
                body: r#"{"error":"invalid_grant"}"#.to_string(),
                extra_headers: Vec::new(),
            }])
            .await;
            let error =
                refresh_access_token_at(&reqwest::Client::new(), &endpoint, "refresh", fixed_now())
                    .await
                    .unwrap_err();
            assert!(matches!(error, GeminiError::TokenRefreshHttp { status: s } if s == status));
            assert!(error.requires_user_action());
            assert!(!error.is_transient());
            handle.await.unwrap();
        }
    }

    #[tokio::test]
    async fn refresh_429_with_retry_after_parses_seconds() {
        let (endpoint, handle) = mock_token_server(vec![MockResponse {
            status: 429,
            body: "{}".to_string(),
            extra_headers: vec![("Retry-After", "120".to_string())],
        }])
        .await;
        let error =
            refresh_access_token_at(&reqwest::Client::new(), &endpoint, "refresh", fixed_now())
                .await
                .unwrap_err();
        assert!(matches!(
            error,
            GeminiError::RateLimited {
                retry_after_secs: Some(120)
            }
        ));
        assert!(error.is_transient());
        assert!(!error.requires_user_action());
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn refresh_429_without_retry_after_returns_none_secs() {
        let (endpoint, handle) = mock_token_server(vec![MockResponse {
            status: 429,
            body: "{}".to_string(),
            extra_headers: Vec::new(),
        }])
        .await;
        let error =
            refresh_access_token_at(&reqwest::Client::new(), &endpoint, "refresh", fixed_now())
                .await
                .unwrap_err();
        assert!(matches!(
            error,
            GeminiError::RateLimited {
                retry_after_secs: None
            }
        ));
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn refresh_5xx_is_transient_token_refresh_http() {
        let (endpoint, handle) = mock_token_server(vec![MockResponse {
            status: 503,
            body: "{}".to_string(),
            extra_headers: Vec::new(),
        }])
        .await;
        let error =
            refresh_access_token_at(&reqwest::Client::new(), &endpoint, "refresh", fixed_now())
                .await
                .unwrap_err();
        assert!(matches!(
            error,
            GeminiError::TokenRefreshHttp { status: 503 }
        ));
        assert!(error.is_transient());
        assert!(!error.requires_user_action());
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn refresh_network_error_is_transient_and_offline() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let endpoint = format!("http://{addr}/token");
        let error = refresh_access_token_at(
            &reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(2))
                .build()
                .unwrap(),
            &endpoint,
            "refresh",
            fixed_now(),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, GeminiError::TokenRefreshRequest(_)));
        assert!(error.is_network_unavailable());
        assert!(error.is_transient());
    }

    #[tokio::test]
    async fn exponential_backoff_caps_at_3600_secs() {
        use crate::runtime::rate_limit_backoff_secs;
        assert_eq!(rate_limit_backoff_secs(1), 300);
        assert_eq!(rate_limit_backoff_secs(2), 600);
        assert_eq!(rate_limit_backoff_secs(3), 1200);
        assert_eq!(rate_limit_backoff_secs(4), 2400);
        assert_eq!(rate_limit_backoff_secs(5), 3600);
        assert_eq!(rate_limit_backoff_secs(10), 3600);
    }
}
