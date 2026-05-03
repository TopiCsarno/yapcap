// SPDX-License-Identifier: MPL-2.0

use crate::account_storage::ProviderAccountStorage;
use crate::config::{Config, ManagedCursorAccountConfig, paths};
use crate::providers::cursor::identity::{managed_account_id, managed_config_id, normalized_email};
use crate::providers::cursor::storage::{
    managed_account_dir, new_account_id, stable_storage_id_from_normalized_email,
};

pub fn upsert_managed_account(
    config: &mut Config,
    mut account: ManagedCursorAccountConfig,
) -> ManagedCursorAccountConfig {
    account.email = normalized_email(&account.email);
    let previous = config
        .cursor_managed_accounts
        .iter()
        .find(|existing| existing.email == account.email)
        .cloned();
    if let Some(prev) = previous {
        if account.id.is_empty() {
            account.id = if prev.id.is_empty() {
                stable_storage_id_from_normalized_email(&account.email)
            } else {
                prev.id.clone()
            };
        }
        account.account_root = managed_account_dir(&account.id);
    } else {
        if account.id.is_empty() {
            account.id = new_account_id();
        }
        account.account_root = managed_account_dir(&account.id);
    }
    let new_managed_id = managed_account_id(&account.id);
    if !config.selected_cursor_account_ids.contains(&new_managed_id) {
        config.selected_cursor_account_ids.push(new_managed_id);
    }
    config
        .cursor_managed_accounts
        .retain(|existing| existing.email != account.email);
    config.cursor_managed_accounts.push(account.clone());
    config
        .cursor_managed_accounts
        .sort_by(|left, right| left.email.cmp(&right.email));
    account
}

pub fn sync_managed_accounts(config: &mut Config) -> bool {
    let original_accounts = config.cursor_managed_accounts.clone();
    let original_selected = config.selected_cursor_account_ids.clone();
    let mut selected_ids = original_selected.clone();
    let mut deduped = Vec::new();

    for mut account in config.cursor_managed_accounts.drain(..) {
        account.email = normalized_email(&account.email);
        if account.email.is_empty() {
            continue;
        }
        if account.id.is_empty() {
            account.id = stable_storage_id_from_normalized_email(&account.email);
        }
        if account.account_root.as_os_str().is_empty() {
            account.account_root = managed_account_dir(&account.id);
        }
        let Some(account) = normalize_account_layout(account) else {
            continue;
        };
        if let Some(index) = deduped
            .iter()
            .position(|existing: &ManagedCursorAccountConfig| existing.email == account.email)
        {
            let existing = deduped.remove(index);
            let keep_existing = prefer_managed_account(&existing, &account, &original_selected);
            let (mut winner, loser) = if keep_existing {
                (existing, account)
            } else {
                (account, existing)
            };
            let loser_managed_id = managed_account_id(&loser.id);
            let winner_managed_id = managed_account_id(&winner.id);
            for id in &mut selected_ids {
                if *id == loser_managed_id {
                    id.clone_from(&winner_managed_id);
                }
            }
            merge_metadata(&mut winner, &loser);
            deduped.push(winner);
            continue;
        }
        deduped.push(account);
    }

    deduped.sort_by(|left, right| left.email.cmp(&right.email));
    for id in &mut selected_ids {
        if let Some(key) = managed_config_id(id)
            && key.contains('@')
            && let Some(acc) = deduped.iter().find(|a| a.email == key)
        {
            *id = managed_account_id(&acc.id);
        }
    }
    selected_ids.retain(|id| {
        deduped
            .iter()
            .any(|account| managed_account_id(&account.id) == *id)
    });

    let changed = deduped != original_accounts || selected_ids != original_selected;
    config.cursor_managed_accounts = deduped;
    config.selected_cursor_account_ids = selected_ids;
    changed
}

fn normalize_account_layout(
    mut account: ManagedCursorAccountConfig,
) -> Option<ManagedCursorAccountConfig> {
    if account.id.is_empty() {
        account.id = stable_storage_id_from_normalized_email(&account.email);
    }
    account.account_root = managed_account_dir(&account.id);
    let storage = ProviderAccountStorage::new(paths().cursor_accounts_dir);
    let metadata = storage.load_metadata(&account.id).ok()?;
    if storage.load_tokens(&account.id).is_err() {
        return None;
    }
    let email = normalized_email(&metadata.email);
    if email.is_empty() {
        return None;
    }
    account.email.clone_from(&email);
    account.label = email;
    account.created_at = metadata.created_at;
    account.updated_at = metadata.updated_at;
    Some(account)
}

fn prefer_managed_account(
    existing: &ManagedCursorAccountConfig,
    candidate: &ManagedCursorAccountConfig,
    selected_managed_ids: &[String],
) -> bool {
    let existing_id = managed_account_id(&existing.id);
    let candidate_id = managed_account_id(&candidate.id);
    if selected_managed_ids
        .iter()
        .any(|id| id == existing_id.as_str())
    {
        return true;
    }
    if selected_managed_ids
        .iter()
        .any(|id| id == candidate_id.as_str())
    {
        return false;
    }
    existing.last_authenticated_at.or(Some(existing.updated_at))
        >= candidate
            .last_authenticated_at
            .or(Some(candidate.updated_at))
}

