// SPDX-License-Identifier: MPL-2.0

use crate::config::Config;
use crate::model::{AppState, ProviderAccountRuntimeState, ProviderId, UsageSnapshot};
use crate::providers::adapters::adapter;
use crate::providers::interface::{
    ProviderAccountAddAction, ProviderAccountDescriptor, ProviderAccountFacts,
    ProviderCapabilities, ProviderLoginKind,
};

#[cfg(test)]
mod account_facts_tests;
#[cfg(test)]
mod tests;

pub fn capabilities(provider: ProviderId) -> ProviderCapabilities {
    adapter(provider).capabilities()
}

pub fn login_kind(provider: ProviderId) -> ProviderLoginKind {
    adapter(provider).login_kind()
}

pub fn supports_opencode_import(provider: ProviderId) -> bool {
    adapter(provider).supports_opencode_import()
}

pub fn account_add_action(provider: ProviderId) -> ProviderAccountAddAction {
    adapter(provider).account_add_action()
}

pub fn selection_required_message(provider: ProviderId) -> Option<String> {
    adapter(provider).selection_required_message()
}

pub fn startup_sync(config: &mut Config) -> bool {
    let mut changed = false;
    for provider in ProviderId::ALL {
        changed |= adapter(provider).sync_managed_accounts(config);
    }
    changed
}

pub fn discover_accounts(provider: ProviderId, config: &Config) -> Vec<ProviderAccountDescriptor> {
    adapter(provider).discover_accounts(config)
}

pub fn prepare_account_facts(
    provider: ProviderId,
    config: &Config,
    state: &AppState,
) -> Vec<ProviderAccountFacts> {
    let provider_adapter = adapter(provider);
    let descriptors = provider_adapter.discover_accounts(config);
    let tooltip = provider_adapter.reauthenticate_tooltip();
    state
        .accounts_for(provider)
        .into_iter()
        .map(|account| {
            descriptors
                .iter()
                .find(|descriptor| descriptor.account_id == account.account_id)
                .map_or_else(
                    || {
                        ProviderAccountFacts::without_descriptor(
                            account,
                            provider_adapter.account_status(account),
                            tooltip.clone(),
                        )
                    },
                    |descriptor| provider_adapter.account_facts(descriptor, account),
                )
        })
        .collect()
}

pub fn toggle_account_selection(provider: ProviderId, config: &mut Config, account_id: &str) {
    let ids = config.selected_account_ids_mut(provider);
    if ids.as_slice() != [account_id] {
        ids.clear();
        ids.push(account_id.to_string());
    }
}

pub fn toggle_account_panel_flag(provider: ProviderId, config: &mut Config, account_id: &str) {
    let ids = config.panel_account_ids_mut(provider);
    if ids.iter().any(|id| id == account_id) {
        ids.retain(|id| id != account_id);
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

pub fn sync_panel_ids_with_discoveries(config: &mut Config, provider: ProviderId) {
    let valid: Vec<String> = discover_accounts(provider, config)
        .into_iter()
        .map(|a| a.account_id)
        .collect();
    let ids = config.panel_account_ids_mut(provider);
    ids.retain(|id| valid.contains(id));
}

pub async fn fetch_account(
    account: &ProviderAccountDescriptor,
    client: &reqwest::Client,
) -> crate::error::Result<UsageSnapshot, crate::error::AppError> {
    adapter(account.provider)
        .fetch_account(&account.handle, client)
        .await
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

pub fn system_active_account_id(provider: ProviderId, config: &Config) -> Option<String> {
    adapter(provider).system_active_account_id(config)
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
