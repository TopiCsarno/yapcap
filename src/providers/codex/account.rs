// SPDX-License-Identifier: MPL-2.0

use crate::account_storage::ProviderAccountStorage;
use crate::config::{Config, ManagedCodexAccountConfig, managed_codex_account_dir, paths};
use chrono::Utc;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexAccount {
    pub id: String,
    pub label: String,
    pub email: Option<String>,
    pub provider_account_id: Option<String>,
    pub codex_home: PathBuf,
}

pub fn discover_accounts(config: &Config) -> Vec<CodexAccount> {
    let storage = ProviderAccountStorage::new(paths().codex_accounts_dir);
    let mut accounts = Vec::new();
    for managed in &config.codex_managed_accounts {
        let Ok(metadata) = storage.load_metadata(&managed.id) else {
            continue;
        };
        if storage.load_tokens(&managed.id).is_err() {
            continue;
        }
        let email = Some(metadata.email).filter(|email| !email.is_empty());
        let label = email.clone().unwrap_or_else(|| "Codex account".to_string());
        let discovered = CodexAccount {
            id: managed.id.clone(),
            label,
            email,
            provider_account_id: metadata.provider_account_id,
            codex_home: storage.account_dir(&managed.id),
        };
        match discovered.email.as_deref().map(normalized_email) {
            Some(email_key) => {
                if let Some(index) = accounts.iter().position(|existing: &CodexAccount| {
                    existing.email.as_deref().map(normalized_email) == Some(email_key.clone())
                }) {
                    let existing = &accounts[index];
                    if prefer_account(existing, &discovered, &config.selected_codex_account_ids) {
                        continue;
                    }
                    accounts[index] = discovered;
                } else {
                    accounts.push(discovered);
                }
            }
            None => accounts.push(discovered),
        }
    }
    accounts
}

pub fn sync_managed_accounts(config: &mut Config) -> bool {
    let changed = dedupe_managed_accounts(config);
    let dirs_changed = resync_managed_codex_dirs(config);
    let had_system = config
        .selected_codex_account_ids
        .iter()
        .any(|id| id == "system");
    config
        .selected_codex_account_ids
        .retain(|id| id != "system");
    changed || had_system || dirs_changed
}

fn resync_managed_codex_dirs(config: &mut Config) -> bool {
    let mut dirs_changed = false;
    for account in &mut config.codex_managed_accounts {
        let canonical = managed_codex_account_dir(&account.id);
        if account.codex_home != canonical {
            account.codex_home = canonical;
            dirs_changed = true;
        }
    }
    dirs_changed
}

pub fn apply_login_account(config: &mut Config, account: ManagedCodexAccountConfig) {
    let account_id = account.id.clone();
    if !config.selected_codex_account_ids.contains(&account_id) {
        config.selected_codex_account_ids.push(account_id.clone());
    }
    config
        .codex_managed_accounts
        .retain(|existing| existing.id != account_id);
    config.codex_managed_accounts.push(account);
    dedupe_managed_accounts(config);
}

pub fn normalized_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

pub(crate) fn find_matching_account<'a>(
    config: &'a Config,
    email: Option<&str>,
) -> Option<&'a ManagedCodexAccountConfig> {
    let email = email?;
    let email = normalized_email(email);
    config
        .codex_managed_accounts
        .iter()
        .find(|account| account.email.as_deref().map(normalized_email) == Some(email.clone()))
}

pub(crate) fn create_private_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path)
        .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
    set_private_dir_permissions(path)?;
    Ok(())
}

pub(crate) fn new_account_id() -> String {
    let millis = Utc::now().timestamp_millis();
    format!("codex-{millis}-{}", std::process::id())
}

