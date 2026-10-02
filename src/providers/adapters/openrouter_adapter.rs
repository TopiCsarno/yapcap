// SPDX-License-Identifier: MPL-2.0

use super::{openrouter_system_active_account_id, reconcile_provider_account_descriptors};
use crate::account_storage::ProviderAccountStorage;
use crate::config::{Config, paths};
use crate::error::AppError;
use crate::model::{AppState, ProviderId, UsageSnapshot};
use crate::providers::interface::{
    BoxFuture, ProviderAccountAction, ProviderAccountDescriptor, ProviderAccountHandle,
    ProviderAdapter, ProviderCapabilities, ProviderLoginKind,
};

pub(super) struct OpenRouterAdapter;

impl ProviderAdapter for OpenRouterAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::OpenRouter
    }

    fn login_kind(&self) -> ProviderLoginKind {
        ProviderLoginKind::OpenRouter
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_background_status_refresh: false,
            requires_auth_prompt_on_auth_failure: false,
        }
    }

    fn selection_required_message(&self) -> Option<String> {
        Some(crate::fl!("openrouter-account-select-required"))
    }

    fn discover_accounts(&self, config: &Config) -> Vec<ProviderAccountDescriptor> {
        crate::providers::openrouter::account::discover_accounts(config)
            .into_iter()
            .filter_map(|account| {
                config
                    .openrouter_managed_accounts
                    .iter()
                    .find(|managed| managed.id == account.id)
                    .cloned()
                    .map(|managed| ProviderAccountDescriptor {
                        provider: self.id(),
                        account_id: account.id,
                        label: account.label,
                        actions: vec![
                            ProviderAccountAction::Delete,
                            ProviderAccountAction::Reauthenticate,
                        ],
                        handle: ProviderAccountHandle::OpenRouter(managed),
                    })
            })
            .collect()
    }

    fn sync_managed_accounts(&self, config: &mut Config) -> bool {
        crate::providers::openrouter::sync_managed_accounts(config)
    }

    fn delete_account(&self, account_id: &str, config: &mut Config) -> bool {
        if !config
            .openrouter_managed_accounts
            .iter()
            .any(|account| account.id == account_id)
        {
            return false;
        }
        if ProviderAccountStorage::new(paths().openrouter_accounts_dir)
            .delete_account(account_id)
            .is_err()
        {
            return false;
        }
        config
            .openrouter_managed_accounts
            .retain(|account| account.id != account_id);
        config
            .selected_openrouter_account_ids
            .retain(|id| id != account_id);
        true
    }

    fn reconcile_provider_accounts(&self, config: &Config, state: &mut AppState) {
        let accounts = self.discover_accounts(config);
        reconcile_provider_account_descriptors(self.id(), config, state, &accounts);
        if let Some(provider_state) = state.provider_mut(ProviderId::OpenRouter) {
            provider_state.system_active_account_id = self.system_active_account_id(config);
        }
    }

    fn system_active_account_id(&self, config: &Config) -> Option<String> {
        openrouter_system_active_account_id(&config.openrouter_managed_accounts)
    }

    fn fetch_account<'a>(
        &self,
        handle: &'a ProviderAccountHandle,
        client: &'a reqwest::Client,
    ) -> BoxFuture<'a, crate::error::Result<UsageSnapshot, AppError>> {
        let provider = self.id();
        Box::pin(async move {
            match handle {
                ProviderAccountHandle::OpenRouter(account) => {
                    crate::providers::openrouter::fetch(client, account)
                        .await
                        .map_err(AppError::from)
                }
                _ => Err(AppError::InvalidAccountHandle { provider }),
            }
        })
    }
}
