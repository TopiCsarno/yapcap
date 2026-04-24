// SPDX-License-Identifier: MPL-2.0

use crate::error::{AuthError, Result};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct CodexAuth {
    pub access_token: String,
    pub account_id: Option<String>,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ClaudeAuth {
    pub access_token: String,
    pub id_token: Option<String>,
    pub scopes: Vec<String>,
    pub subscription_type: Option<String>,
    pub expires_at_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct CodexAuthFile {
    tokens: CodexTokens,
}

#[derive(Debug, Deserialize)]
struct CodexTokens {
    access_token: String,
    account_id: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClaudeCredentialsFile {
    #[serde(rename = "claudeAiOauth")]
    oauth: ClaudeOauthBlock,
}

#[derive(Debug, Deserialize)]
struct ClaudeOauthBlock {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "idToken", alias = "id_token", default)]
    id_token: Option<String>,
    scopes: Vec<String>,
    #[serde(rename = "subscriptionType")]
    subscription_type: Option<String>,
    #[serde(rename = "expiresAt")]
    expires_at_ms: Option<i64>,
}

pub fn codex_home() -> Result<PathBuf, AuthError> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|p| p.join(".codex")))
        .ok_or(AuthError::ResolveCodexHome)
}

pub fn load_codex_auth_from_home(home: &Path) -> Result<CodexAuth, AuthError> {
    load_codex_auth_from_path(&home.join("auth.json"))
}

pub fn load_codex_auth_from_path(path: &Path) -> Result<CodexAuth, AuthError> {
    let raw = fs::read_to_string(path).map_err(|source| AuthError::ReadCodexAuthFile {
        path: path.to_path_buf(),
        source,
    })?;
    let parsed: CodexAuthFile =
        serde_json::from_str(&raw).map_err(AuthError::ParseCodexAuthJson)?;
    Ok(CodexAuth {
        access_token: parsed.tokens.access_token,
        account_id: parsed.tokens.account_id,
        refresh_token: parsed.tokens.refresh_token,
        id_token: parsed.tokens.id_token,
    })
}

