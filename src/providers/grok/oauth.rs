// SPDX-License-Identifier: MPL-2.0

use std::fmt::Write as _;
use std::io::Read as _;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::error::GrokError;

pub const ISSUER: &str = "https://auth.x.ai";
pub const CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
pub const AUTHORIZE_URL: &str = "https://auth.x.ai/oauth2/authorize";
pub const TOKEN_URL: &str = "https://auth.x.ai/oauth2/token";
pub const SCOPES: &str = "openid profile email offline_access grok-cli:access api:access";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkceCodes {
    pub code_verifier: String,
    pub code_challenge: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
pub struct GrokClaims {
    #[serde(default)]
    pub sub: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub team_id: Option<String>,
    #[serde(default)]
    pub tier: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrokTokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
    pub id_token: Option<String>,
    pub token_type: Option<String>,
    pub scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawTokenResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    expires_at: Option<i64>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

pub fn new_pkce() -> PkceCodes {
    let bytes = random_bytes();
    let code_verifier = URL_SAFE_NO_PAD.encode(&bytes[..32]);
    let digest = Sha256::digest(code_verifier.as_bytes());
    let code_challenge = URL_SAFE_NO_PAD.encode(digest);
    PkceCodes {
        code_verifier,
        code_challenge,
    }
}

pub fn new_state() -> String {
    let bytes = random_bytes();
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    bytes[..32]
        .iter()
        .map(|&b| CHARSET[(b as usize) % CHARSET.len()] as char)
        .collect()
}

pub fn authorization_url(redirect_uri: &str, pkce: &PkceCodes, state: &str) -> String {
    let params = [
        ("response_type", "code"),
        ("client_id", CLIENT_ID),
        ("redirect_uri", redirect_uri),
        ("scope", SCOPES),
        ("code_challenge", pkce.code_challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("state", state),
    ];
    let query = params
        .into_iter()
        .map(|(key, value)| format!("{}={}", percent_encode(key), percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{AUTHORIZE_URL}?{query}")
}

pub async fn exchange_code(
    client: &reqwest::Client,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
    token_url: &str,
) -> Result<GrokTokenResponse, GrokError> {
    let params = [
        ("grant_type", "authorization_code"),
        ("client_id", CLIENT_ID),
        ("code", code),
        ("code_verifier", code_verifier),
        ("redirect_uri", redirect_uri),
    ];
    let response = client
        .post(token_url)
        .header("User-Agent", "yapcap")
        .form(&params)
        .send()
        .await
        .map_err(GrokError::TokenRefreshRequest)?;

    let status = response.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let retry_after_secs = parse_retry_after(response.headers());
        return Err(GrokError::RateLimited { retry_after_secs });
    }
    if !status.is_success() {
        return Err(GrokError::TokenRefreshHttp {
            status: status.as_u16(),
        });
    }

    let body = response
        .text()
        .await
        .map_err(GrokError::TokenRefreshDecode)?;
    parse_token_response(&body)
}

pub async fn refresh_token(
    client: &reqwest::Client,
    refresh_token: &str,
    token_url: &str,
) -> Result<GrokTokenResponse, GrokError> {
    let params = [
        ("grant_type", "refresh_token"),
        ("client_id", CLIENT_ID),
        ("refresh_token", refresh_token),
    ];
    let response = client
        .post(token_url)
        .header("User-Agent", "yapcap")
        .form(&params)
        .send()
        .await
        .map_err(GrokError::TokenRefreshRequest)?;

    let status = response.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let retry_after_secs = parse_retry_after(response.headers());
        return Err(GrokError::RateLimited { retry_after_secs });
    }
    if !status.is_success() {
        return Err(GrokError::TokenRefreshHttp {
            status: status.as_u16(),
        });
    }

    let body = response
        .text()
        .await
        .map_err(GrokError::TokenRefreshDecode)?;
    parse_token_response_at(&body, Some(refresh_token), Utc::now())
}

pub fn parse_token_response(raw: &str) -> Result<GrokTokenResponse, GrokError> {
    parse_token_response_at(raw, None, Utc::now())
}

pub fn parse_token_response_at(
    raw: &str,
    fallback_refresh_token: Option<&str>,
    now: DateTime<Utc>,
) -> Result<GrokTokenResponse, GrokError> {
    let raw_resp: RawTokenResponse =
        serde_json::from_str(raw).map_err(|e| GrokError::TokenRefreshParse(e.to_string()))?;

    let access_token = raw_resp
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .ok_or_else(|| GrokError::TokenRefreshParse("missing access_token".to_string()))?;

    let refresh_token = raw_resp
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .or_else(|| fallback_refresh_token.map(str::to_string))
        .ok_or_else(|| GrokError::TokenRefreshParse("missing refresh_token".to_string()))?;

    let expires_at = calculate_expires_at(&raw_resp, &access_token, now)?;

    Ok(GrokTokenResponse {
        access_token,
        refresh_token,
        expires_at,
        id_token: raw_resp.id_token.filter(|token| !token.trim().is_empty()),
        token_type: raw_resp.token_type.filter(|token| !token.trim().is_empty()),
        scope: raw_resp.scope.filter(|token| !token.trim().is_empty()),
    })
}

pub fn decode_jwt_claims(token: &str) -> Option<GrokClaims> {
    let payload = token.split('.').nth(1)?;
    let unpadded = payload.trim_end_matches('=');
    let bytes = URL_SAFE_NO_PAD.decode(unpadded).ok()?;
    serde_json::from_slice::<GrokClaims>(&bytes).ok()
}

fn calculate_expires_at(
    raw: &RawTokenResponse,
    access_token: &str,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, GrokError> {
    if let Some(expires_in) = raw.expires_in {
        if expires_in <= 0 {
            return Err(GrokError::TokenRefreshParse(
                "invalid expires_in".to_string(),
            ));
        }
        return Ok(now + Duration::seconds(expires_in));
    }
    if let Some(expires_at) = raw.expires_at {
        return DateTime::from_timestamp(expires_at, 0)
            .ok_or_else(|| GrokError::TokenRefreshParse("invalid expires_at".to_string()));
    }
    if let Some(exp) = jwt_expiration_claim(access_token) {
        return Ok(exp);
    }
    if let Some(id_token) = &raw.id_token
        && let Some(exp) = jwt_expiration_claim(id_token)
    {
        return Ok(exp);
    }
    Ok(now + Duration::seconds(3600))
}

fn jwt_expiration_claim(token: &str) -> Option<DateTime<Utc>> {
    let payload = token.split('.').nth(1)?;
    let unpadded = payload.trim_end_matches('=');
    let bytes = URL_SAFE_NO_PAD.decode(unpadded).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let exp = value.get("exp")?.as_i64()?;
    DateTime::<Utc>::from_timestamp(exp, 0)
}

fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
}

pub fn percent_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            write!(out, "%{byte:02X}").expect("writing to string cannot fail");
        }
    }
    out
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
