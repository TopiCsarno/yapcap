// SPDX-License-Identifier: MPL-2.0

use super::{gemini_system_active_account_id, reconcile_provider_account_descriptors};
use crate::account_storage::ProviderAccountStorage;
use crate::config::{Config, managed_gemini_account_dir, paths};
use crate::error::AppError;
use crate::model::{AppState, ProviderId, UsageSnapshot};
use crate::providers::gemini;
use crate::providers::interface::{
    BoxFuture, ProviderAccountDescriptor, ProviderAccountHandle, ProviderAdapter,
    ProviderCapabilities,
};

pub(super) struct GeminiAdapter;

impl ProviderAdapter for GeminiAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::Gemini
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
        config
            .gemini_managed_accounts
            .iter()
            .cloned()
            .map(|managed| ProviderAccountDescriptor {
                provider: self.id(),
                account_id: managed.id.clone(),
                label: managed.label.clone(),
                capabilities,
                handle: ProviderAccountHandle::Gemini(managed),
            })
            .collect()
    }

    fn delete_account(&self, account_id: &str, config: &mut Config) -> bool {
        if !config
            .gemini_managed_accounts
            .iter()
            .any(|a| a.id == account_id)
        {
            return false;
        }
        let storage = ProviderAccountStorage::new(paths().gemini_accounts_dir);
        if let Err(error) = storage.delete_account(account_id) {
            tracing::warn!(account_id, error = %error, "failed to delete gemini account");
        }
        config
            .gemini_managed_accounts
            .retain(|a| a.id != account_id);
        config
            .selected_gemini_account_ids
            .retain(|id| id != account_id);
        true
    }

    fn reconcile_provider_accounts(&self, config: &Config, state: &mut AppState) {
        let accounts = self.discover_accounts(config);
        reconcile_provider_account_descriptors(self.id(), config, state, &accounts);
        if let Some(provider_state) = state.provider_mut(ProviderId::Gemini) {
            provider_state.system_active_account_id =
                gemini_system_active_account_id(&config.gemini_managed_accounts);
        }
    }

    fn fetch_account<'a>(
        &self,
        handle: &'a ProviderAccountHandle,
        client: &'a reqwest::Client,
    ) -> BoxFuture<'a, crate::error::Result<UsageSnapshot, AppError>> {
        Box::pin(async move {
            match handle {
                ProviderAccountHandle::Gemini(managed) => gemini::fetch(
                    client,
                    &managed.id,
                    managed_gemini_account_dir(&managed.id),
                    managed.hd.clone(),
                )
                .await
                .map_err(AppError::from),
                _ => unreachable!(),
            }
        })
    }
}
