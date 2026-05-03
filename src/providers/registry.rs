// SPDX-License-Identifier: MPL-2.0

use crate::account_storage::ProviderAccountStorage;
use crate::config::{
    Config, ProviderVisibilityMode, managed_claude_account_dir, managed_codex_account_dir, paths,
};
use crate::error::AppError;
use crate::model::{
    AccountSelectionStatus, AppState, AuthState, ProviderAccountRuntimeState, ProviderHealth,
    ProviderId, UsageSnapshot,
};
use crate::providers::interface::{
    BoxFuture, ProviderAccountDescriptor, ProviderAccountHandle, ProviderAdapter,
    ProviderCapabilities,
};
use crate::providers::{claude, codex, cursor};
use chrono::Utc;
use std::collections::HashMap;
use tokio::task::JoinSet;

#[cfg(test)]
mod tests;

pub fn capabilities(provider: ProviderId) -> ProviderCapabilities {
    adapter(provider).capabilities()
}

pub fn startup_sync(config: &mut Config) -> bool {
    let codex_changed = codex::sync_managed_accounts(config);
    let cursor_changed = cursor::sync_managed_accounts(config);
    let claude_changed = claude::sync_managed_account_dirs(config);
    codex_changed | cursor_changed | claude_changed
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
) -> crate::error::Result<UsageSnapshot, AppError> {
    let provider = match handle {
        ProviderAccountHandle::Codex(_) => ProviderId::Codex,
        ProviderAccountHandle::Claude(_) => ProviderId::Claude,
        ProviderAccountHandle::Cursor(_) => ProviderId::Cursor,
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

pub async fn refresh_account_statuses(
    provider: ProviderId,
    config: Config,
    previous_accounts: Vec<ProviderAccountRuntimeState>,
) -> Vec<ProviderAccountRuntimeState> {
    adapter(provider)
        .refresh_account_statuses(config, previous_accounts)
        .await
}

fn adapter(provider: ProviderId) -> &'static dyn ProviderAdapter {
    match provider {
        ProviderId::Codex => &CODEX_ADAPTER,
        ProviderId::Claude => &CLAUDE_ADAPTER,
        ProviderId::Cursor => &CURSOR_ADAPTER,
    }
}

static CODEX_ADAPTER: CodexAdapter = CodexAdapter;
static CLAUDE_ADAPTER: ClaudeAdapter = ClaudeAdapter;
static CURSOR_ADAPTER: CursorAdapter = CursorAdapter;

struct CodexAdapter;

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

struct ClaudeAdapter;

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

struct CursorAdapter;

impl ProviderAdapter for CursorAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::Cursor
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_delete: true,
            supports_reauthentication: true,
            supports_background_status_refresh: true,
            requires_auth_prompt_on_auth_failure: true,
        }
    }

    fn discover_accounts(&self, config: &Config) -> Vec<ProviderAccountDescriptor> {
        cursor::discover_accounts(config)
            .into_iter()
            .map(cursor_account_descriptor)
            .collect()
    }

    fn delete_account(&self, account_id: &str, config: &mut Config) -> bool {
        let Some(account) =
            cursor::find_managed_account(&config.cursor_managed_accounts, account_id).cloned()
        else {
            return false;
        };
        let email = account.email.clone();
        remove_managed_cursor_account(&account.id);
        config.cursor_managed_accounts.retain(|a| a.email != email);
        config
            .selected_cursor_account_ids
            .retain(|id| id != account_id);
        true
    }

    fn reconcile_provider_accounts(&self, config: &Config, state: &mut AppState) {
        let accounts = self.discover_accounts(config);
        reconcile_provider_account_descriptors(self.id(), config, state, &accounts);
        if let Some(provider_state) = state.provider_mut(ProviderId::Cursor) {
            provider_state.system_active_account_id =
                cursor_system_active_account_id(&config.cursor_managed_accounts);
        }
    }

    fn fetch_account<'a>(
        &self,
        handle: &'a ProviderAccountHandle,
        client: &'a reqwest::Client,
    ) -> BoxFuture<'a, crate::error::Result<UsageSnapshot, AppError>> {
        Box::pin(async move {
            match handle {
                ProviderAccountHandle::Cursor(account) => {
                    cursor::fetch(client, account).await.map_err(AppError::from)
                }
                _ => unreachable!(),
            }
        })
    }

    fn refresh_account_statuses(
        &self,
        config: Config,
        previous_accounts: Vec<ProviderAccountRuntimeState>,
    ) -> BoxFuture<'static, Vec<ProviderAccountRuntimeState>> {
        Box::pin(refresh_cursor_account_statuses(config, previous_accounts))
    }
}

