// SPDX-License-Identifier: MPL-2.0

use super::reconcile_provider_account_descriptors;
use crate::config::Config;
use crate::error::AppError;
use crate::model::{AppState, ProviderId, UsageSnapshot};
use crate::providers::command_code;
use crate::providers::interface::{
    BoxFuture, ProviderAccountAction, ProviderAccountDescriptor, ProviderAccountHandle,
    ProviderAdapter, ProviderCapabilities, ProviderLoginKind,
};

pub(super) struct CommandCodeAdapter;

impl ProviderAdapter for CommandCodeAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::CommandCode
    }

    fn login_kind(&self) -> ProviderLoginKind {
        ProviderLoginKind::CommandCode
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_background_status_refresh: false,
            requires_auth_prompt_on_auth_failure: false,
        }
    }

    fn discover_accounts(&self, config: &Config) -> Vec<ProviderAccountDescriptor> {
        command_code::account::descriptors(config)
            .into_iter()
            .map(|account| ProviderAccountDescriptor {
                provider: self.id(),
                account_id: account.id.clone(),
                label: account.label.clone(),
                actions: vec![
                    ProviderAccountAction::Delete,
                    ProviderAccountAction::Reauthenticate,
                ],
                handle: ProviderAccountHandle::CommandCode(account),
            })
            .collect()
    }

    fn sync_managed_accounts(&self, _config: &mut Config) -> bool {
        false
    }

    fn delete_account(&self, account_id: &str, config: &mut Config) -> bool {
        if !config
            .command_code_managed_accounts
            .iter()
            .any(|account| account.id == account_id)
            || command_code::account::storage()
                .delete_account(account_id)
                .is_err()
        {
            return false;
        }
        config
            .command_code_managed_accounts
            .retain(|account| account.id != account_id);
        config
            .selected_command_code_account_ids
            .retain(|id| id != account_id);
        true
    }

    fn reconcile_provider_accounts(&self, config: &Config, state: &mut AppState) {
        reconcile_provider_account_descriptors(
            self.id(),
            config,
            state,
            &self.discover_accounts(config),
        );
    }

    fn fetch_account<'a>(
        &self,
        handle: &'a ProviderAccountHandle,
        client: &'a reqwest::Client,
    ) -> BoxFuture<'a, Result<UsageSnapshot, AppError>> {
        Box::pin(async move {
            match handle {
                ProviderAccountHandle::CommandCode(account) => command_code::fetch(client, account)
                    .await
                    .map_err(AppError::from),
                _ => Err(AppError::InvalidAccountHandle {
                    provider: ProviderId::CommandCode,
                }),
            }
        })
    }
}
