// SPDX-License-Identifier: MPL-2.0

use std::num::ParseFloatError;
use std::path::PathBuf;

use crate::model::ProviderId;
use thiserror::Error;

pub type Result<T, E = AppError> = std::result::Result<T, E>;
pub const OFFLINE_MESSAGE: &str = "No internet connection. Information is not up to date.";

#[derive(Debug, Error)]
pub enum AppError {
    #[error("invalid account handle for provider {provider:?}")]
    InvalidAccountHandle { provider: ProviderId },
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

impl From<GeminiError> for AppError {
    fn from(value: GeminiError) -> Self {
        Self::Provider(ProviderError::Gemini(value))
    }
}

impl From<CopilotError> for AppError {
    fn from(value: CopilotError) -> Self {
        Self::Provider(ProviderError::Copilot(value))
    }
}

impl From<MinimaxError> for AppError {
    fn from(value: MinimaxError) -> Self {
        Self::Provider(ProviderError::Minimax(value))
    }
}

impl From<KimiError> for AppError {
    fn from(value: KimiError) -> Self {
        Self::Provider(ProviderError::Kimi(value))
    }
}

impl From<AntigravityError> for AppError {
    fn from(value: AntigravityError) -> Self {
        Self::Provider(ProviderError::Antigravity(value))
    }
}

impl From<OpenCodeGoError> for AppError {
    fn from(value: OpenCodeGoError) -> Self {
        Self::Provider(ProviderError::OpenCodeGo(value))
    }
}

impl From<GrokError> for AppError {
    fn from(value: GrokError) -> Self {
        Self::Provider(ProviderError::Grok(value))
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
            Self::Logging(_) => false,
            Self::InvalidAccountHandle { .. } => false,
        }
    }

