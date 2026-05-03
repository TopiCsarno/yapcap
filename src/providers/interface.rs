// SPDX-License-Identifier: MPL-2.0

use crate::config::{
    Config, ManagedClaudeAccountConfig, ManagedCodexAccountConfig, ManagedCursorAccountConfig,
};
use crate::error::AppError;
use crate::model::{AppState, ProviderAccountRuntimeState, ProviderId, UsageSnapshot};
use std::future::Future;
use std::pin::Pin;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderCapabilities {
    pub supports_delete: bool,
    pub supports_reauthentication: bool,
    pub supports_background_status_refresh: bool,
    pub requires_auth_prompt_on_auth_failure: bool,
}

impl ProviderCapabilities {
    pub const fn action_support(self) -> ProviderAccountActionSupport {
        ProviderAccountActionSupport {
            can_delete: self.supports_delete,
            can_reauthenticate: self.supports_reauthentication,
            supports_background_status_refresh: self.supports_background_status_refresh,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderAccountActionSupport {
    pub can_delete: bool,
    pub can_reauthenticate: bool,
    pub supports_background_status_refresh: bool,
}

#[derive(Debug, Clone)]
pub struct ProviderAccountDescriptor {
    pub provider: ProviderId,
    pub account_id: String,
    pub label: String,
    pub capabilities: ProviderCapabilities,
    pub handle: ProviderAccountHandle,
}

impl ProviderAccountDescriptor {
    #[must_use]
    pub fn action_support(&self) -> ProviderAccountActionSupport {
        self.capabilities.action_support()
    }
}

#[derive(Debug, Clone)]
pub enum ProviderAccountHandle {
    Codex(ManagedCodexAccountConfig),
    Claude(ManagedClaudeAccountConfig),
    Cursor(ManagedCursorAccountConfig),
}

pub trait ProviderAdapter: Send + Sync {
    fn id(&self) -> ProviderId;

    fn capabilities(&self) -> ProviderCapabilities;

    fn discover_accounts(&self, config: &Config) -> Vec<ProviderAccountDescriptor>;

    fn delete_account(&self, account_id: &str, config: &mut Config) -> bool;

    fn reconcile_provider_accounts(&self, config: &Config, state: &mut AppState);

    fn fetch_account<'a>(
        &self,
        handle: &'a ProviderAccountHandle,
        client: &'a reqwest::Client,
    ) -> BoxFuture<'a, crate::error::Result<UsageSnapshot, AppError>>;

    fn refresh_account_statuses(
        &self,
        _config: Config,
        _previous_accounts: Vec<ProviderAccountRuntimeState>,
    ) -> BoxFuture<'static, Vec<ProviderAccountRuntimeState>> {
        Box::pin(async { Vec::new() })
    }
}
