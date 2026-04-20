// SPDX-License-Identifier: MPL-2.0

use std::num::ParseFloatError;
use std::path::PathBuf;
use std::time::Duration;

use thiserror::Error;

pub type Result<T, E = ConfigError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Browser(#[from] BrowserError),
    #[error(transparent)]
    Cache(#[from] CacheError),
    #[error(transparent)]
    Config(#[from] ConfigError),
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
    pub fn requires_user_action(&self) -> bool {
        match self {
            Self::Auth(_) => true,
            Self::Browser(error) => error.requires_user_action(),
            Self::Provider(error) => error.requires_user_action(),
            Self::Cache(_) | Self::Config(_) | Self::Logging(_) => false,
        }
    }

    pub fn is_transient(&self) -> bool {
        match self {
            Self::Provider(error) => error.is_transient(),
            _ => false,
        }
    }
}

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("could not resolve CODEX_HOME or ~/.codex")]
    ResolveCodexHome,
    #[error("could not resolve CLAUDE_HOME or ~/.claude")]
    ResolveClaudeHome,
    #[error("failed to read codex auth file {path}")]
    ReadCodexAuthFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse codex auth.json")]
    ParseCodexAuthJson(#[source] serde_json::Error),
    #[error("failed to read claude credentials {path}")]
    ReadClaudeCredentials {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse claude credentials")]
    ParseClaudeCredentials(#[source] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum BrowserError {
    #[error("failed to connect to secret service")]
    ConnectSecretService(#[source] secret_service::Error),
    #[error("failed to search secret service")]
    SearchSecretService(#[source] secret_service::Error),
    #[error("no matching keyring item found for browser safe storage")]
    MissingKeyringItem,
    #[error("failed to read browser safe storage secret")]
    ReadBrowserSecret(#[source] secret_service::Error),
    #[error("browser safe storage secret is not valid UTF-8")]
    BrowserSecretNotUtf8(#[source] std::string::FromUtf8Error),
    #[error("cookie database not found at {path}")]
    CookieDatabaseNotFound { path: PathBuf },
    #[error("failed to create temp cookie db")]
    CreateTempCookieDb(#[source] std::io::Error),
    #[error("failed to copy {path}")]
    CopyCookieDb {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to open copied cookie db")]
    OpenCookieDb(#[source] rusqlite::Error),
    #[error("failed to prepare cookie lookup")]
    PrepareCookieLookup(#[source] rusqlite::Error),
    #[error("cookie not found in browser db")]
    CookieNotFound(#[source] rusqlite::Error),
    #[error("encrypted cookie blob is empty")]
    EmptyCookieBlob,
    #[error("cookie blob not recognized")]
    CookieBlobNotRecognized(#[source] std::string::FromUtf8Error),
    #[error("failed to initialize AES-CBC decryptor: {0}")]
    InitAesCbc(String),
    #[error("failed to decrypt chromium cookie: {0}")]
    DecryptCookieCbc(String),
    #[error("failed to initialize AES-GCM decryptor: {0}")]
    InitAesGcm(String),
    #[error("failed to decrypt chromium cookie with AES-GCM: {0}")]
    DecryptCookieGcm(String),
    #[error("decrypted cookie is not valid UTF-8, even after stripping domain hash")]
    CookieNotUtf8AfterPrefix(#[source] std::string::FromUtf8Error),
    #[error("decrypted cookie is not valid UTF-8 ({len} bytes — likely wrong decryption key)")]
    CookieNotUtf8 {
        len: usize,
        #[source]
        source: std::string::FromUtf8Error,
    },
    #[error("failed to decrypt chromium cookie: {0}")]
    CookieDecryptFailed(String),
}

impl BrowserError {
    #[must_use]
    pub fn requires_user_action(&self) -> bool {
        matches!(
            self,
            Self::MissingKeyringItem
                | Self::CookieDatabaseNotFound { .. }
                | Self::CookieNotFound(_)
        )
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
pub enum ConfigError {
    #[error("failed to read config file {path}")]
    ReadConfigFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("missing home directory")]
    MissingHomeDir,
    #[error("could not find Firefox profile with cookies.sqlite")]
    FirefoxProfileNotFound,
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
    pub fn requires_user_action(&self) -> bool {
        match self {
            Self::Codex(error) => error.requires_user_action(),
            Self::Claude(error) => error.requires_user_action(),
            Self::Cursor(error) => error.requires_user_action(),
        }
    }

    pub fn is_transient(&self) -> bool {
        match self {
            Self::Claude(error) => error.is_transient(),
            Self::Codex(_) | Self::Cursor(_) => false,
        }
    }
}

#[derive(Debug, Error)]
pub enum CodexError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error("invalid codex bearer header")]
    InvalidBearerHeader(#[source] reqwest::header::InvalidHeaderValue),
    #[error("invalid codex account id header")]
    InvalidAccountIdHeader(#[source] reqwest::header::InvalidHeaderValue),
    #[error("codex usage request failed")]
    UsageRequest(#[source] reqwest::Error),
    #[error("Codex login required")]
    Unauthorized,
    #[error("codex usage endpoint returned error")]
    UsageEndpoint(#[source] reqwest::Error),
    #[error("failed to decode codex usage response")]
    DecodeUsage(#[source] reqwest::Error),
    #[error("Codex response had no usage windows")]
    NoUsageData,
    #[error("failed to parse codex credit balance {balance}")]
    InvalidCreditBalance {
        balance: String,
        #[source]
        source: ParseFloatError,
    },
    #[error("codex CLI binary not found")]
    CliUnavailable,
    #[error("failed to spawn codex CLI")]
    CliCommand(#[source] std::io::Error),
    #[error("failed to communicate with codex CLI")]
    CliIo(#[source] std::io::Error),
    #[error("codex CLI timed out after {timeout:?}")]
    CliTimeout { timeout: Duration },
    #[error("failed to parse codex CLI output")]
    CliParse,
    #[error("codex RPC protocol mismatch or incompatible version")]
    RpcProtocol,
}

impl CodexError {
    pub fn requires_user_action(&self) -> bool {
        matches!(
            self,
            Self::Auth(_) | Self::Unauthorized | Self::CliUnavailable
        )
    }
}

#[derive(Debug, Error)]
pub enum ClaudeError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error("Claude token missing user:profile scope")]
    MissingProfileScope,
    #[error("invalid claude bearer header")]
    InvalidBearerHeader(#[source] reqwest::header::InvalidHeaderValue),
    #[error("claude usage request failed")]
    UsageRequest(#[source] reqwest::Error),
    #[error("Claude token unauthorized or expired")]
    Unauthorized,
    #[error("claude CLI binary not found")]
    CliUnavailable,
    #[error("failed to spawn claude CLI")]
    CliCommand(#[source] std::io::Error),
    #[error("failed to communicate with claude CLI")]
    CliIo(#[source] std::io::Error),
    #[error("claude CLI timed out after {timeout:?}")]
    CliTimeout { timeout: Duration },
    #[error("claude auth status failed with exit status {status}")]
    CliStatusFailed { status: String },
    #[error("claude usage endpoint rate limited (429)")]
    RateLimited,
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
    pub fn requires_user_action(&self) -> bool {
        matches!(
            self,
            Self::Auth(_)
                | Self::MissingProfileScope
                | Self::Unauthorized
                | Self::CliUnavailable
                | Self::CliCommand(_)
                | Self::CliIo(_)
                | Self::CliTimeout { .. }
                | Self::CliStatusFailed { .. }
        )
    }

    pub fn is_transient(&self) -> bool {
        matches!(self, Self::RateLimited)
    }
}

#[derive(Debug, Error)]
pub enum CursorError {
    #[error(transparent)]
    Browser(#[from] BrowserError),
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
}

impl CursorError {
    pub fn requires_user_action(&self) -> bool {
        matches!(self, Self::Browser(error) if error.requires_user_action())
            || matches!(self, Self::Unauthorized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_error_requires_user_action() {
        let err = AppError::Auth(AuthError::ResolveCodexHome);
        assert!(err.requires_user_action());
        assert!(!err.is_transient());
    }

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
        let err = AppError::Provider(ProviderError::Claude(ClaudeError::RateLimited));
        assert!(!err.requires_user_action());
        assert!(err.is_transient());
    }

    #[test]
    fn codex_unauthorized_requires_user_action() {
        let err = AppError::Provider(ProviderError::Codex(CodexError::Unauthorized));
        assert!(err.requires_user_action());
        assert!(!err.is_transient());
    }

    #[test]
    fn cursor_unauthorized_requires_user_action() {
        let err = AppError::Provider(ProviderError::Cursor(CursorError::Unauthorized));
        assert!(err.requires_user_action());
    }

    #[test]
    fn browser_missing_keyring_requires_user_action() {
        let err = BrowserError::MissingKeyringItem;
        assert!(err.requires_user_action());
    }

    #[test]
    fn browser_decrypt_error_does_not_require_user_action() {
        let err = BrowserError::InitAesCbc("bad key".into());
        assert!(!err.requires_user_action());
    }

    #[test]
    fn cursor_browser_auth_error_propagates_requires_user_action() {
        let err = CursorError::Browser(BrowserError::CookieDatabaseNotFound {
            path: PathBuf::from("/no/such/path"),
        });
        assert!(err.requires_user_action());
    }

    #[test]
    fn cursor_browser_decrypt_error_does_not_require_user_action() {
        let err = CursorError::Browser(BrowserError::DecryptCookieCbc("fail".into()));
        assert!(!err.requires_user_action());
    }

    #[test]
    fn codex_cli_errors_do_not_require_user_action_by_default() {
        let err = CodexError::CliParse;
        assert!(!err.requires_user_action());
    }

    #[test]
    fn claude_cli_unavailable_requires_user_action() {
        let err = ClaudeError::CliUnavailable;
        assert!(err.requires_user_action());
        assert!(!err.is_transient());
    }
}
