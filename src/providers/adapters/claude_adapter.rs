// SPDX-License-Identifier: MPL-2.0

use super::{claude_system_active_account_id, reconcile_provider_account_descriptors};
use crate::config::{Config, managed_claude_account_dir};
use crate::error::AppError;
use crate::model::{AppState, ProviderId, UsageSnapshot};
use crate::providers::claude;
use crate::providers::interface::{
    BoxFuture, ProviderAccountDescriptor, ProviderAccountHandle, ProviderAdapter,
    ProviderCapabilities,
};

pub(super) struct ClaudeAdapter;

impl ProviderAdapter for ClaudeAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::Claude
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_delete: true,
            supports_reauthentication: true,
            supports_background_status_refresh: false,
            requires_auth_prompt_on_auth_failure: false,
        }
    }

    fn discover_accounts(&self, config: &Config) -> Vec<ProviderAccountDescriptor> {
        let capabilities = self.capabilities();
        claude::discover_accounts(config)
            .into_iter()
            .filter_map(|account| {
                config
                    .claude_managed_accounts
                    .iter()
                    .find(|managed| managed.id == account.id)
                    .cloned()
                    .map(|managed| ProviderAccountDescriptor {
                        provider: self.id(),
                        account_id: account.id,
                        label: account.label,
                        capabilities,
                        handle: ProviderAccountHandle::Claude(managed),
                    })
            })
            .collect()
    }

    fn delete_account(&self, account_id: &str, config: &mut Config) -> bool {
        if !config
            .claude_managed_accounts
            .iter()
            .any(|a| a.id == account_id)
        {
            return false;
        }
        claude::remove_managed_config_dir(&managed_claude_account_dir(account_id));
        config
            .claude_managed_accounts
            .retain(|a| a.id != account_id);
        config
            .selected_claude_account_ids
            .retain(|id| id != account_id);
        true
    }

    fn reconcile_provider_accounts(&self, config: &Config, state: &mut AppState) {
        let accounts = self.discover_accounts(config);
        reconcile_provider_account_descriptors(self.id(), config, state, &accounts);
        if let Some(provider_state) = state.provider_mut(ProviderId::Claude) {
            provider_state.system_active_account_id =
                claude_system_active_account_id(&config.claude_managed_accounts);
        }
    }

    fn fetch_account<'a>(
        &self,
        handle: &'a ProviderAccountHandle,
        client: &'a reqwest::Client,
    ) -> BoxFuture<'a, crate::error::Result<UsageSnapshot, AppError>> {
        Box::pin(async move {
            match handle {
                ProviderAccountHandle::Claude(account) => {
                    claude::fetch(client, &account.id, managed_claude_account_dir(&account.id))
                        .await
                        .map_err(AppError::from)
                }
                _ => unreachable!(),
            }
        })
    }
}