    #[must_use]
    pub fn requires_user_action(&self) -> bool {
        match self {
            Self::Provider(error) => error.requires_user_action(),
            Self::Logging(_) => false,
            Self::InvalidAccountHandle { .. } => false,
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
    pub fn rate_limit_retry_after_secs(&self) -> Option<u64> {
        match self {
            Self::Provider(ProviderError::Claude(e)) => e.rate_limit_retry_after_secs(),
            Self::Provider(ProviderError::Gemini(e)) => e.rate_limit_retry_after_secs(),
            Self::Provider(ProviderError::Copilot(e)) => e.rate_limit_retry_after_secs(),
            Self::Provider(ProviderError::Minimax(e)) => e.rate_limit_retry_after_secs(),
            Self::Provider(ProviderError::Kimi(e)) => e.rate_limit_retry_after_secs(),
            Self::Provider(ProviderError::Antigravity(e)) => e.rate_limit_retry_after_secs(),
            Self::Provider(ProviderError::OpenCodeGo(e)) => e.rate_limit_retry_after_secs(),
            Self::Provider(ProviderError::Grok(e)) => e.rate_limit_retry_after_secs(),
            _ => None,
        }
    }
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
    #[error(transparent)]
    Gemini(#[from] GeminiError),
    #[error(transparent)]
    Copilot(#[from] CopilotError),
    #[error(transparent)]
    Minimax(#[from] MinimaxError),
    #[error(transparent)]
    Kimi(#[from] KimiError),
    #[error(transparent)]
    Antigravity(#[from] AntigravityError),
    #[error(transparent)]
    OpenCodeGo(#[from] OpenCodeGoError),
    #[error(transparent)]
    Grok(#[from] GrokError),
}

impl ProviderError {
    #[must_use]
    pub fn is_network_unavailable(&self) -> bool {
        match self {
            Self::Codex(error) => error.is_network_unavailable(),
            Self::Claude(error) => error.is_network_unavailable(),
            Self::Cursor(error) => error.is_network_unavailable(),
            Self::Gemini(error) => error.is_network_unavailable(),
            Self::Copilot(error) => error.is_network_unavailable(),
            Self::Minimax(error) => error.is_network_unavailable(),
            Self::Kimi(error) => error.is_network_unavailable(),
            Self::Antigravity(error) => error.is_network_unavailable(),
            Self::OpenCodeGo(error) => error.is_network_unavailable(),
            Self::Grok(error) => error.is_network_unavailable(),
        }
    }

    #[must_use]
    pub fn requires_user_action(&self) -> bool {
        match self {
            Self::Codex(error) => error.requires_user_action(),
            Self::Claude(error) => error.requires_user_action(),
            Self::Cursor(error) => error.requires_user_action(),
            Self::Gemini(error) => error.requires_user_action(),
            Self::Copilot(error) => error.requires_user_action(),
            Self::Minimax(error) => error.requires_user_action(),
            Self::Kimi(error) => error.requires_user_action(),
            Self::Antigravity(error) => error.requires_user_action(),
            Self::OpenCodeGo(error) => error.requires_user_action(),
            Self::Grok(error) => error.requires_user_action(),
        }
    }

    #[must_use]
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Claude(error) => error.is_transient(),
            Self::Codex(error) => error.is_transient(),
            Self::Cursor(_) => false,
            Self::Gemini(error) => error.is_transient(),
            Self::Copilot(error) => error.is_transient(),
            Self::Minimax(error) => error.is_transient(),
            Self::Kimi(error) => error.is_transient(),
            Self::Antigravity(error) => error.is_transient(),
            Self::OpenCodeGo(error) => error.is_transient(),
            Self::Grok(error) => error.is_transient(),
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
    #[error("Codex credentials are missing; restore from OpenCode or sign in again")]
    CredentialsMissing,
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

#[derive(Debug, Error)]
pub enum GeminiError {
    #[error("gemini token refresh request failed")]
    TokenRefreshRequest(#[source] reqwest::Error),
    #[error("gemini token refresh returned HTTP {status}")]
    TokenRefreshHttp { status: u16 },
    #[error("failed to decode gemini token refresh response")]
    TokenRefreshDecode(#[source] reqwest::Error),
    #[error("failed to parse gemini token refresh response: {0}")]
    TokenRefreshParse(String),
    #[error("Rate limited by Gemini{} — will retry automatically",
        .retry_after_secs.map_or(String::new(), |s| format!(" (retry in {})", format_retry_secs(s))))]
    RateLimited { retry_after_secs: Option<u64> },
    #[error("Gemini login required")]
    Unauthorized,
    #[error("gemini code-assist request failed")]
    LoadCodeAssistRequest(#[source] reqwest::Error),
    #[error("gemini loadCodeAssist returned HTTP {status}")]
    LoadCodeAssistHttp { status: u16 },
    #[error("failed to parse gemini loadCodeAssist response: {0}")]
    LoadCodeAssistParse(String),
    #[error("gemini retrieveUserQuota request failed")]
    QuotaRequest(#[source] reqwest::Error),
    #[error("gemini retrieveUserQuota returned HTTP {status}")]
    QuotaHttp { status: u16 },
    #[error("failed to parse gemini retrieveUserQuota response: {0}")]
    QuotaParse(String),
    #[error("Gemini response had no usage windows")]
    NoUsageData,
    #[error(
        "Gemini account has no Code Assist project. Run `gemini` once to let Google auto-provision a project, then retry."
    )]
    NoCloudaicompanionProject,
    #[error("failed to read Gemini account storage: {0}")]
    AccountStorage(String),
}

impl GeminiError {
    #[must_use]
    pub fn is_network_unavailable(&self) -> bool {
        match self {
            Self::TokenRefreshRequest(source)
            | Self::LoadCodeAssistRequest(source)
            | Self::QuotaRequest(source) => request_could_not_reach_network(source),
            _ => false,
        }
    }

    #[must_use]
    pub fn requires_user_action(&self) -> bool {
        if let Self::TokenRefreshHttp { status } = self {
            return (400..500).contains(status) && *status != 429;
        }
        matches!(self, Self::Unauthorized)
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
            Self::TokenRefreshRequest(source)
            | Self::LoadCodeAssistRequest(source)
            | Self::QuotaRequest(source) => request_could_not_reach_network(source),
            Self::TokenRefreshHttp { status }
            | Self::LoadCodeAssistHttp { status }
            | Self::QuotaHttp { status } => *status == 429 || *status >= 500,
            _ => false,
        }
    }
}

#[derive(Debug, Error)]
pub enum CopilotError {
    #[error("Copilot login required")]
    LoginRequired,
    #[error("failed to read Copilot account storage: {0}")]
    AccountStorage(String),
    #[error("Copilot usage request failed")]
    UsageRequest(#[source] reqwest::Error),
    #[error("Copilot usage endpoint returned HTTP {status}")]
    UsageHttp { status: u16 },
    #[error("Copilot usage endpoint returned error")]
    UsageEndpoint(#[source] reqwest::Error),
    #[error("failed to decode Copilot usage response")]
    DecodeUsage(#[source] reqwest::Error),
    #[error("failed to parse Copilot usage response")]
    ParseUsage(#[source] serde_json::Error),
    #[error("invalid Copilot reset date {value}")]
    InvalidResetDate {
        value: String,
        #[source]
        source: chrono::ParseError,
    },
    #[error("Rate limited by Copilot{} — will retry automatically",
        .retry_after_secs.map_or(String::new(), |s| format!(" (retry in {})", format_retry_secs(s))))]
    RateLimited { retry_after_secs: Option<u64> },
    #[error("Unrecognized Copilot response: {detail}")]
    UnrecognizedResponse { detail: String },
}

impl CopilotError {
    #[must_use]
    pub fn is_network_unavailable(&self) -> bool {
        match self {
            Self::UsageRequest(source) => request_could_not_reach_network(source),
            _ => false,
        }
    }

    #[must_use]
    pub fn requires_user_action(&self) -> bool {
        matches!(self, Self::LoginRequired)
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
            Self::UsageRequest(source) => request_could_not_reach_network(source),
            Self::UsageHttp { status } => *status >= 500,
            _ => false,
        }
    }
}

#[derive(Debug, Error)]
pub enum MinimaxError {
    #[error("Minimax login required")]
    LoginRequired,
    #[error("Minimax usage request failed")]
    UsageRequest(#[source] reqwest::Error),
    #[error("Minimax usage endpoint returned HTTP {status}")]
    UsageHttp { status: u16 },
    #[error("Minimax usage endpoint returned error")]
    UsageEndpoint(#[source] reqwest::Error),
    #[error("failed to decode Minimax usage response")]
    DecodeUsage(#[source] serde_json::Error),
    #[error("failed to parse Minimax token plan response")]
    ParseTokenPlan,
    #[error("Rate limited by Minimax — will retry automatically")]
    RateLimited { retry_after_secs: Option<u64> },
    #[error("Minimax API error ({status_code}): {status_msg}")]
    ApiError {
        status_code: i32,
        status_msg: String,
    },
}

impl MinimaxError {
    #[must_use]
    pub fn is_network_unavailable(&self) -> bool {
        match self {
            Self::UsageRequest(source) => request_could_not_reach_network(source),
            _ => false,
        }
    }

    #[must_use]
    pub fn requires_user_action(&self) -> bool {
        matches!(self, Self::LoginRequired)
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
            Self::UsageRequest(source) => request_could_not_reach_network(source),
            Self::UsageHttp { status } => *status >= 500,
            _ => false,
        }
    }
}

#[derive(Debug, Error)]
pub enum KimiError {
    #[error("Kimi login required")]
    LoginRequired,
    #[error("Kimi usage request failed")]
    UsageRequest(#[source] reqwest::Error),
    #[error("Kimi usage endpoint returned HTTP {status}")]
    UsageHttp { status: u16 },
    #[error("Kimi usage endpoint returned error")]
    UsageEndpoint(#[source] reqwest::Error),
    #[error("failed to decode Kimi usage response")]
    DecodeUsage(#[source] serde_json::Error),
    #[error("Kimi response had no usage windows")]
    NoUsageData,
    #[error("Rate limited by Kimi — will retry automatically")]
    RateLimited { retry_after_secs: Option<u64> },
    #[error("Kimi API error: {message}")]
    ApiError { message: String },
}

impl KimiError {
    #[must_use]
    pub fn is_network_unavailable(&self) -> bool {
        match self {
            Self::UsageRequest(source) => request_could_not_reach_network(source),
            _ => false,
        }
    }

    #[must_use]
    pub fn requires_user_action(&self) -> bool {
        matches!(self, Self::LoginRequired)
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
            Self::UsageRequest(source) => request_could_not_reach_network(source),
            Self::UsageHttp { status } => *status >= 500,
            _ => false,
        }
    }
}

#[derive(Debug, Error)]
pub enum AntigravityError {
    #[error("antigravity token refresh request failed")]
    TokenRefreshRequest(#[source] reqwest::Error),
    #[error("antigravity token refresh returned HTTP {status}")]
    TokenRefreshHttp { status: u16 },
    #[error("failed to decode antigravity token refresh response")]
    TokenRefreshDecode(#[source] reqwest::Error),
    #[error("failed to parse antigravity token refresh response: {0}")]
    TokenRefreshParse(String),
    #[error("Rate limited by Antigravity{} — will retry automatically",
        .retry_after_secs.map_or(String::new(), |s| format!(" (retry in {})", format_retry_secs(s))))]
    RateLimited { retry_after_secs: Option<u64> },
    #[error("Antigravity login required")]
    Unauthorized,
    #[error("antigravity code-assist request failed")]
    LoadCodeAssistRequest(#[source] reqwest::Error),
    #[error("antigravity loadCodeAssist returned HTTP {status}")]
    LoadCodeAssistHttp { status: u16 },
    #[error("failed to parse antigravity loadCodeAssist response: {0}")]
    LoadCodeAssistParse(String),
    #[error("antigravity retrieveUserQuotaSummary request failed")]
    QuotaRequest(#[source] reqwest::Error),
    #[error("antigravity retrieveUserQuotaSummary returned HTTP {status}")]
    QuotaHttp { status: u16 },
    #[error("failed to parse antigravity retrieveUserQuotaSummary response: {0}")]
    QuotaParse(String),
    #[error("Antigravity response had no usage windows")]
    NoUsageData,
    #[error("failed to read Antigravity account storage: {0}")]
    AccountStorage(String),
}

impl AntigravityError {
    #[must_use]
    pub fn is_network_unavailable(&self) -> bool {
        match self {
            Self::TokenRefreshRequest(source)
            | Self::LoadCodeAssistRequest(source)
            | Self::QuotaRequest(source) => request_could_not_reach_network(source),
            _ => false,
        }
    }

    #[must_use]
    pub fn requires_user_action(&self) -> bool {
        if let Self::TokenRefreshHttp { status } = self {
            return (400..500).contains(status) && *status != 429;
        }
        matches!(self, Self::Unauthorized)
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
            Self::TokenRefreshRequest(source)
            | Self::LoadCodeAssistRequest(source)
            | Self::QuotaRequest(source) => request_could_not_reach_network(source),
            Self::TokenRefreshHttp { status }
            | Self::LoadCodeAssistHttp { status }
            | Self::QuotaHttp { status } => *status == 429 || *status >= 500,
            _ => false,
        }
    }
}

#[derive(Debug, Error)]
pub enum OpenCodeGoError {
    #[error("OpenCode Go login required")]
    LoginRequired,
    #[error("OpenCode Go subscription required")]
    EntitlementRequired,
    #[error("OpenCode Go usage request failed")]
    UsageRequest(#[source] reqwest::Error),
    #[error("OpenCode Go usage endpoint returned HTTP {status}")]
    UsageHttp { status: u16 },
    #[error("OpenCode Go usage endpoint returned error")]
    UsageEndpoint(#[source] reqwest::Error),
    #[error("failed to decode OpenCode Go usage response")]
    DecodeUsage(#[source] serde_json::Error),
    #[error("OpenCode Go response had no usage windows")]
    NoUsageData,
    #[error("Rate limited by OpenCode Go — will retry automatically")]
    RateLimited { retry_after_secs: Option<u64> },
}

impl OpenCodeGoError {
    #[must_use]
    pub fn is_network_unavailable(&self) -> bool {
        matches!(self, Self::UsageRequest(source) if request_could_not_reach_network(source))
    }

    #[must_use]
    pub fn requires_user_action(&self) -> bool {
        matches!(self, Self::LoginRequired)
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
            Self::UsageRequest(source) => request_could_not_reach_network(source),
            Self::UsageHttp { status } => *status >= 500,
            _ => false,
        }
    }
}

#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum GrokError {
    #[error("failed to read Grok account storage: {0}")]
    AccountStorage(String),
    #[error("Grok credentials are missing; restore from ~/.grok/auth.json or sign in again")]
    CredentialsMissing,
    #[error("invalid grok bearer header")]
    InvalidBearerHeader(#[source] reqwest::header::InvalidHeaderValue),
    #[error("grok usage request failed")]
    UsageRequest(#[source] reqwest::Error),
    #[error("Grok login required")]
    Unauthorized,
    #[error("Rate limited by Grok{} — will retry automatically",
        .retry_after_secs.map_or(String::new(), |s| format!(" (retry in {})", format_retry_secs(s))))]
    RateLimited { retry_after_secs: Option<u64> },
    #[error("grok token refresh request failed")]
    TokenRefreshRequest(#[source] reqwest::Error),
    #[error("grok token refresh returned HTTP {status}")]
    TokenRefreshHttp { status: u16 },
    #[error("failed to decode grok token refresh response")]
    TokenRefreshDecode(#[source] reqwest::Error),
    #[error("failed to parse grok token refresh response: {0}")]
    TokenRefreshParse(String),
    #[error("grok usage endpoint returned HTTP {status}")]
    UsageEndpoint {
        status: u16,
        #[source]
        source: reqwest::Error,
    },
    #[error("failed to decode grok usage response")]
    DecodeUsage(#[source] serde_json::Error),
    #[error("Grok response had no usage windows")]
    NoUsageData,
    #[error("invalid grok reset timestamp {value}")]
    InvalidResetTimestamp {
        value: String,
        #[source]
        source: chrono::ParseError,
    },
    #[error("grok token refresh not available")]
    RefreshUnavailable,
}

impl GrokError {
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
        matches!(
            self,
            Self::CredentialsMissing | Self::Unauthorized | Self::RefreshUnavailable
        )
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
            Self::UsageRequest(source) | Self::TokenRefreshRequest(source) => {
                request_could_not_reach_network(source)
            }
            Self::TokenRefreshHttp { status } => *status == 429 || *status >= 500,
            Self::UsageEndpoint { status, .. } => *status == 429 || *status >= 500,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kimi_rate_limit_routes_retry_after_through_app_error() {
        let err = AppError::Provider(ProviderError::Kimi(KimiError::RateLimited {
            retry_after_secs: Some(42),
        }));
        assert_eq!(err.rate_limit_retry_after_secs(), Some(42));
        assert!(err.is_transient());
        assert!(!err.requires_user_action());
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

    #[test]
    fn gemini_refresh_auth_failures_require_user_action() {
        for status in [400, 401, 403] {
            let err = AppError::Provider(ProviderError::Gemini(GeminiError::TokenRefreshHttp {
                status,
            }));
            assert!(err.requires_user_action());
            assert!(!err.is_transient());
        }
    }

    #[test]
    fn gemini_refresh_rate_limit_and_server_errors_are_transient() {
        for status in [429, 500, 503] {
            let err = AppError::Provider(ProviderError::Gemini(GeminiError::TokenRefreshHttp {
                status,
            }));
            assert!(!err.requires_user_action());
            assert!(err.is_transient());
        }
    }

    #[test]
    fn gemini_rate_limited_routes_retry_after_through_app_error() {
        let err = AppError::Provider(ProviderError::Gemini(GeminiError::RateLimited {
            retry_after_secs: Some(42),
        }));
        assert_eq!(err.rate_limit_retry_after_secs(), Some(42));
        assert!(err.is_transient());
        assert!(!err.requires_user_action());
    }

    #[test]
    fn copilot_auth_failures_require_user_action() {
        let err = AppError::Provider(ProviderError::Copilot(CopilotError::LoginRequired));
        assert!(err.requires_user_action());
        assert!(!err.is_transient());
    }

    #[test]
    fn copilot_rate_limited_routes_retry_after_through_app_error() {
        let err = AppError::Provider(ProviderError::Copilot(CopilotError::RateLimited {
            retry_after_secs: Some(42),
        }));
        assert_eq!(err.rate_limit_retry_after_secs(), Some(42));
        assert!(err.is_transient());
        assert!(!err.requires_user_action());
    }

    #[test]
    fn grok_rate_limit_routes_retry_after_through_app_error() {
        let err = AppError::Provider(ProviderError::Grok(GrokError::RateLimited {
            retry_after_secs: Some(42),
        }));
        assert_eq!(err.rate_limit_retry_after_secs(), Some(42));
        assert!(err.is_transient());
        assert!(!err.requires_user_action());
    }

    #[test]
    fn grok_refresh_auth_failures_require_user_action() {
        for status in [400, 401, 403] {
            let err =
                AppError::Provider(ProviderError::Grok(GrokError::TokenRefreshHttp { status }));
            assert!(err.requires_user_action());
            assert!(!err.is_transient());
        }
    }

    #[test]
    fn grok_refresh_rate_limit_and_server_errors_are_transient() {
        for status in [429, 500, 503] {
            let err =
                AppError::Provider(ProviderError::Grok(GrokError::TokenRefreshHttp { status }));
            assert!(!err.requires_user_action());
            assert!(err.is_transient());
        }
    }

    #[test]
    fn grok_unauthorized_and_credentials_missing_require_user_action() {
        assert!(AppError::from(GrokError::Unauthorized).requires_user_action());
        assert!(AppError::from(GrokError::CredentialsMissing).requires_user_action());
        assert!(AppError::from(GrokError::RefreshUnavailable).requires_user_action());
    }
}
