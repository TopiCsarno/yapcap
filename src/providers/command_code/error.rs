// SPDX-License-Identifier: MPL-2.0

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CommandCodeError {
    #[error("Command Code login required")]
    LoginRequired,
    #[error("Command Code denied access to account usage")]
    AccessDenied,
    #[error("Command Code usage request failed")]
    Request(#[source] reqwest::Error),
    #[error("Command Code usage endpoint returned HTTP {status}")]
    Http { status: u16 },
    #[error("Command Code usage request timed out")]
    Timeout,
    #[error("Command Code usage response is unavailable or has changed")]
    InvalidResponse,
    #[error("Command Code credentials could not be read")]
    Storage,
    #[error("Rate limited by Command Code — will retry automatically")]
    RateLimited { retry_after_secs: Option<u64> },
}

impl CommandCodeError {
    pub fn requires_user_action(&self) -> bool {
        matches!(self, Self::LoginRequired | Self::Storage)
    }

    pub fn is_network_unavailable(&self) -> bool {
        matches!(self, Self::Request(error) if error.is_connect() || error.is_timeout())
    }

    pub fn is_transient(&self) -> bool {
        self.is_network_unavailable()
            || matches!(
                self,
                Self::Timeout | Self::RateLimited { .. } | Self::Http { status: 500..=599 }
            )
    }

    pub fn rate_limit_retry_after_secs(&self) -> Option<u64> {
        match self {
            Self::RateLimited { retry_after_secs } => *retry_after_secs,
            _ => None,
        }
    }
}
