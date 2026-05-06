// SPDX-License-Identifier: MPL-2.0

use super::{codex_system_active_account_id, reconcile_provider_account_descriptors};
use crate::config::{Config, managed_codex_account_dir};
use crate::error::AppError;
use crate::model::{AppState, ProviderId, UsageSnapshot};
use crate::providers::adapters::remove_managed_codex_account;
use crate::providers::codex;
use crate::providers::interface::{
    BoxFuture, ProviderAccountDescriptor, ProviderAccountHandle, ProviderAdapter,
    ProviderCapabilities,
};

pub(super) struct CodexAdapter;

impl ProviderAdapter for CodexAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::Codex
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_delete: true,
            supports_reauthentication: false,
            supports_background_status_refresh: false,
            requires_auth_prompt_on_auth_failure: false,
        }
    }

    fn discover_accounts(&self, config: &Config) -> Vec<ProviderAccountDescriptor> {
        let capabilities = self.capabilities();
        codex::discover_accounts(config)
            .into_iter()
            .filter_map(|account| {
                config
                    .codex_managed_accounts
                    .iter()
                    .find(|managed| managed.id == account.id)
                    .cloned()
                    .map(|managed| ProviderAccountDescriptor {
                        provider: self.id(),
                        account_id: account.id,
                        label: account.label,
                        capabilities,
                        handle: ProviderAccountHandle::Codex(managed),
                    })
            })
            .collect()
    }

    fn delete_account(&self, account_id: &str, config: &mut Config) -> bool {
        let account = config
            .codex_managed_accounts
            .iter()
            .find(|a| a.id == account_id)
            .cloned();
        let Some(account) = account else {
            return false;
        };
        remove_managed_codex_account(&account.id);
        config.codex_managed_accounts.retain(|a| a.id != account_id);
        config
            .selected_codex_account_ids
            .retain(|id| id != account_id);
        true
    }

    fn reconcile_provider_accounts(&self, config: &Config, state: &mut AppState) {
        let accounts = self.discover_accounts(config);
        reconcile_provider_account_descriptors(self.id(), config, state, &accounts);
        if let Some(provider_state) = state.provider_mut(ProviderId::Codex) {
            provider_state.system_active_account_id =
                codex_system_active_account_id(&config.codex_managed_accounts);
        }
    }

    fn fetch_account<'a>(
        &self,
        handle: &'a ProviderAccountHandle,
        client: &'a reqwest::Client,
    ) -> BoxFuture<'a, crate::error::Result<UsageSnapshot, AppError>> {
        Box::pin(async move {
            match handle {
                ProviderAccountHandle::Codex(account) => {
                    codex::fetch(client, &account.id, managed_codex_account_dir(&account.id))
                        .await
                        .map_err(AppError::from)
                }
                _ => unreachable!(),
            }
        })
    }
}
