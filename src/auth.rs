// SPDX-License-Identifier: MPL-2.0

use crate::error::{AuthError, Result};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct CodexAuth {
    pub access_token: String,
    pub account_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ClaudeAuth {
    pub access_token: String,
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
    scopes: Vec<String>,
    #[serde(rename = "subscriptionType")]
    subscription_type: Option<String>,
    #[serde(rename = "expiresAt")]
    expires_at_ms: Option<i64>,
}

pub fn load_codex_auth() -> Result<CodexAuth, AuthError> {
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|p| p.join(".codex")))
        .ok_or(AuthError::ResolveCodexHome)?;
    let path = home.join("auth.json");
    let raw = fs::read_to_string(&path).map_err(|source| AuthError::ReadCodexAuthFile {
        path: path.clone(),
        source,
    })?;
    let parsed: CodexAuthFile =
        serde_json::from_str(&raw).map_err(AuthError::ParseCodexAuthJson)?;
    Ok(CodexAuth {
        access_token: parsed.tokens.access_token,
        account_id: parsed.tokens.account_id,
    })
}

pub fn load_claude_auth() -> Result<ClaudeAuth, AuthError> {
    let path = claude_credentials_path()?;
    load_claude_auth_from_path(&path)
}

pub fn claude_credentials_path() -> Result<PathBuf, AuthError> {
    let home = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("CLAUDE_HOME").map(PathBuf::from))
        .or_else(|| dirs::home_dir().map(|p| p.join(".claude")))
        .ok_or(AuthError::ResolveClaudeHome)?;
    Ok(home.join(".credentials.json"))
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

        // simulate CODEX_HOME
        unsafe { std::env::set_var("CODEX_HOME", dir.to_str().unwrap()) };
        let auth = load_codex_auth().unwrap();
        unsafe { std::env::remove_var("CODEX_HOME") };

        assert_eq!(auth.access_token, "codex-tok");
        assert_eq!(auth.account_id.as_deref(), Some("acc-123"));
    }

    #[test]
    fn missing_claude_credentials_returns_io_error() {
        let dir = temp_dir();
        let path = dir.join("nonexistent.json");
        let err = load_claude_auth_from_path(&path).unwrap_err();
        assert!(matches!(err, AuthError::ReadClaudeCredentials { .. }));
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
