// SPDX-License-Identifier: MPL-2.0

use std::num::ParseFloatError;
use std::path::PathBuf;

use thiserror::Error;

pub type Result<T, E = AppError> = std::result::Result<T, E>;
pub const OFFLINE_MESSAGE: &str = "No internet connection. Information is not up to date.";

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Cache(#[from] CacheError),
    #[error(transparent)]
    Logging(#[from] LoggingError),
    #[error(transparent)]
    Provider(#[from] ProviderError),
}

impl From<CodexError> for AppError {
    fn from(value: CodexError) -> Self {
        Self::Provider(ProviderError::Codex(value))
    }
}

impl From<ClaudeError> for AppError {
    fn from(value: ClaudeError) -> Self {
        Self::Provider(ProviderError::Claude(value))
    }
}

impl From<CursorError> for AppError {
    fn from(value: CursorError) -> Self {
        Self::Provider(ProviderError::Cursor(value))
    }
}

impl AppError {
    #[must_use]
    pub fn user_message(&self) -> String {
        if self.is_network_unavailable() {
            OFFLINE_MESSAGE.to_string()
        } else {
            format!("{self:#}")
        }
    }

    #[must_use]
    pub fn is_network_unavailable(&self) -> bool {
        match self {
            Self::Provider(error) => error.is_network_unavailable(),
            Self::Cache(_) | Self::Logging(_) => false,
        }
    }

    #[must_use]
    pub fn requires_user_action(&self) -> bool {
        match self {
            Self::Provider(error) => error.requires_user_action(),
            Self::Cache(_) | Self::Logging(_) => false,
        }
    }

    #[must_use]
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Provider(error) => error.is_transient(),
            _ => false,
        }
    }

    #[must_use]
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::Provider(ProviderError::Claude(e)) if e.is_rate_limited())
    }

    #[must_use]
    pub fn rate_limit_retry_after_secs(&self) -> Option<u64> {
        match self {
            Self::Provider(ProviderError::Claude(e)) => e.rate_limit_retry_after_secs(),
            _ => None,
        }
    }
}

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("failed to read cache {path}")]
    ReadCache {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse cached snapshots")]
    ParseCache(#[source] serde_json::Error),
    #[error("failed to create {path}")]
    CreateCacheDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to encode cache")]
    EncodeCache(#[source] serde_json::Error),
    #[error("failed to write cache {path}")]
    WriteCache {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Error)]
pub enum LoggingError {
    #[error("failed to create {path}")]
    CreateLogDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to initialize tracing")]
    InitTracing(#[source] tracing_subscriber::util::TryInitError),
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error(transparent)]
    Codex(#[from] CodexError),
    #[error(transparent)]
    Claude(#[from] ClaudeError),
    #[error(transparent)]
    Cursor(#[from] CursorError),
}

impl ProviderError {
    #[must_use]
    pub fn is_network_unavailable(&self) -> bool {
        match self {
            Self::Codex(error) => error.is_network_unavailable(),
            Self::Claude(error) => error.is_network_unavailable(),
            Self::Cursor(error) => error.is_network_unavailable(),
        }
    }

    #[must_use]
    pub fn requires_user_action(&self) -> bool {
        match self {
            Self::Codex(error) => error.requires_user_action(),
            Self::Claude(error) => error.requires_user_action(),
            Self::Cursor(error) => error.requires_user_action(),
        }
    }

    #[must_use]
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Claude(error) => error.is_transient(),
            Self::Codex(error) => error.is_transient(),
            Self::Cursor(_) => false,
        }
    }
}

fn request_could_not_reach_network(error: &reqwest::Error) -> bool {
    error.is_connect() || (!error.is_status() && error.is_timeout())
}

fn format_retry_secs(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}h", secs / 3600)
    } else if secs >= 60 {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}

#[derive(Debug, Error)]
pub enum CodexError {
    #[error("failed to read Codex account storage: {0}")]
    AccountStorage(String),
    #[error("invalid codex bearer header")]
    InvalidBearerHeader(#[source] reqwest::header::InvalidHeaderValue),
    #[error("invalid codex account id header")]
    InvalidAccountIdHeader(#[source] reqwest::header::InvalidHeaderValue),
    #[error("codex usage request failed")]
    UsageRequest(#[source] reqwest::Error),
    #[error("Codex login required")]
    Unauthorized,
    #[error("codex usage endpoint returned HTTP {status}{details}")]
    UsageHttp { status: u16, details: String },
    #[error("failed to decode codex usage response")]
    DecodeUsageJson(#[source] serde_json::Error),
    #[error("codex token refresh not available")]
    RefreshUnavailable,
    #[error("codex token refresh request failed")]
    RefreshRequest(#[source] reqwest::Error),
    #[error("codex token refresh returned HTTP {status}{details}")]
    RefreshHttp { status: u16, details: String },
    #[error("failed to decode codex token refresh response")]
    RefreshDecode(#[source] reqwest::Error),
    #[error("Codex response had no usage windows")]
    NoUsageData,
    #[error("failed to parse codex credit balance {balance}")]
    InvalidCreditBalance {
        balance: String,
        #[source]
        source: ParseFloatError,
    },
}

impl CodexError {
    #[must_use]
    pub fn is_network_unavailable(&self) -> bool {
        match self {
            Self::UsageRequest(source) | Self::RefreshRequest(source) => {
                request_could_not_reach_network(source)
            }
            _ => false,
        }
    }

    #[must_use]
    pub fn requires_user_action(&self) -> bool {
        matches!(
            self,
            Self::Unauthorized
                | Self::RefreshUnavailable
                | Self::RefreshHttp {
                    status: 400 | 401 | 403,
                    ..
                }
        )
    }

    #[must_use]
    pub fn is_transient(&self) -> bool {
        match self {
            Self::UsageRequest(source) | Self::RefreshRequest(source) => {
                request_could_not_reach_network(source)
            }
            Self::RefreshHttp { status, .. } => *status == 429 || *status >= 500,
            _ => false,
        }
    }
}

#[derive(Debug, Error)]
pub enum ClaudeError {
    #[error("Claude token missing user:profile scope")]
    MissingProfileScope,
    #[error("invalid claude bearer header")]
    InvalidBearerHeader(#[source] reqwest::header::InvalidHeaderValue),
    #[error("claude usage request failed")]
    UsageRequest(#[source] reqwest::Error),
    #[error("Claude token unauthorized or expired")]
    Unauthorized,
    #[error("Rate limited by Claude{} — will retry automatically",
        .retry_after_secs.map_or(String::new(), |s| format!(" (retry in {})", format_retry_secs(s))))]
    RateLimited { retry_after_secs: Option<u64> },
    #[error("claude token refresh request failed")]
    TokenRefreshRequest(#[source] reqwest::Error),
    #[error("claude token refresh returned HTTP {status}")]
    TokenRefreshHttp { status: u16 },
    #[error("failed to decode claude token refresh response")]
    TokenRefreshDecode(#[source] reqwest::Error),
    #[error("failed to parse claude token refresh response: {0}")]
    TokenRefreshParse(String),
    #[error("claude usage endpoint returned HTTP {status}")]
    UsageEndpoint {
        status: u16,
        #[source]
        source: reqwest::Error,
    },
    #[error("failed to decode claude usage response")]
    DecodeUsage(#[source] reqwest::Error),
    #[error("Claude response had no usage windows")]
    NoUsageData,
    #[error("invalid claude reset timestamp {value}")]
    InvalidResetTimestamp {
        value: String,
        #[source]
        source: chrono::ParseError,
    },
}

impl ClaudeError {
    #[must_use]
    pub fn is_network_unavailable(&self) -> bool {
        match self {
            Self::UsageRequest(source) | Self::TokenRefreshRequest(source) => {
                request_could_not_reach_network(source)
            }
            _ => false,
        }
    }

    #[must_use]
    pub fn requires_user_action(&self) -> bool {
        if let Self::TokenRefreshHttp { status } = self {
            return (400..500).contains(status) && *status != 429;
        }
        matches!(self, Self::MissingProfileScope | Self::Unauthorized)
    }

    #[must_use]
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::RateLimited { .. })
    }

    #[must_use]
    pub fn rate_limit_retry_after_secs(&self) -> Option<u64> {
        match self {
            Self::RateLimited { retry_after_secs } => *retry_after_secs,
            _ => None,
        }
    }

    #[must_use]
    pub fn is_transient(&self) -> bool {
        match self {
            Self::RateLimited { .. } => true,
            Self::TokenRefreshRequest(source) => request_could_not_reach_network(source),
            Self::TokenRefreshHttp { status } => *status == 429 || *status >= 500,
            _ => false,
        }
    }
}

#[derive(Debug, Error)]
pub enum CursorError {
    #[error("invalid cursor cookie header")]
    InvalidCookieHeader(#[source] reqwest::header::InvalidHeaderValue),
    #[error("cursor usage request failed")]
    UsageRequest(#[source] reqwest::Error),
    #[error("Cursor login required")]
    Unauthorized,
    #[error("cursor usage endpoint returned error")]
    UsageEndpoint(#[source] reqwest::Error),
    #[error("failed to decode cursor usage response")]
    DecodeUsage(#[source] reqwest::Error),
    #[error("cursor identity request failed")]
    IdentityRequest(#[source] reqwest::Error),
    #[error("failed to decode cursor identity response")]
    DecodeIdentity(#[source] reqwest::Error),
    #[error("invalid cursor billing cycle end {value}")]
    InvalidBillingCycleEnd {
        value: String,
        #[source]
        source: chrono::ParseError,
    },
    #[error("Cursor state database not found at {path}")]
    StateDbNotFound { path: PathBuf },
    #[error("failed to open Cursor state database")]
    StateDbOpen(#[source] rusqlite::Error),
    #[error("failed to query Cursor state database")]
    StateDbQuery(#[source] rusqlite::Error),
    #[error("Cursor state database is missing key: {0}")]
    StateDbMissingKey(String),
    #[error("JWT has {count} segments, expected 3")]
    JwtWrongSegments { count: usize },
    #[error("failed to base64-decode JWT payload")]
    JwtBase64(#[source] base64::DecodeError),
    #[error("JWT payload is not valid JSON")]
    JwtNotJson(#[source] serde_json::Error),
    #[error("JWT is missing 'sub' claim")]
    JwtMissingSub,
    #[error("JWT is missing valid 'exp' claim")]
    JwtMissingExp,
    #[error("Cursor token refresh request failed")]
    TokenRefreshRequest(#[source] reqwest::Error),
    #[error("Cursor session requires re-authentication")]
    TokenRefreshLogout,
    #[error("Cursor token refresh failed with status {status}")]
    TokenRefreshFailed { status: u16 },
    #[error("failed to decode Cursor token refresh response")]
    TokenRefreshDecode(#[source] reqwest::Error),
    #[error("Cursor account email not available")]
    ScanMissingEmail,
}

impl CursorError {
    #[must_use]
    pub fn is_network_unavailable(&self) -> bool {
        match self {
            Self::UsageRequest(source)
            | Self::IdentityRequest(source)
            | Self::TokenRefreshRequest(source) => request_could_not_reach_network(source),
            _ => false,
        }
    }

    #[must_use]
    pub fn requires_user_action(&self) -> bool {
        matches!(self, Self::Unauthorized | Self::TokenRefreshLogout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_error_does_not_require_user_action() {
        let err = AppError::Cache(CacheError::EncodeCache(
            serde_json::from_str::<i32>("!").unwrap_err(),
        ));
        assert!(!err.requires_user_action());
        assert!(!err.is_transient());
    }

    #[test]
    fn claude_rate_limit_is_transient() {
        let err = AppError::Provider(ProviderError::Claude(ClaudeError::RateLimited {
            retry_after_secs: None,
        }));
        assert!(!err.requires_user_action());
        assert!(err.is_transient());
    }

    #[test]
    fn claude_refresh_auth_failures_require_user_action() {
        for status in [400, 401, 403] {
            let err = AppError::Provider(ProviderError::Claude(ClaudeError::TokenRefreshHttp {
                status,
            }));
            assert!(err.requires_user_action());
            assert!(!err.is_transient());
        }
    }

    #[test]
    fn claude_refresh_rate_limit_and_server_errors_are_transient() {
        for status in [429, 500, 503] {
            let err = AppError::Provider(ProviderError::Claude(ClaudeError::TokenRefreshHttp {
                status,
            }));
            assert!(!err.requires_user_action());
            assert!(err.is_transient());
        }
    }

    #[test]
    fn codex_unauthorized_requires_user_action() {
        let err = AppError::Provider(ProviderError::Codex(CodexError::Unauthorized));
        assert!(err.requires_user_action());
        assert!(!err.is_transient());
    }

    #[test]
    fn codex_refresh_auth_failures_require_user_action() {
        for status in [400, 401, 403] {
            let err = AppError::Provider(ProviderError::Codex(CodexError::RefreshHttp {
                status,
                details: String::new(),
            }));
            assert!(err.requires_user_action());
            assert!(!err.is_transient());
        }
    }

    #[test]
    fn codex_refresh_rate_limit_and_server_errors_are_transient() {
        for status in [429, 500, 503] {
            let err = AppError::Provider(ProviderError::Codex(CodexError::RefreshHttp {
                status,
                details: String::new(),
            }));
            assert!(!err.requires_user_action());
            assert!(err.is_transient());
        }
    }

    #[test]
    fn cursor_unauthorized_requires_user_action() {
        let err = AppError::Provider(ProviderError::Cursor(CursorError::Unauthorized));
        assert!(err.requires_user_action());
    }

    #[test]
    fn codex_cli_errors_do_not_require_user_action_by_default() {
        let err = CodexError::NoUsageData;
        assert!(!err.requires_user_action());
    }
}