fn dedupe_managed_accounts(config: &mut Config) -> bool {
    let original_selected = config.selected_codex_account_ids.clone();
    let mut selected_ids = original_selected.clone();
    let original_len = config.codex_managed_accounts.len();
    let mut deduped = Vec::new();

    for account in config.codex_managed_accounts.drain(..) {
        let Some(email_key) = account.email.as_deref().map(normalized_email) else {
            deduped.push(account);
            continue;
        };

        if let Some(index) = deduped
            .iter()
            .position(|existing: &ManagedCodexAccountConfig| {
                existing.email.as_deref().map(normalized_email) == Some(email_key.clone())
            })
        {
            let existing = deduped.remove(index);
            let keep_existing = prefer_managed_account(&existing, &account, &original_selected);
            let (mut winner, loser) = if keep_existing {
                (existing, account)
            } else {
                (account, existing)
            };
            let loser_id = loser.id.clone();
            let winner_id = winner.id.clone();
            merge_account_metadata(&mut winner, &loser);
            for id in &mut selected_ids {
                if id == loser_id.as_str() {
                    id.clone_from(&winner_id);
                }
            }
            deduped.push(winner);
            continue;
        }

        deduped.push(account);
    }

    selected_ids.retain(|id| id != "system");
    let changed = deduped.len() != original_len || selected_ids != original_selected;
    config.codex_managed_accounts = deduped;
    config.selected_codex_account_ids = selected_ids;
    changed
}

fn prefer_account(
    existing: &CodexAccount,
    candidate: &CodexAccount,
    selected_ids: &[String],
) -> bool {
    if selected_ids.iter().any(|id| id == existing.id.as_str()) {
        return true;
    }
    if selected_ids.iter().any(|id| id == candidate.id.as_str()) {
        return false;
    }
    existing.id >= candidate.id
}

fn prefer_managed_account(
    existing: &ManagedCodexAccountConfig,
    candidate: &ManagedCodexAccountConfig,
    selected_ids: &[String],
) -> bool {
    if selected_ids.iter().any(|id| id == existing.id.as_str()) {
        return true;
    }
    if selected_ids.iter().any(|id| id == candidate.id.as_str()) {
        return false;
    }
    let existing_auth = existing.last_authenticated_at.or(Some(existing.updated_at));
    let candidate_auth = candidate
        .last_authenticated_at
        .or(Some(candidate.updated_at));
    existing_auth >= candidate_auth
}

