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
    ProviderAccountActionSupport, ProviderAccountHandle, ProviderDiscoveredAccount,
};
use crate::providers::{claude, codex, cursor};
use chrono::Utc;
use std::collections::HashMap;
use tokio::task::JoinSet;

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
        changed |= set_provider_enabled_flag(config, provider, enabled);
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
        .map(cursor_account_to_discovered)
        .collect()
}

pub fn upsert_discovered_accounts(config: &mut Config, accounts: &[ProviderDiscoveredAccount]) {
    for account in accounts {
        if let ProviderAccountHandle::Cursor(managed) = &account.handle {
            cursor::upsert_managed_account(config, managed.clone());
        }
    }
}

pub fn discover_accounts(provider: ProviderId, config: &Config) -> Vec<ProviderDiscoveredAccount> {
    match provider {
        ProviderId::Codex => {
            let action_support = ProviderAccountActionSupport {
                can_delete: true,
                can_reauthenticate: false,
                supports_background_status_refresh: false,
            };
            codex::discover_accounts(config)
                .into_iter()
                .filter_map(|account| {
                    config
                        .codex_managed_accounts
                        .iter()
                        .find(|managed| managed.id == account.id)
                        .cloned()
                        .map(|managed| ProviderDiscoveredAccount {
                            provider,
                            account_id: account.id,
                            label: account.label,
                            action_support: action_support.clone(),
                            handle: ProviderAccountHandle::Codex(managed),
                        })
                })
                .collect()
        }
        ProviderId::Claude => {
            let action_support = ProviderAccountActionSupport {
                can_delete: true,
                can_reauthenticate: false,
                supports_background_status_refresh: false,
            };
            claude::discover_accounts(config)
                .into_iter()
                .filter_map(|account| {
                    config
                        .claude_managed_accounts
                        .iter()
                        .find(|managed| managed.id == account.id)
                        .cloned()
                        .map(|managed| ProviderDiscoveredAccount {
                            provider,
                            account_id: account.id,
                            label: account.label,
                            action_support: action_support.clone(),
                            handle: ProviderAccountHandle::Claude(managed),
                        })
                })
                .collect()
        }
        ProviderId::Cursor => cursor::discover_accounts(config)
            .into_iter()
            .map(cursor_account_to_discovered)
            .collect(),
    }
}

fn cursor_account_to_discovered(
    account: crate::config::ManagedCursorAccountConfig,
) -> ProviderDiscoveredAccount {
    let account_id = cursor::managed_account_id(&account.id);
    let label = account.email.clone();
    ProviderDiscoveredAccount {
        provider: ProviderId::Cursor,
        account_id,
        label,
        action_support: ProviderAccountActionSupport {
            can_delete: true,
            can_reauthenticate: true,
            supports_background_status_refresh: true,
        },
        handle: ProviderAccountHandle::Cursor(account),
    }
}

pub fn active_account_preference(provider: ProviderId, config: &Config) -> Option<String> {
    match provider {
        ProviderId::Codex => config.active_codex_account_id.clone(),
        ProviderId::Claude => config.active_claude_account_id.clone(),
        ProviderId::Cursor => config.active_cursor_account_id.clone(),
    }
}

pub fn set_active_account_preference(
    provider: ProviderId,
    config: &mut Config,
    account_id: Option<String>,
) {
    match provider {
        ProviderId::Codex => config.active_codex_account_id = account_id,
        ProviderId::Claude => config.active_claude_account_id = account_id,
        ProviderId::Cursor => config.active_cursor_account_id = account_id,
    }
}

fn set_provider_enabled_flag(config: &mut Config, provider: ProviderId, enabled: bool) -> bool {
    let target = match provider {
        ProviderId::Codex => &mut config.codex_enabled,
        ProviderId::Claude => &mut config.claude_enabled,
        ProviderId::Cursor => &mut config.cursor_enabled,
    };
    let changed = *target != enabled;
    *target = enabled;
    changed
}

pub fn sync_active_preference_with_discoveries(config: &mut Config, provider: ProviderId) {
    let valid: Vec<String> = discover_accounts(provider, config)
        .into_iter()
        .map(|a| a.account_id)
        .collect();
    let preferred = active_account_preference(provider, config);
    let next = resolve_active_account(preferred.as_deref(), &valid);
    set_active_account_preference(provider, config, next);
}

pub async fn fetch_handle(
    handle: &ProviderAccountHandle,
    client: &reqwest::Client,
) -> crate::error::Result<UsageSnapshot, AppError> {
    match handle {
        ProviderAccountHandle::Codex(account) => codex::fetch(client, account.codex_home.clone())
            .await
            .map_err(AppError::from),
        ProviderAccountHandle::Claude(account) => claude::fetch(client, account.config_dir.clone())
            .await
            .map_err(AppError::from),
        ProviderAccountHandle::Cursor(account) => {
            cursor::fetch(client, account).await.map_err(AppError::from)
        }
    }
}

pub fn supports_background_status_refresh(provider: ProviderId) -> bool {
    matches!(provider, ProviderId::Cursor)
}

pub fn auth_error_requires_reauth_prompt(provider: ProviderId) -> bool {
    matches!(provider, ProviderId::Cursor)
}

