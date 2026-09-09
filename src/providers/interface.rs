// SPDX-License-Identifier: MPL-2.0

use crate::config::{
    Config, ManagedAntigravityAccountConfig, ManagedClaudeAccountConfig, ManagedCodexAccountConfig,
    ManagedCopilotAccountConfig, ManagedCursorAccountConfig, ManagedGeminiAccountConfig,
    ManagedGrokAccountConfig, ManagedKimiAccountConfig, ManagedMinimaxAccountConfig,
    ManagedOpenCodeGoAccountConfig,
};
use crate::error::AppError;
use crate::model::{AppState, AuthState, ProviderAccountRuntimeState, ProviderId, UsageSnapshot};
use std::future::Future;
use std::pin::Pin;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderCapabilities {
    pub supports_background_status_refresh: bool,
    pub requires_auth_prompt_on_auth_failure: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderAccountAction {
    Delete,
    Reauthenticate,
    RestoreFromOpenCode,
    RestoreFromGrok,
    Rescan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderAccountAddAction {
    Login,
    Scan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderAccountStatusKind {
    Warning,
    Neutral,
    Destructive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderAccountStatus {
    pub kind: ProviderAccountStatusKind,
    pub badge_text: String,
    pub tooltip_text: String,
    pub reauth_eligible: bool,
    pub style_as_action_required: bool,
}

#[derive(Debug, Clone)]
pub struct ProviderAccountFacts {
    pub account_id: String,
    pub label: String,
    pub actions: Vec<ProviderAccountAction>,
    pub status: Option<ProviderAccountStatus>,
    pub reauthenticate_tooltip: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderLoginKind {
    Codex,
    Claude,
    Cursor,
    Gemini,
    Copilot,
    Minimax,
    Kimi,
    Antigravity,
    OpenCodeGo,
    Grok,
}

#[derive(Debug, Clone)]
pub struct ProviderAccountDescriptor {
    pub provider: ProviderId,
    pub account_id: String,
    pub label: String,
    pub actions: Vec<ProviderAccountAction>,
    pub handle: ProviderAccountHandle,
}

impl ProviderAccountFacts {
    #[must_use]
    pub fn supports_action(&self, action: ProviderAccountAction) -> bool {
        self.actions.contains(&action)
    }

    fn from_descriptor(
        descriptor: &ProviderAccountDescriptor,
        label: String,
        status: Option<ProviderAccountStatus>,
        reauthenticate_tooltip: String,
    ) -> Self {
        Self {
            account_id: descriptor.account_id.clone(),
            label,
            actions: descriptor.actions.clone(),
            status,
            reauthenticate_tooltip,
        }
    }

    pub(crate) fn without_descriptor(
        account: &ProviderAccountRuntimeState,
        status: Option<ProviderAccountStatus>,
        reauthenticate_tooltip: String,
    ) -> Self {
        Self {
            account_id: account.account_id.clone(),
            label: account.label.clone(),
            actions: Vec::new(),
            status,
            reauthenticate_tooltip,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ProviderAccountHandle {
    Codex(ManagedCodexAccountConfig),
    Claude(ManagedClaudeAccountConfig),
    Cursor(ManagedCursorAccountConfig),
    Gemini(ManagedGeminiAccountConfig),
    Copilot(ManagedCopilotAccountConfig),
    Minimax(ManagedMinimaxAccountConfig),
    Kimi(ManagedKimiAccountConfig),
    Antigravity(ManagedAntigravityAccountConfig),
    OpenCodeGo(ManagedOpenCodeGoAccountConfig),
    Grok(ManagedGrokAccountConfig),
}

pub trait ProviderAdapter: Send + Sync {
    fn id(&self) -> ProviderId;

    fn login_kind(&self) -> ProviderLoginKind;

    fn supports_opencode_import(&self) -> bool {
        false
    }

    fn account_add_action(&self) -> ProviderAccountAddAction {
        ProviderAccountAddAction::Login
    }

    fn selection_required_message(&self) -> Option<String> {
        None
    }

    fn reauthenticate_tooltip(&self) -> String {
        format!("Re-authenticate this {} account", self.id().label())
    }

    fn account_label(
        &self,
        descriptor: &ProviderAccountDescriptor,
        _account: &ProviderAccountRuntimeState,
    ) -> String {
        descriptor.label.clone()
    }

    fn account_status(
        &self,
        account: &ProviderAccountRuntimeState,
    ) -> Option<ProviderAccountStatus> {
        generic_account_status(account)
    }

    fn account_facts(
        &self,
        descriptor: &ProviderAccountDescriptor,
        account: &ProviderAccountRuntimeState,
    ) -> ProviderAccountFacts {
        ProviderAccountFacts::from_descriptor(
            descriptor,
            self.account_label(descriptor, account),
            self.account_status(account),
            self.reauthenticate_tooltip(),
        )
    }

    fn capabilities(&self) -> ProviderCapabilities;

    fn discover_accounts(&self, config: &Config) -> Vec<ProviderAccountDescriptor>;

    fn sync_managed_accounts(&self, config: &mut Config) -> bool;

    fn delete_account(&self, account_id: &str, config: &mut Config) -> bool;

    fn reconcile_provider_accounts(&self, config: &Config, state: &mut AppState);

    fn system_active_account_id(&self, _config: &Config) -> Option<String> {
        None
    }

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

fn generic_account_status(account: &ProviderAccountRuntimeState) -> Option<ProviderAccountStatus> {
    (account.auth_state == AuthState::ActionRequired).then(|| ProviderAccountStatus {
        kind: ProviderAccountStatusKind::Warning,
        badge_text: crate::fl!("badge-login-required"),
        tooltip_text: crate::fl!("badge-login-required-tooltip"),
        reauth_eligible: true,
        style_as_action_required: true,
    })
}
