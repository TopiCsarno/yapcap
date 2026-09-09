// SPDX-License-Identifier: MPL-2.0

mod antigravity_adapter;
mod claude_adapter;
mod codex_adapter;
mod copilot_adapter;
mod cursor_adapter;
mod gemini_adapter;
mod grok_adapter;
mod kimi_adapter;
mod minimax_adapter;
mod opencode_go_adapter;

use crate::account_storage::ProviderAccountStorage;
use crate::config::{Config, host_user_home_dir, paths};
use crate::model::{
    AccountSelectionStatus, AppState, AuthState, ProviderAccountRuntimeState, ProviderId,
};
use crate::providers::interface::{ProviderAccountDescriptor, ProviderAdapter};
use crate::providers::{claude, codex, cursor, gemini, grok, opencode_go};

pub(super) fn adapter(provider: ProviderId) -> &'static dyn ProviderAdapter {
    match provider {
        ProviderId::Codex => &CODEX_ADAPTER,
        ProviderId::Claude => &CLAUDE_ADAPTER,
        ProviderId::Cursor => &CURSOR_ADAPTER,
        ProviderId::Gemini => &GEMINI_ADAPTER,
        ProviderId::Copilot => &COPILOT_ADAPTER,
        ProviderId::Kimi => &KIMI_ADAPTER,
        ProviderId::Minimax => &MINIMAX_ADAPTER,
        ProviderId::Antigravity => &ANTIGRAVITY_ADAPTER,
        ProviderId::OpenCodeGo => &OPENCODE_GO_ADAPTER,
        ProviderId::Grok => &GROK_ADAPTER,
    }
}

static CODEX_ADAPTER: codex_adapter::CodexAdapter = codex_adapter::CodexAdapter;
static CLAUDE_ADAPTER: claude_adapter::ClaudeAdapter = claude_adapter::ClaudeAdapter;
static CURSOR_ADAPTER: cursor_adapter::CursorAdapter = cursor_adapter::CursorAdapter;
static GEMINI_ADAPTER: gemini_adapter::GeminiAdapter = gemini_adapter::GeminiAdapter;
static COPILOT_ADAPTER: copilot_adapter::CopilotAdapter = copilot_adapter::CopilotAdapter;
static KIMI_ADAPTER: kimi_adapter::KimiAdapter = kimi_adapter::KimiAdapter;
static MINIMAX_ADAPTER: minimax_adapter::MinimaxAdapter = minimax_adapter::MinimaxAdapter;
static ANTIGRAVITY_ADAPTER: antigravity_adapter::AntigravityAdapter =
    antigravity_adapter::AntigravityAdapter;
static OPENCODE_GO_ADAPTER: opencode_go_adapter::OpenCodeGoAdapter =
    opencode_go_adapter::OpenCodeGoAdapter;
static GROK_ADAPTER: grok_adapter::GrokAdapter = grok_adapter::GrokAdapter;

pub(super) fn opencode_go_system_active_account_id(
    managed_accounts: &[crate::config::ManagedOpenCodeGoAccountConfig],
) -> Option<String> {
    let storage = ProviderAccountStorage::new(paths().opencode_go_accounts_dir);
    if let Some(key) = opencode_go::opencode::discover_api_key() {
        return opencode_go::account::system_active_account_id(managed_accounts, &storage, &key);
    }
    let (key, source) = opencode_go::environment_api_key()?;
    opencode_go::account::system_active_account_id(managed_accounts, &storage, &key).or_else(|| {
        managed_accounts
            .iter()
            .find(|account| account.api_key_source == source)
            .map(|account| account.id.clone())
    })
}

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

    selected_ids.truncate(1);

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
            && entry.auth_state == AuthState::ActionRequired
            && entry.error.as_deref() == Some("Not refreshed yet")
        {
            entry.auth_state = AuthState::Ready;
        }

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

pub(super) fn minimax_system_active_account_id(
    managed_accounts: &[crate::config::ManagedMinimaxAccountConfig],
) -> Option<String> {
    std::env::var("MINIMAX_API_KEY").ok().and_then(|api_key| {
        if api_key.is_empty() {
            None
        } else {
            managed_accounts
                .iter()
                .find(|account| account.api_key_source == "env:MINIMAX_API_KEY")
                .map(|account| account.id.clone())
        }
    })
}

pub(super) fn kimi_system_active_account_id(
    managed_accounts: &[crate::config::ManagedKimiAccountConfig],
) -> Option<String> {
    std::env::var("KIMI_API_KEY").ok().and_then(|api_key| {
        if api_key.is_empty() {
            None
        } else {
            managed_accounts
                .iter()
                .find(|account| account.api_key_source == "env:KIMI_API_KEY")
                .map(|account| account.id.clone())
        }
    })
}

pub(super) fn grok_system_active_account_id(
    managed_accounts: &[crate::config::ManagedGrokAccountConfig],
) -> Option<String> {
    let path = host_user_home_dir()?.join(".grok").join("auth.json");
    grok::account::system_active_account_id(managed_accounts, &path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_provider_has_an_owning_adapter() {
        for provider in ProviderId::ALL {
            assert_eq!(adapter(provider).id(), provider);
        }
    }

    #[test]
    fn every_supported_provider_declares_login_controls() {
        for provider in ProviderId::ALL {
            let _ = adapter(provider).login_kind();
        }
    }

    #[test]
    fn every_supported_provider_exposes_startup_sync() {
        let mut config = Config::default();

        for provider in ProviderId::ALL {
            let _ = adapter(provider).sync_managed_accounts(&mut config);
        }
    }
}
