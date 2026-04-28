// SPDX-License-Identifier: MPL-2.0

#[cfg(debug_assertions)]
use crate::config::ManagedCursorAccountConfig;
use crate::config::{Config, ProviderVisibilityMode, paths};
use crate::error::AppError;
use crate::model::{
    AccountSelectionStatus, AppState, AuthState, ProviderAccountRuntimeState, ProviderHealth,
    ProviderId, UsageSnapshot,
};
use crate::providers::interface::{
    BoxFuture, ProviderAccountDescriptor, ProviderAccountHandle, ProviderAdapter,
    ProviderCapabilities, ProviderDiscoveredAccount,
};
use crate::providers::{claude, codex, cursor};
use chrono::Utc;
use std::collections::HashMap;
use tokio::task::JoinSet;

#[cfg(test)]
mod tests;

pub fn ambient_active_account_id(provider: ProviderId, config: &Config) -> Option<String> {
    adapter(provider).ambient_active_account_id(config)
}

pub fn capabilities(provider: ProviderId) -> ProviderCapabilities {
    adapter(provider).capabilities()
}

pub fn startup_cleanup() {
    cursor::cleanup_pending_dirs();
}

pub fn startup_sync(config: &mut Config) -> bool {
    let codex_changed = codex::sync_imported_account(config).unwrap_or_else(|error| {
        tracing::warn!(error = %error, "failed to import external Codex home");
        false
    });
    let claude_import_changed = claude::sync_imported_account(config).unwrap_or_else(|error| {
        tracing::warn!(error = %error, "failed to sync external Claude config");
        false
    });
    let claude_changed = claude::sync_managed_accounts(config);
    let cursor_changed = cursor::sync_managed_accounts(config);
    codex_changed | claude_import_changed | claude_changed | cursor_changed
}

pub fn initialize_provider_visibility(config: &mut Config, providers: &[ProviderId]) -> bool {
    if config.provider_visibility_mode != ProviderVisibilityMode::AutoInitPending {
        return false;
    }

    let mut changed = false;
    for &provider in providers {
        let enabled = !discover_accounts(provider, config).is_empty();
        changed |= config.set_provider_enabled(provider, enabled);
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

#[cfg(debug_assertions)]
pub fn startup_debug_apply(config: &Config) {
    if cursor::expired_cookie_debug_enabled() {
        cursor::simulate_expired_cookie_accounts(&config.cursor_managed_accounts);
    }
}

#[cfg(debug_assertions)]
pub fn startup_debug_apply_for_accounts(accounts: &[ManagedCursorAccountConfig]) {
    if cursor::expired_cookie_debug_enabled() {
        cursor::simulate_expired_cookie_accounts(accounts);
    }
}

pub async fn browser_account_discovery(
    config: Config,
    client: reqwest::Client,
) -> Vec<ProviderDiscoveredAccount> {
    cursor::discover_browser_accounts(config, client)
        .await
        .into_iter()
        .map(cursor_account_descriptor)
        .collect()
}

pub fn upsert_discovered_accounts(config: &mut Config, accounts: &[ProviderDiscoveredAccount]) {
    let providers = ProviderId::ALL.map(|provider| {
        accounts
            .iter()
            .filter(|account| account.provider == provider)
            .cloned()
            .collect::<Vec<_>>()
    });
    for provider_accounts in providers {
        if let Some(account) = provider_accounts.first() {
            adapter(account.provider).upsert_discovered_accounts(config, &provider_accounts);
        }
    }
}

pub fn discover_accounts(provider: ProviderId, config: &Config) -> Vec<ProviderDiscoveredAccount> {
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

    fn ambient_active_account_id(&self, config: &Config) -> Option<String> {
        codex::ambient_active_account_id(config)
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
        remove_managed_codex_home(&account.codex_home);
        config.codex_managed_accounts.retain(|a| a.id != account_id);
        config
            .selected_codex_account_ids
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
                ProviderAccountHandle::Codex(account) => {
                    codex::fetch(client, account.codex_home.clone())
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
            supports_reauthentication: false,
            supports_background_status_refresh: false,
            requires_auth_prompt_on_auth_failure: false,
        }
    }

    fn ambient_active_account_id(&self, config: &Config) -> Option<String> {
        claude::ambient_active_account_id(config)
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
        let account = config
            .claude_managed_accounts
            .iter()
            .find(|a| a.id == account_id)
            .cloned();
        let Some(account) = account else {
            return false;
        };
        claude::remove_managed_config_dir(&account.config_dir);
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
                    claude::fetch(client, account.config_dir.clone())
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

    fn ambient_active_account_id(&self, config: &Config) -> Option<String> {
        cursor::ambient_active_account_id(config)
    }

    fn discover_accounts(&self, config: &Config) -> Vec<ProviderAccountDescriptor> {
        cursor::discover_accounts(config)
            .into_iter()
            .map(cursor_account_descriptor)
            .collect()
    }

    fn upsert_discovered_accounts(
        &self,
        config: &mut Config,
        accounts: &[ProviderAccountDescriptor],
    ) {
        for account in accounts {
            if let ProviderAccountHandle::Cursor(managed) = &account.handle {
                cursor::upsert_managed_account(config, managed.clone());
            }
        }
    }

    fn delete_account(&self, account_id: &str, config: &mut Config) -> bool {
        let Some(account) =
            cursor::find_managed_account(&config.cursor_managed_accounts, account_id).cloned()
        else {
            return false;
        };
        let email = account.email.clone();
        cursor::remove_managed_profile(&account.account_root);
        config.cursor_managed_accounts.retain(|a| a.email != email);
        config
            .selected_cursor_account_ids
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
        let active_id = ambient_active_account_id(provider, config);
        if let Some(ref id) = active_id
            && selected_ids.contains(id)
        {
            selected_ids = vec![id.clone()];
        } else {
            selected_ids.truncate(1);
        }
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
        provider_state.selected_account_ids = selected_ids;
        provider_state.active_account_id = ambient_active_account_id(provider, config);
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

fn remove_managed_codex_home(codex_home: &std::path::Path) {
    let root = paths().codex_accounts_dir;
    let Ok(root) = root.canonicalize() else {
        return;
    };
    let Ok(metadata) = std::fs::symlink_metadata(codex_home) else {
        return;
    };
    if metadata.file_type().is_symlink() {
        tracing::warn!(path = %codex_home.display(), "refusing to delete symlinked codex account home");
        return;
    }
    let Ok(home) = codex_home.canonicalize() else {
        return;
    };
    if !home.starts_with(&root) {
        tracing::warn!(path = %home.display(), root = %root.display(), "refusing to delete codex account outside managed root");
        return;
    }
    if let Err(error) = std::fs::remove_dir_all(&home) {
        tracing::warn!(path = %home.display(), error = %error, "failed to delete codex account home");
    }
}