fn cursor_account_descriptor(
    account: crate::config::ManagedCursorAccountConfig,
) -> ProviderAccountDescriptor {
    let account_id = cursor::managed_account_id(&account.id);
    let label = account.email.clone();
    ProviderAccountDescriptor {
        provider: ProviderId::Cursor,
        account_id,
        label,
        capabilities: capabilities(ProviderId::Cursor),
        handle: ProviderAccountHandle::Cursor(account),
    }
}

fn reconcile_provider_account_descriptors(
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

async fn refresh_cursor_account_statuses(
    config: Config,
    previous_accounts: Vec<ProviderAccountRuntimeState>,
) -> Vec<ProviderAccountRuntimeState> {
    let client = crate::runtime::http_client();
    let accounts = cursor::discover_accounts(&config);
    let mut previous_by_id = previous_accounts
        .into_iter()
        .filter(|entry| entry.provider == ProviderId::Cursor)
        .map(|entry| (entry.account_id.clone(), entry))
        .collect::<HashMap<_, _>>();
    let mut tasks = JoinSet::new();

    for managed in accounts {
        let client = client.clone();
        let account_id = cursor::managed_account_id(&managed.id);
        let previous = previous_by_id.remove(&account_id);
        tasks.spawn(async move { refresh_cursor_account_status(client, managed, previous).await });
    }

    let mut refreshed = Vec::with_capacity(tasks.len());
    while let Some(result) = tasks.join_next().await {
        if let Ok(account) = result {
            refreshed.push(account);
        }
    }
    refreshed.sort_by(|left, right| left.account_id.cmp(&right.account_id));
    refreshed
}

async fn refresh_cursor_account_status(
    client: reqwest::Client,
    managed: crate::config::ManagedCursorAccountConfig,
    previous: Option<ProviderAccountRuntimeState>,
) -> ProviderAccountRuntimeState {
    let account_id = cursor::managed_account_id(&managed.id);
    let label = managed.email.clone();
    let mut account = previous.unwrap_or_else(|| {
        ProviderAccountRuntimeState::empty(ProviderId::Cursor, account_id, label)
    });

    if account.auth_state == AuthState::ActionRequired {
        return account;
    }

    match cursor::fetch(&client, &managed).await {
        Ok(snapshot) => {
            account.health = ProviderHealth::Ok;
            account.auth_state = AuthState::Ready;
            account.source_label = Some(snapshot.source.clone());
            account.last_success_at = Some(Utc::now());
            account.snapshot = Some(snapshot);
            account.error = None;
        }
        Err(error) => {
            let error = AppError::from(error);
            account.health = ProviderHealth::Error;
            account.auth_state = crate::runtime::classify_auth_state(&error);
            account.error = Some(error.user_message());
        }
    }

    account
}

fn remove_managed_codex_account(account_id: &str) {
    let storage = ProviderAccountStorage::new(paths().codex_accounts_dir);
    if let Err(error) = storage.delete_account(account_id) {
        tracing::warn!(account_id, error = %error, "failed to delete codex account");
    }
}

fn remove_managed_cursor_account(account_id: &str) {
    let storage = ProviderAccountStorage::new(paths().cursor_accounts_dir);
    if let Err(error) = storage.delete_account(account_id) {
        tracing::warn!(account_id, error = %error, "failed to delete cursor account");
    }
}

pub(crate) fn cursor_system_active_account_id(
    managed_accounts: &[crate::config::ManagedCursorAccountConfig],
) -> Option<String> {
    let db_path = cursor::default_state_db_path()?;
    let storage = ProviderAccountStorage::new(paths().cursor_accounts_dir);
    cursor::system_active_account_id(managed_accounts, &storage, &db_path)
}

pub(crate) fn codex_system_active_account_id(
    managed_accounts: &[crate::config::ManagedCodexAccountConfig],
) -> Option<String> {
    let auth_path = dirs::home_dir()?.join(".codex/auth.json");
    codex::system_active_account_id(managed_accounts, &auth_path)
}
