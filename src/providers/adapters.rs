// SPDX-License-Identifier: MPL-2.0

mod claude_adapter;
mod codex_adapter;
mod cursor_adapter;
mod gemini_adapter;

use crate::account_storage::ProviderAccountStorage;
use crate::config::{Config, host_user_home_dir, paths};
use crate::model::{AccountSelectionStatus, AppState, ProviderAccountRuntimeState, ProviderId};
use crate::providers::interface::{ProviderAccountDescriptor, ProviderAdapter};
use crate::providers::{claude, codex, cursor, gemini};

pub(super) fn adapter(provider: ProviderId) -> &'static dyn ProviderAdapter {
    match provider {
        ProviderId::Codex => &CODEX_ADAPTER,
        ProviderId::Claude => &CLAUDE_ADAPTER,
        ProviderId::Cursor => &CURSOR_ADAPTER,
        ProviderId::Gemini => &GEMINI_ADAPTER,
    }
}

static CODEX_ADAPTER: codex_adapter::CodexAdapter = codex_adapter::CodexAdapter;
static CLAUDE_ADAPTER: claude_adapter::ClaudeAdapter = claude_adapter::ClaudeAdapter;
static CURSOR_ADAPTER: cursor_adapter::CursorAdapter = cursor_adapter::CursorAdapter;
static GEMINI_ADAPTER: gemini_adapter::GeminiAdapter = gemini_adapter::GeminiAdapter;

pub(super) fn reconcile_provider_account_descriptors(
    provider: ProviderId,
    config: &Config,
    state: &mut AppState,
    accounts: &[ProviderAccountDescriptor],
) {
    let valid_ids: Vec<String> = accounts.iter().map(|a| a.account_id.clone()).collect();

    let mut selected_ids: Vec<String> = config
        .selected_account_ids(provider)
        .iter()
        .filter(|id| valid_ids.contains(id))
        .cloned()
        .collect();

    if !config.show_all_accounts(provider) {
        selected_ids.truncate(1);
    }

    if selected_ids.is_empty() && valid_ids.len() == 1 {
        selected_ids = valid_ids.first().cloned().into_iter().collect();
    }

    state
        .provider_accounts
        .retain(|entry| entry.provider != provider || valid_ids.contains(&entry.account_id));

    for account in accounts {
        let mut entry = state
            .provider_accounts
            .iter()
            .find(|e| e.provider == provider && e.account_id == account.account_id)
            .cloned()
            .unwrap_or_else(|| {
                ProviderAccountRuntimeState::empty(
                    provider,
                    account.account_id.clone(),
                    account.label.clone(),
                )
            });
        entry.label.clone_from(&account.label);

        if entry.snapshot.is_none()
            && selected_ids.contains(&account.account_id)
            && accounts.len() == 1
            && let Some(snapshot) = state
                .provider(provider)
                .and_then(|p| p.legacy_display_snapshot.clone())
        {
            entry.snapshot = Some(snapshot);
        }

        state.upsert_account(entry);
    }

    if let Some(provider_state) = state.provider_mut(provider) {
        provider_state.enabled = config.provider_enabled(provider);
        provider_state.account_status = account_status(&selected_ids, accounts.len());
        provider_state.error = match provider_state.account_status {
            AccountSelectionStatus::LoginRequired => Some("Login required".to_string()),
            AccountSelectionStatus::SelectionRequired => Some("Select an account".to_string()),
            _ => provider_state.error.take(),
        };
        provider_state.active_account_id = selected_ids.first().cloned();
        provider_state.selected_account_ids = selected_ids;
    }
}

fn account_status(selected_ids: &[String], valid_count: usize) -> AccountSelectionStatus {
    if !selected_ids.is_empty() {
        AccountSelectionStatus::Ready
    } else if valid_count == 0 {
        AccountSelectionStatus::LoginRequired
    } else {
        AccountSelectionStatus::SelectionRequired
    }
}

pub(super) fn remove_managed_codex_account(account_id: &str) {
    let storage = ProviderAccountStorage::new(paths().codex_accounts_dir);
    if let Err(error) = storage.delete_account(account_id) {
        tracing::warn!(account_id, error = %error, "failed to delete codex account");
    }
}

pub(super) fn remove_managed_cursor_account(account_id: &str) {
    let storage = ProviderAccountStorage::new(paths().cursor_accounts_dir);
    if let Err(error) = storage.delete_account(account_id) {
        tracing::warn!(account_id, error = %error, "failed to delete cursor account");
    }
}

pub(super) fn cursor_system_active_account_id(
    managed_accounts: &[crate::config::ManagedCursorAccountConfig],
) -> Option<String> {
    let db_path = cursor::default_state_db_path()?;
    let storage = ProviderAccountStorage::new(paths().cursor_accounts_dir);
    cursor::system_active_account_id(managed_accounts, &storage, &db_path)
}

pub(super) fn codex_system_active_account_id(
    managed_accounts: &[crate::config::ManagedCodexAccountConfig],
) -> Option<String> {
    let auth_path = host_user_home_dir()?.join(".codex/auth.json");
    codex::system_active_account_id(managed_accounts, &auth_path)
}

pub(super) fn claude_system_active_account_id(
    managed_accounts: &[crate::config::ManagedClaudeAccountConfig],
) -> Option<String> {
    let path = host_user_home_dir()?.join(".claude.json");
    claude::system_active_account_id(managed_accounts, &path)
}

pub(super) fn gemini_system_active_account_id(
    managed_accounts: &[crate::config::ManagedGeminiAccountConfig],
) -> Option<String> {
    let path = host_user_home_dir()?
        .join(".gemini")
        .join("google_accounts.json");
    gemini::system_active_account_id(managed_accounts, &path)
}