fn merge_account_metadata(
    target: &mut ManagedCodexAccountConfig,
    source: &ManagedCodexAccountConfig,
) {
    if target.email.is_none() {
        target.email.clone_from(&source.email);
    }
    if target.provider_account_id.is_none() {
        target
            .provider_account_id
            .clone_from(&source.provider_account_id);
    }
    if target.label == "Codex account" && source.label != "Codex account" {
        target.label.clone_from(&source.label);
    }
    if source.created_at < target.created_at {
        target.created_at = source.created_at;
    }
    if source.updated_at > target.updated_at {
        target.updated_at = source.updated_at;
    }
    if source.last_authenticated_at > target.last_authenticated_at {
        target.last_authenticated_at = source.last_authenticated_at;
    }
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("failed to secure {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_storage::{
        NewProviderAccount, ProviderAccountStorage, ProviderAccountTokens,
    };
    use crate::model::ProviderId;
    use crate::test_support;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("yapcap-{name}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn create_stored_account(
        storage: &ProviderAccountStorage,
        email: &str,
        provider_account_id: &str,
    ) -> crate::account_storage::StoredProviderAccount {
        storage
            .create_account(NewProviderAccount {
                provider: ProviderId::Codex,
                email: email.to_string(),
                provider_account_id: Some(provider_account_id.to_string()),
                organization_id: None,
                organization_name: None,
                tokens: ProviderAccountTokens {
                    access_token: "access".to_string(),
                    refresh_token: "refresh".to_string(),
                    expires_at: Utc::now() + chrono::TimeDelta::hours(1),
                    scope: Vec::new(),
                    token_id: None,
                },
                snapshot: None,
            })
            .unwrap()
    }

    #[test]
    fn sync_managed_accounts_removes_legacy_system_selection_without_importing() {
        let _guard = test_support::env_lock();
        let home = temp_dir("codex-import-source");
        let state_root = temp_dir("codex-import-state");

        unsafe {
            std::env::set_var("CODEX_HOME", &home);
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }

        let mut config = Config {
            selected_codex_account_ids: vec!["system".to_string()],
            ..Config::default()
        };

        let changed = sync_managed_accounts(&mut config);

        unsafe {
            std::env::remove_var("CODEX_HOME");
            std::env::remove_var("XDG_STATE_HOME");
        }

        assert!(changed);
        assert!(config.codex_managed_accounts.is_empty());
        assert!(config.selected_codex_account_ids.is_empty());
    }

    #[test]
    fn dedupes_existing_same_email_accounts() {
        let now = Utc::now();
        let home_a = temp_dir("codex-dedupe-a");
        let home_b = temp_dir("codex-dedupe-b");

        let mut config = Config {
            selected_codex_account_ids: vec!["codex-b".to_string()],
            codex_managed_accounts: vec![
                ManagedCodexAccountConfig {
                    id: "codex-a".to_string(),
                    label: "user@example.com".to_string(),
                    codex_home: home_a,
                    email: Some("user@example.com".to_string()),
                    provider_account_id: Some("acct-123".to_string()),
                    created_at: now,
                    updated_at: now,
                    last_authenticated_at: Some(now),
                },
                ManagedCodexAccountConfig {
                    id: "codex-b".to_string(),
                    label: "USER@example.com".to_string(),
                    codex_home: home_b,
                    email: Some("USER@example.com".to_string()),
                    provider_account_id: None,
                    created_at: now,
                    updated_at: now + chrono::TimeDelta::seconds(1),
                    last_authenticated_at: Some(now + chrono::TimeDelta::seconds(1)),
                },
            ],
            ..Default::default()
        };

        let changed = dedupe_managed_accounts(&mut config);

        assert!(changed);
        assert_eq!(config.codex_managed_accounts.len(), 1);
        assert_eq!(
            config.codex_managed_accounts[0].email.as_deref(),
            Some("USER@example.com")
        );
        assert_eq!(config.selected_codex_account_ids.as_slice(), ["codex-b"]);
        assert_eq!(
            config.codex_managed_accounts[0]
                .provider_account_id
                .as_deref(),
            Some("acct-123")
        );
    }

    #[test]
    fn discover_accounts_returns_one_entry_per_email() {
        let now = Utc::now();
        let _guard = test_support::env_lock();
        let state_root = temp_dir("codex-discover-state");

        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }

        let storage = ProviderAccountStorage::new(paths().codex_accounts_dir);
        let account_a = create_stored_account(&storage, "user@example.com", "acct-123");
        let account_b = create_stored_account(&storage, "USER@example.com", "acct-999");

        let config = Config {
            selected_codex_account_ids: vec![account_b.account_ref.account_id.clone()],
            codex_managed_accounts: vec![
                ManagedCodexAccountConfig {
                    id: account_a.account_ref.account_id.clone(),
                    label: "user@example.com".to_string(),
                    codex_home: account_a.account_dir,
                    email: Some("user@example.com".to_string()),
                    provider_account_id: Some("acct-123".to_string()),
                    created_at: now,
                    updated_at: now,
                    last_authenticated_at: Some(now),
                },
                ManagedCodexAccountConfig {
                    id: account_b.account_ref.account_id.clone(),
                    label: "USER@example.com".to_string(),
                    codex_home: account_b.account_dir,
                    email: Some("USER@example.com".to_string()),
                    provider_account_id: Some("acct-999".to_string()),
                    created_at: now,
                    updated_at: now + chrono::TimeDelta::seconds(1),
                    last_authenticated_at: Some(now + chrono::TimeDelta::seconds(1)),
                },
            ],
            ..Config::default()
        };

        let accounts = discover_accounts(&config);

        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }

        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, account_b.account_ref.account_id);
    }

    #[test]
    fn discover_accounts_reads_yapcap_owned_storage() {
        let _guard = test_support::env_lock();
        let state_root = temp_dir("codex-storage-discover-state");

        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }

        let storage = ProviderAccountStorage::new(paths().codex_accounts_dir);
        let stored = create_stored_account(&storage, "user@example.com", "acct-123");

        let now = Utc::now();
        let config = Config {
            codex_managed_accounts: vec![ManagedCodexAccountConfig {
                id: stored.account_ref.account_id.clone(),
                label: "old label".to_string(),
                codex_home: stored.account_dir,
                email: None,
                provider_account_id: None,
                created_at: now,
                updated_at: now,
                last_authenticated_at: Some(now),
            }],
            ..Config::default()
        };

        let accounts = discover_accounts(&config);

        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }

        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].label, "user@example.com");
        assert_eq!(accounts[0].email.as_deref(), Some("user@example.com"));
        assert_eq!(accounts[0].provider_account_id.as_deref(), Some("acct-123"));
    }
}
