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

pub fn load_codex_auth() -> Result<CodexAuth> {
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|path| path.join(".codex")))
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

pub fn load_claude_auth() -> Result<ClaudeAuth> {
    let path = claude_credentials_path()?;
    load_claude_auth_from_path(&path)
}

pub fn claude_credentials_path() -> Result<PathBuf> {
    let home = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("CLAUDE_HOME").map(PathBuf::from))
        .or_else(|| dirs::home_dir().map(|path| path.join(".claude")))
        .ok_or(AuthError::ResolveClaudeHome)?;
    Ok(home.join(".credentials.json"))
}

pub fn load_claude_auth_from_path(path: &Path) -> Result<ClaudeAuth> {
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
    use tempfile::tempdir;

    #[test]
    fn loads_claude_expires_at() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".credentials.json");
        fs::write(
            &path,
            r#"{
              "claudeAiOauth": {
                "accessToken": "token",
                "refreshToken": "refresh",
                "expiresAt": 1776609779660,
                "scopes": ["user:profile"],
                "subscriptionType": "pro"
              }
            }"#,
        )
        .unwrap();

        let auth = load_claude_auth_from_path(&path).unwrap();
        assert_eq!(auth.access_token, "token");
        assert_eq!(auth.expires_at_ms, Some(1776609779660));
        assert_eq!(auth.subscription_type.as_deref(), Some("pro"));
    }
}