pub fn update_codex_auth_tokens(
    path: &Path,
    access_token: &str,
    refresh_token: Option<&str>,
    last_refresh_iso: Option<&str>,
) -> Result<(), AuthError> {
    let raw = fs::read_to_string(path).map_err(|source| AuthError::ReadCodexAuthFile {
        path: path.to_path_buf(),
        source,
    })?;
    let mut parsed: serde_json::Value =
        serde_json::from_str(&raw).map_err(AuthError::ParseCodexAuthJson)?;

    let tokens = parsed
        .as_object_mut()
        .and_then(|obj| obj.get_mut("tokens"))
        .and_then(serde_json::Value::as_object_mut)
        .ok_or(AuthError::InvalidCodexAuthShape)?;

    tokens.insert(
        "access_token".to_string(),
        serde_json::Value::String(access_token.to_string()),
    );
    if let Some(refresh_token) = refresh_token {
        tokens.insert(
            "refresh_token".to_string(),
            serde_json::Value::String(refresh_token.to_string()),
        );
    }
    if let Some(last_refresh_iso) = last_refresh_iso {
        parsed
            .as_object_mut()
            .ok_or(AuthError::InvalidCodexAuthShape)?
            .insert(
                "last_refresh".to_string(),
                serde_json::Value::String(last_refresh_iso.to_string()),
            );
    }

    let serialized =
        serde_json::to_string_pretty(&parsed).map_err(AuthError::ParseCodexAuthJson)?;
    fs::write(path, serialized).map_err(|source| AuthError::ReadCodexAuthFile {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

pub fn email_from_id_token(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = base64url_decode(payload)?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    json.get("email")?.as_str().map(str::to_string)
}

pub fn email_from_claude_access_token(access_token: &str) -> Option<String> {
    let mut parts = access_token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let bytes = base64url_decode(payload)?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    for key in [
        "email",
        "user_email",
        "https://claude.ai/email",
        "https://anthropic.com/email",
    ] {
        if let Some(value) = json.get(key).and_then(|v| v.as_str())
            && let Some(email) = email_from_plain_str(value)
        {
            return Some(email);
        }
    }
    if let Some(sub) = json.get("sub").and_then(|v| v.as_str())
        && let Some(email) = email_from_plain_str(sub)
    {
        return Some(email);
    }
    email_like_in_json(&json, 12)
}

pub fn email_from_claude_credentials(auth: &ClaudeAuth) -> Option<String> {
    email_from_claude_access_token(&auth.access_token).or_else(|| {
        auth.id_token.as_deref().and_then(|token| {
            email_from_id_token(token).or_else(|| email_from_claude_access_token(token))
        })
    })
}

fn email_from_plain_str(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.len() <= 3 || !s.contains('@') || s.chars().any(char::is_whitespace) {
        return None;
    }
    Some(s.to_string())
}

fn email_like_in_json(value: &serde_json::Value, depth: u8) -> Option<String> {
    if depth == 0 {
        return None;
    }
    match value {
        serde_json::Value::String(s) => email_from_plain_str(s),
        serde_json::Value::Array(items) => items
            .iter()
            .find_map(|item| email_like_in_json(item, depth - 1)),
        serde_json::Value::Object(map) => map
            .values()
            .find_map(|item| email_like_in_json(item, depth - 1)),
        _ => None,
    }
}

fn base64url_decode(input: &str) -> Option<Vec<u8>> {
    const INVALID: u8 = 0xFF;
    let mut table = [INVALID; 128];
    for (i, &c) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
        .iter()
        .enumerate()
    {
        table[c as usize] = u8::try_from(i).unwrap_or(INVALID);
    }
    table[b'-' as usize] = 62;
    table[b'_' as usize] = 63;

    let mut output = Vec::with_capacity(input.len() * 3 / 4 + 2);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for &byte in input.as_bytes() {
        if byte == b'=' || byte > 127 {
            break;
        }
        let val = table[byte as usize];
        if val == INVALID {
            return None;
        }
        buf = (buf << 6) | u32::from(val);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push(u8::try_from((buf >> bits) & 0xFF).unwrap_or(0));
        }
    }
    Some(output)
}

pub fn claude_credentials_path_for_config_dir(config_dir: &Path) -> PathBuf {
    config_dir.join(".credentials.json")
}

pub fn load_claude_auth_from_config_dir(config_dir: &Path) -> Result<ClaudeAuth, AuthError> {
    load_claude_auth_from_path(&claude_credentials_path_for_config_dir(config_dir))
}

pub fn load_claude_auth_from_path(path: &Path) -> Result<ClaudeAuth, AuthError> {
    let raw = fs::read_to_string(path).map_err(|source| AuthError::ReadClaudeCredentials {
        path: path.to_path_buf(),
        source,
    })?;
    let parsed: ClaudeCredentialsFile =
        serde_json::from_str(&raw).map_err(AuthError::ParseClaudeCredentials)?;
    Ok(ClaudeAuth {
        access_token: parsed.oauth.access_token,
        id_token: parsed.oauth.id_token,
        scopes: parsed.oauth.scopes,
        subscription_type: parsed.oauth.subscription_type,
        expires_at_ms: parsed.oauth.expires_at_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yapcap-auth-test-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn loads_claude_credentials_with_all_fields() {
        let dir = temp_dir();
        let path = dir.join(".credentials.json");
        fs::write(
            &path,
            r#"{
              "claudeAiOauth": {
                "accessToken": "tok-abc",
                "refreshToken": "ref-xyz",
                "expiresAt": 1776609779660,
                "scopes": ["user:profile", "user:inference"],
                "subscriptionType": "pro"
              }
            }"#,
        )
        .unwrap();

        let auth = load_claude_auth_from_path(&path).unwrap();
        assert_eq!(auth.access_token, "tok-abc");
        assert_eq!(auth.expires_at_ms, Some(1776609779660));
        assert_eq!(auth.subscription_type.as_deref(), Some("pro"));
        assert!(auth.scopes.contains(&"user:profile".to_string()));
    }

    #[test]
    fn loads_codex_auth_with_account_id() {
        let dir = temp_dir();
        let path = dir.join("auth.json");
        fs::write(
            &path,
            r#"{"tokens": {"access_token": "codex-tok", "account_id": "acc-123"}}"#,
        )
        .unwrap();

        let auth = load_codex_auth_from_home(&dir).unwrap();

        assert_eq!(auth.access_token, "codex-tok");
        assert_eq!(auth.account_id.as_deref(), Some("acc-123"));
        assert!(auth.id_token.is_none());
    }

    #[test]
    fn missing_claude_credentials_returns_io_error() {
        let dir = temp_dir();
        let path = dir.join("nonexistent.json");
        let err = load_claude_auth_from_path(&path).unwrap_err();
        assert!(matches!(err, AuthError::ReadClaudeCredentials { .. }));
    }

    #[test]
    fn extracts_email_from_id_token() {
        let payload = r#"{"email":"topi2236@gmail.com","sub":"auth0|abc"}"#;
        let encoded = {
            let b = payload.as_bytes();
            let mut out = String::new();
            let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut i = 0;
            while i < b.len() {
                let b0 = b[i] as u32;
                let b1 = if i + 1 < b.len() { b[i + 1] as u32 } else { 0 };
                let b2 = if i + 2 < b.len() { b[i + 2] as u32 } else { 0 };
                out.push(table[((b0 >> 2) & 0x3f) as usize] as char);
                out.push(table[(((b0 << 4) | (b1 >> 4)) & 0x3f) as usize] as char);
                out.push(if i + 1 < b.len() {
                    table[(((b1 << 2) | (b2 >> 6)) & 0x3f) as usize] as char
                } else {
                    '='
                });
                out.push(if i + 2 < b.len() {
                    table[(b2 & 0x3f) as usize] as char
                } else {
                    '='
                });
                i += 3;
            }
            out.replace('+', "-").replace('/', "_").replace('=', "")
        };
        let token = format!("header.{encoded}.sig");
        assert_eq!(
            email_from_id_token(&token),
            Some("topi2236@gmail.com".to_string())
        );
    }

    #[test]
    fn extracts_email_from_claude_access_token_jwt() {
        use base64::Engine;
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("{}");
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"email":"oauth@example.com"}"#);
        let token = format!("{header}.{payload}.sig");
        assert_eq!(
            email_from_claude_access_token(&token).as_deref(),
            Some("oauth@example.com")
        );
        assert_eq!(
            email_from_claude_access_token("opaque-token-without-jwt-shape"),
            None
        );
    }

    #[test]
    fn extracts_email_from_two_part_jwt_access_token() {
        use base64::Engine;
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("{}");
        let payload =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"email":"two@part.dev"}"#);
        let token = format!("{header}.{payload}");
        assert_eq!(
            email_from_claude_access_token(&token).as_deref(),
            Some("two@part.dev")
        );
    }

    #[test]
    fn extracts_email_from_nested_claim_in_jwt() {
        use base64::Engine;
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("{}");
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"user":{"profile":{"primary_email":"nested@example.com"}}}"#);
        let token = format!("{header}.{payload}.sig");
        assert_eq!(
            email_from_claude_access_token(&token).as_deref(),
            Some("nested@example.com")
        );
    }

    #[test]
    fn email_from_claude_credentials_falls_back_to_id_token() {
        use base64::Engine;
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("{}");
        let id_payload =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"email":"via-id@x.com"}"#);
        let id_jwt = format!("{header}.{id_payload}.s");
        let auth = ClaudeAuth {
            access_token: "opaque".to_string(),
            id_token: Some(id_jwt),
            scopes: vec![],
            subscription_type: None,
            expires_at_ms: None,
        };
        assert_eq!(
            email_from_claude_credentials(&auth).as_deref(),
            Some("via-id@x.com")
        );
    }

    #[test]
    fn loads_claude_id_token_field() {
        let dir = temp_dir();
        let path = dir.join(".credentials.json");
        fs::write(
            &path,
            r#"{
              "claudeAiOauth": {
                "accessToken": "opaque",
                "idToken": "id-tok-value",
                "scopes": ["user:profile"]
              }
            }"#,
        )
        .unwrap();
        let auth = load_claude_auth_from_path(&path).unwrap();
        assert_eq!(auth.id_token.as_deref(), Some("id-tok-value"));
    }

    #[test]
    fn bad_json_returns_parse_error() {
        let dir = temp_dir();
        let path = dir.join(".credentials.json");
        fs::write(&path, "not json").unwrap();
        let err = load_claude_auth_from_path(&path).unwrap_err();
        assert!(matches!(err, AuthError::ParseClaudeCredentials(_)));
    }
}