pub fn delete_account(provider: ProviderId, account_id: &str, config: &mut Config) -> bool {
    match provider {
        ProviderId::Cursor => {
            let Some(account) =
                cursor::find_managed_account(&config.cursor_managed_accounts, account_id).cloned()
            else {
                return false;
            };
            let email = account.email.clone();
            cursor::remove_managed_profile(&account.account_root);
            config.cursor_managed_accounts.retain(|a| a.email != email);
            if config.active_cursor_account_id.as_deref() == Some(account_id) {
                config.active_cursor_account_id = None;
            }
            true
        }
        ProviderId::Codex => {
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
            if config.active_codex_account_id.as_deref() == Some(account_id) {
                config.active_codex_account_id = None;
            }
            true
        }
        ProviderId::Claude => {
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
            if config.active_claude_account_id.as_deref() == Some(account_id) {
                config.active_claude_account_id = None;
            }
            true
        }
    }
}

pub fn reconcile_provider_accounts(provider: ProviderId, config: &Config, state: &mut AppState) {
    let accounts = discover_accounts(provider, config);
    let valid_ids: Vec<String> = accounts.iter().map(|a| a.account_id.clone()).collect();
    let preferred = active_account_preference(provider, config);
    let active_id = resolve_active_account(preferred.as_deref(), &valid_ids);

    state
        .provider_accounts
        .retain(|entry| entry.provider != provider || valid_ids.contains(&entry.account_id));

    for account in &accounts {
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
            && active_id.as_deref() == Some(account.account_id.as_str())
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
        provider_state.active_account_id = active_id;
        provider_state.account_status =
            account_status(provider_state.active_account_id.as_deref(), accounts.len());
        provider_state.error = match provider_state.account_status {
            AccountSelectionStatus::LoginRequired => Some("Login required".to_string()),
            AccountSelectionStatus::SelectionRequired => Some("Select an account".to_string()),
            _ => provider_state.error.take(),
        };
    }
}

fn resolve_active_account(persisted: Option<&str>, valid_ids: &[String]) -> Option<String> {
    if let Some(persisted) = persisted
        && valid_ids.iter().any(|id| id == persisted)
    {
        return Some(persisted.to_string());
    }
    if valid_ids.len() == 1 {
        return valid_ids.first().cloned();
    }
    None
}

fn account_status(active_id: Option<&str>, valid_count: usize) -> AccountSelectionStatus {
    if active_id.is_some() {
        AccountSelectionStatus::Ready
    } else if valid_count == 0 {
        AccountSelectionStatus::LoginRequired
    } else {
        AccountSelectionStatus::SelectionRequired
    }
}

pub async fn refresh_account_statuses(
    provider: ProviderId,
    config: Config,
    previous_accounts: Vec<ProviderAccountRuntimeState>,
) -> Vec<ProviderAccountRuntimeState> {
    if !supports_background_status_refresh(provider) {
        return Vec::new();
    }
    refresh_cursor_account_statuses(config, previous_accounts).await
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderVisibilityMode;
    use crate::config::{Config, CursorCredentialSource, ManagedCursorAccountConfig};
    use chrono::Utc;
    use std::path::PathBuf;

    #[test]
    fn cursor_supports_background_status_refresh() {
        assert!(supports_background_status_refresh(ProviderId::Cursor));
        assert!(!supports_background_status_refresh(ProviderId::Codex));
        assert!(!supports_background_status_refresh(ProviderId::Claude));
    }

    #[test]
    fn cursor_requires_reauth_prompt_on_auth_error() {
        assert!(auth_error_requires_reauth_prompt(ProviderId::Cursor));
        assert!(!auth_error_requires_reauth_prompt(ProviderId::Codex));
        assert!(!auth_error_requires_reauth_prompt(ProviderId::Claude));
    }

    #[test]
    fn each_provider_resolves_accounts() {
        let config = Config::default();
        for provider in ProviderId::ALL {
            let accounts = discover_accounts(provider, &config);
            assert!(
                accounts.is_empty(),
                "default config should have no accounts for {provider:?}"
            );
        }
    }

    #[test]
    fn initialize_provider_visibility_disables_empty_provider_for_new_config() {
        let mut config = Config::default();

        assert!(initialize_provider_visibility(
            &mut config,
            &[ProviderId::Cursor]
        ));
        assert!(!config.cursor_enabled);
        assert_eq!(
            config.provider_visibility_mode,
            ProviderVisibilityMode::AutoInitPending
        );
    }

    #[test]
    fn initialize_provider_visibility_enables_provider_when_account_exists() {
        let now = Utc::now();
        let mut config = Config {
            cursor_enabled: false,
            cursor_managed_accounts: vec![ManagedCursorAccountConfig {
                id: "cursor-test".to_string(),
                email: "user@example.com".to_string(),
                label: "user@example.com".to_string(),
                account_root: PathBuf::from("/tmp/cursor-test"),
                credential_source: CursorCredentialSource::ImportedBrowserProfile,
                browser: None,
                display_name: None,
                plan: None,
                created_at: now,
                updated_at: now,
                last_authenticated_at: Some(now),
            }],
            ..Config::default()
        };

        assert!(initialize_provider_visibility(
            &mut config,
            &[ProviderId::Cursor]
        ));
        assert!(config.cursor_enabled);
    }

    #[test]
    fn initialize_provider_visibility_is_noop_after_initialization() {
        let mut config = Config {
            provider_visibility_mode: ProviderVisibilityMode::UserManaged,
            ..Config::default()
        };

        assert!(!initialize_provider_visibility(
            &mut config,
            &[ProviderId::Codex, ProviderId::Claude, ProviderId::Cursor]
        ));
        assert!(config.codex_enabled);
        assert!(config.claude_enabled);
        assert!(config.cursor_enabled);
    }
}
