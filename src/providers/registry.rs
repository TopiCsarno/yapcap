// SPDX-License-Identifier: MPL-2.0

use crate::config::{Config, ProviderVisibilityMode};
use crate::model::{AppState, ProviderAccountRuntimeState, ProviderId, UsageSnapshot};
use crate::providers::adapters::adapter;
use crate::providers::interface::{
    ProviderAccountDescriptor, ProviderAccountHandle, ProviderCapabilities,
};
use crate::providers::{claude, codex, cursor, gemini};

#[cfg(test)]
mod tests;

pub fn capabilities(provider: ProviderId) -> ProviderCapabilities {
    adapter(provider).capabilities()
}

pub fn startup_sync(config: &mut Config) -> bool {
    let codex_changed = codex::sync_managed_accounts(config);
    let cursor_changed = cursor::sync_managed_accounts(config);
    let claude_changed = claude::sync_managed_account_dirs(config);
    let gemini_changed = gemini::sync_managed_accounts(config);
    codex_changed | cursor_changed | claude_changed | gemini_changed
}

pub fn initialize_provider_visibility(config: &mut Config, providers: &[ProviderId]) -> bool {
    if config.provider_visibility_mode != ProviderVisibilityMode::AutoInitPending {
        return false;
    }

    let mut changed = false;
    for &provider in providers {
        changed |= config.set_provider_enabled(provider, true);
    }
    changed
}

pub fn finalize_provider_visibility_initialization(config: &mut Config) -> bool {
    if config.provider_visibility_mode != ProviderVisibilityMode::AutoInitPending {
        return false;
    }
    config.provider_visibility_mode = ProviderVisibilityMode::UserManaged;
    true
}

pub fn discover_accounts(provider: ProviderId, config: &Config) -> Vec<ProviderAccountDescriptor> {
    adapter(provider).discover_accounts(config)
}

pub fn toggle_account_selection(provider: ProviderId, config: &mut Config, account_id: &str) {
    if !config.show_all_accounts(provider) {
        let ids = config.selected_account_ids_mut(provider);
        if ids.as_slice() != [account_id] {
            ids.clear();
            ids.push(account_id.to_string());
        }
        return;
    }
    let ids = config.selected_account_ids_mut(provider);
    if let Some(pos) = ids.iter().position(|id| id == account_id) {
        ids.remove(pos);
    } else {
        ids.push(account_id.to_string());
    }
}

pub fn sync_selected_ids_with_discoveries(config: &mut Config, provider: ProviderId) {
    let valid: Vec<String> = discover_accounts(provider, config)
        .into_iter()
        .map(|a| a.account_id)
        .collect();
    let ids = config.selected_account_ids_mut(provider);
    ids.retain(|id| valid.contains(id));
    if ids.is_empty() && valid.len() == 1 {
        ids.push(valid.into_iter().next().unwrap());
    }
}

pub async fn fetch_handle(
    handle: &ProviderAccountHandle,
    client: &reqwest::Client,
) -> crate::error::Result<UsageSnapshot, crate::error::AppError> {
    let provider = match handle {
        ProviderAccountHandle::Codex(_) => ProviderId::Codex,
        ProviderAccountHandle::Claude(_) => ProviderId::Claude,
        ProviderAccountHandle::Cursor(_) => ProviderId::Cursor,
        ProviderAccountHandle::Gemini(_) => ProviderId::Gemini,
    };
    adapter(provider).fetch_account(handle, client).await
}

pub fn supports_background_status_refresh(provider: ProviderId) -> bool {
    capabilities(provider).supports_background_status_refresh
}

pub fn auth_error_requires_reauth_prompt(provider: ProviderId) -> bool {
    capabilities(provider).requires_auth_prompt_on_auth_failure
}

pub fn delete_account(provider: ProviderId, account_id: &str, config: &mut Config) -> bool {
    adapter(provider).delete_account(account_id, config)
}

pub fn reconcile_provider_accounts(provider: ProviderId, config: &Config, state: &mut AppState) {
    adapter(provider).reconcile_provider_accounts(config, state);
}

pub(crate) fn codex_system_active_account_id(
    managed_accounts: &[crate::config::ManagedCodexAccountConfig],
) -> Option<String> {
    crate::providers::adapters::codex_system_active_account_id(managed_accounts)
}

pub(crate) fn claude_system_active_account_id(
    managed_accounts: &[crate::config::ManagedClaudeAccountConfig],
) -> Option<String> {
    crate::providers::adapters::claude_system_active_account_id(managed_accounts)
}

pub(crate) fn gemini_system_active_account_id(
    managed_accounts: &[crate::config::ManagedGeminiAccountConfig],
) -> Option<String> {
    crate::providers::adapters::gemini_system_active_account_id(managed_accounts)
}

pub async fn refresh_account_statuses(
    provider: ProviderId,
    config: Config,
    previous_accounts: Vec<ProviderAccountRuntimeState>,
) -> Vec<ProviderAccountRuntimeState> {
    adapter(provider)
        .refresh_account_statuses(config, previous_accounts)
        .await
}
