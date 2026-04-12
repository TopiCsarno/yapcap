use crate::error::{AuthError, Result};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct CodexAuth {
    pub access_token: String,
    pub account_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ClaudeAuth {
    pub access_token: String,
    pub scopes: Vec<String>,
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
    let home = std::env::var_os("CLAUDE_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|path| path.join(".claude")))
        .ok_or(AuthError::ResolveClaudeHome)?;
    let path = home.join(".credentials.json");
    let raw = fs::read_to_string(&path).map_err(|source| AuthError::ReadClaudeCredentials {
        path: path.clone(),
        source,
    })?;
    let parsed: ClaudeCredentialsFile =
        serde_json::from_str(&raw).map_err(AuthError::ParseClaudeCredentials)?;
    Ok(ClaudeAuth {
        access_token: parsed.oauth.access_token,
        scopes: parsed.oauth.scopes,
    })
}