fn merge_metadata(target: &mut ManagedCursorAccountConfig, source: &ManagedCursorAccountConfig) {
    if target.display_name.is_none() {
        target.display_name.clone_from(&source.display_name);
    }
    if target.plan.is_none() {
        target.plan.clone_from(&source.plan);
    }
    if source.updated_at > target.updated_at {
        target.updated_at = source.updated_at;
    }
    if source.last_authenticated_at > target.last_authenticated_at {
        target.last_authenticated_at = source.last_authenticated_at;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_storage::{NewProviderAccount, ProviderAccountTokens};
    use crate::config::Config;
    use crate::model::ProviderId;
    use crate::test_support;
    use chrono::{Duration, Utc};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("yapcap-{name}-{nanos}"))
    }

    fn account(email: &str) -> ManagedCursorAccountConfig {
        let now = Utc::now();
        let email = normalized_email(email);
        let id = stable_storage_id_from_normalized_email(&email);
        ManagedCursorAccountConfig {
            id: id.clone(),
            email: email.clone(),
            label: email,
            account_root: managed_account_dir(&id),
            display_name: None,
            plan: None,
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }
    }

    fn store_account(account: &ManagedCursorAccountConfig) {
        ProviderAccountStorage::new(paths().cursor_accounts_dir)
            .replace_account(
                account.id.clone(),
                NewProviderAccount {
                    provider: ProviderId::Cursor,
                    email: account.email.clone(),
                    provider_account_id: None,
                    organization_id: None,
                    organization_name: None,
                    tokens: ProviderAccountTokens {
                        access_token: "WorkosCursorSessionToken=test".to_string(),
                        refresh_token: String::new(),
                        expires_at: Utc::now() + Duration::days(3650),
                        scope: Vec::new(),
                        token_id: None,
                    },
                    snapshot: None,
                },
            )
            .unwrap();
    }

    #[test]
    fn upsert_keeps_one_account_per_email() {
        let _guard = test_support::env_lock();
        let state_root = test_dir("cursor-upsert");
        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }

        let mut config = Config::default();
        let first = account("User@example.com");
        let second = account("user@example.com");
        store_account(&first);
        store_account(&second);
        config.cursor_managed_accounts.push(first);
        upsert_managed_account(&mut config, second);
        assert_eq!(config.cursor_managed_accounts.len(), 1);
        let expected = stable_storage_id_from_normalized_email("user@example.com");
        assert_eq!(
            config.selected_cursor_account_ids.as_slice(),
            [managed_account_id(&expected).as_str()]
        );

        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }
    }

    #[test]
    fn directory_naming_is_deterministic() {
        let email = normalized_email("User+test@example.com");
        let id = stable_storage_id_from_normalized_email(&email);
        assert_eq!(managed_account_dir(&id), managed_account_dir(&id));
    }

    #[test]
    fn sync_drops_legacy_token_only_accounts() {
        let _guard = test_support::env_lock();
        let state_root = test_dir("cursor-legacy-token");
        let legacy_root = state_root.join("yapcap/cursor-accounts/legacy");
        fs::create_dir_all(&legacy_root).unwrap();
        fs::write(legacy_root.join("cursor_token"), "cookie").unwrap();
        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }

        let mut config = Config::default();
        let now = Utc::now();
        config
            .cursor_managed_accounts
            .push(ManagedCursorAccountConfig {
                id: String::new(),
                email: "user@example.com".to_string(),
                label: "user@example.com".to_string(),
                account_root: legacy_root,
                display_name: None,
                plan: None,
                created_at: now,
                updated_at: now,
                last_authenticated_at: None,
            });

        assert!(sync_managed_accounts(&mut config));
        assert!(config.cursor_managed_accounts.is_empty());

        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }
    }

    #[test]
    fn sync_drops_malformed_account_dirs() {
        let _guard = test_support::env_lock();
        let state_root = test_dir("cursor-malformed");
        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }

        let mut config = Config::default();
        let entry = account("user@example.com");
        fs::create_dir_all(&entry.account_root).unwrap();
        config.cursor_managed_accounts.push(entry);

        assert!(sync_managed_accounts(&mut config));
        assert!(config.cursor_managed_accounts.is_empty());

        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }
    }

    #[test]
    fn sync_keeps_accounts_with_shared_storage_tokens() {
        let _guard = test_support::env_lock();
        let state_root = test_dir("cursor-shared-storage");
        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }

        let mut config = Config::default();
        let entry = account("user@example.com");
        store_account(&entry);
        config.cursor_managed_accounts.push(entry);

        sync_managed_accounts(&mut config);
        assert_eq!(config.cursor_managed_accounts.len(), 1);

        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }
    }
}
