// SPDX-License-Identifier: MPL-2.0

use super::{grok_system_active_account_id, reconcile_provider_account_descriptors};
use crate::account_storage::ProviderAccountStorage;
use crate::config::{Config, ManagedGrokAccountConfig, managed_grok_account_dir, paths};
use crate::error::AppError;
use crate::model::{AppState, ProviderId, UsageSnapshot};
use crate::providers::grok;
use crate::providers::interface::{
    BoxFuture, ProviderAccountAction, ProviderAccountDescriptor, ProviderAccountHandle,
    ProviderAdapter, ProviderCapabilities, ProviderLoginKind,
};

pub(super) struct GrokAdapter;

impl ProviderAdapter for GrokAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::Grok
    }

    fn login_kind(&self) -> ProviderLoginKind {
        ProviderLoginKind::Grok
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_background_status_refresh: false,
            requires_auth_prompt_on_auth_failure: false,
        }
    }

    fn discover_accounts(&self, config: &Config) -> Vec<ProviderAccountDescriptor> {
        let host_auth_available = grok_host_auth_available();
        grok::account::discover_accounts(config)
            .into_iter()
            .map(|account| grok_account_descriptor(&account, host_auth_available))
            .collect()
    }

    fn sync_managed_accounts(&self, config: &mut Config) -> bool {
        grok::account::sync_managed_account_dirs(config)
    }

    fn delete_account(&self, account_id: &str, config: &mut Config) -> bool {
        if !config
            .grok_managed_accounts
            .iter()
            .any(|a| a.id == account_id)
        {
            return false;
        }
        let storage = ProviderAccountStorage::new(paths().grok_accounts_dir);
        if let Err(error) = storage.delete_account(account_id) {
            tracing::warn!(account_id, error = %error, "failed to delete grok account");
        }
        config.grok_managed_accounts.retain(|a| a.id != account_id);
        config
            .selected_grok_account_ids
            .retain(|id| id != account_id);
        true
    }

    fn reconcile_provider_accounts(&self, config: &Config, state: &mut AppState) {
        let accounts = self.discover_accounts(config);
        reconcile_provider_account_descriptors(self.id(), config, state, &accounts);
        if let Some(provider_state) = state.provider_mut(ProviderId::Grok) {
            provider_state.system_active_account_id = self.system_active_account_id(config);
        }
    }

    fn system_active_account_id(&self, config: &Config) -> Option<String> {
        grok_system_active_account_id(&config.grok_managed_accounts)
    }

    fn fetch_account<'a>(
        &self,
        handle: &'a ProviderAccountHandle,
        client: &'a reqwest::Client,
    ) -> BoxFuture<'a, crate::error::Result<UsageSnapshot, AppError>> {
        let provider = self.id();
        Box::pin(async move {
            match handle {
                ProviderAccountHandle::Grok(account) => {
                    grok::fetch(client, &account.id, managed_grok_account_dir(&account.id))
                        .await
                        .map_err(AppError::from)
                }
                _ => Err(AppError::InvalidAccountHandle { provider }),
            }
        })
    }
}

fn grok_account_descriptor(
    account: &ManagedGrokAccountConfig,
    host_auth_available: bool,
) -> ProviderAccountDescriptor {
    let mut actions = vec![
        ProviderAccountAction::Delete,
        ProviderAccountAction::Reauthenticate,
    ];
    if host_auth_available {
        actions.push(ProviderAccountAction::RestoreFromGrok);
    }
    ProviderAccountDescriptor {
        provider: ProviderId::Grok,
        account_id: account.id.clone(),
        label: account.label.clone(),
        actions,
        handle: ProviderAccountHandle::Grok(account.clone()),
    }
}

fn grok_host_auth_available() -> bool {
    grok::account::host_auth_file_path()
        .and_then(|path| grok::account::read_host_credentials(&path))
        .is_some()
}
