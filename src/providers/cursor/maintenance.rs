// SPDX-License-Identifier: MPL-2.0

use crate::config::{Config, CursorCredentialSource, ManagedCursorAccountConfig, paths};
use crate::providers::cursor::identity::{managed_account_id, managed_config_id, normalized_email};
use crate::providers::cursor::shared::new_account_id;
use crate::providers::cursor::storage::{
    account_metadata_path, create_private_dir, imported_cookie_header_path, managed_account_dir,
    profile_dir, remove_managed_profile, session_dir, stable_storage_id_from_normalized_email,
    write_account_metadata,
};
use std::fs;

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
        if prev.account_root != account.account_root {
            remove_managed_profile(&prev.account_root);
        }
    } else {
        if account.id.is_empty() {
            account.id = new_account_id();
        }
        account.account_root = managed_account_dir(&account.id);
    }
    config.active_cursor_account_id = Some(managed_account_id(&account.id));
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
    let original_active = config.active_cursor_account_id.clone();
    let mut active_id = original_active.clone();
    let mut deduped = Vec::new();

    for mut account in config.cursor_managed_accounts.drain(..) {
        account.email = normalized_email(&account.email);
        if account.email.is_empty() {
            remove_managed_profile(&account.account_root);
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
            let keep_existing =
                prefer_managed_account(&existing, &account, original_active.as_deref());
            let (mut winner, loser) = if keep_existing {
                (existing, account)
            } else {
                (account, existing)
            };
            if active_id.as_deref() == Some(managed_account_id(&loser.id).as_str()) {
                active_id = Some(managed_account_id(&winner.id));
            }
            merge_metadata(&mut winner, &loser);
            write_account_metadata(&winner).ok();
            deduped.push(winner);
            continue;
        }
        deduped.push(account);
    }

    deduped.sort_by(|left, right| left.email.cmp(&right.email));
    if let Some(raw) = active_id.clone()
        && let Some(key) = managed_config_id(&raw)
        && key.contains('@')
        && let Some(acc) = deduped.iter().find(|a| a.email == key)
    {
        active_id = Some(managed_account_id(&acc.id));
    }
    if active_id.as_deref().is_some_and(|id| {
        !deduped
            .iter()
            .any(|account| managed_account_id(&account.id) == id)
    }) {
        active_id = None;
    }

    let changed = deduped != original_accounts || active_id != original_active;
    config.cursor_managed_accounts = deduped;
    config.active_cursor_account_id = active_id;
    changed
}

pub fn cleanup_pending_dirs() {
    let root = paths().cursor_accounts_dir;
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with("pending-cursor-") {
            continue;
        }
        if let Err(error) = fs::remove_dir_all(&path) {
            tracing::debug!(path = %path.display(), error = %error, "failed to remove stale pending Cursor account dir");
        }
    }
}

fn normalize_account_layout(
    mut account: ManagedCursorAccountConfig,
) -> Option<ManagedCursorAccountConfig> {
    if account.id.is_empty() {
        account.id = stable_storage_id_from_normalized_email(&account.email);
    }
    let expected_root = managed_account_dir(&account.id);
    if imported_cookie_header_path(&account.account_root).is_file() {
        account.credential_source = CursorCredentialSource::ImportedBrowserProfile;
    }
    if is_valid_managed_account_dir(&account) {
        if account.account_root != expected_root {
            move_account_root(&account.account_root, &expected_root).ok()?;
            account.account_root = expected_root;
        }
        write_account_metadata(&account).ok()?;
        return Some(account);
    }

    if account.account_root.join("cursor_token").exists() {
        remove_managed_profile(&account.account_root);
        return None;
    }

    let legacy_profile_root = account
        .account_root
        .join("Default")
        .join("Cookies")
        .exists();
    if !legacy_profile_root {
        remove_managed_profile(&account.account_root);
        return None;
    }

    let browser = account.browser?;
    migrate_legacy_profile_root(&mut account, browser).ok()?;
    write_account_metadata(&account).ok()?;
    Some(account)
}

fn prefer_managed_account(
    existing: &ManagedCursorAccountConfig,
    candidate: &ManagedCursorAccountConfig,
    active_managed_id: Option<&str>,
) -> bool {
    let existing_id = managed_account_id(&existing.id);
    let candidate_id = managed_account_id(&candidate.id);
    if active_managed_id == Some(existing_id.as_str()) {
        return true;
    }
    if active_managed_id == Some(candidate_id.as_str()) {
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
    if target.browser.is_none() {
        target.browser = source.browser;
    }
}

fn is_valid_managed_account_dir(account: &ManagedCursorAccountConfig) -> bool {
    if !account_metadata_path(&account.account_root).exists()
        || !session_dir(&account.account_root).is_dir()
    {
        return false;
    }
    match account.credential_source {
        CursorCredentialSource::ManagedProfile => {
            account.browser.is_some()
                && profile_dir(&account.account_root)
                    .join("Default")
                    .join("Cookies")
                    .exists()
        }
        CursorCredentialSource::ImportedBrowserProfile => {
            imported_cookie_header_path(&account.account_root).is_file()
        }
    }
}

fn move_account_root(from: &std::path::Path, to: &std::path::Path) -> Result<(), String> {
    if from == to {
        return Ok(());
    }
    remove_managed_profile(to);
    if let Some(parent) = to.parent() {
        create_private_dir(parent)?;
    }
    fs::rename(from, to).map_err(|error| {
        format!(
            "failed to move {} to {}: {error}",
            from.display(),
            to.display()
        )
    })
}

fn migrate_legacy_profile_root(
    account: &mut ManagedCursorAccountConfig,
    browser: crate::config::Browser,
) -> Result<(), String> {
    if account.id.is_empty() {
        account.id = stable_storage_id_from_normalized_email(&account.email);
    }
    let source_root = account.account_root.clone();
    let target_root = managed_account_dir(&account.id);
    let temp_root = target_root.with_extension("migrating");

    if source_root == target_root {
        remove_managed_profile(&temp_root);
        fs::rename(&source_root, &temp_root).map_err(|error| {
            format!(
                "failed to move {} to {}: {error}",
                source_root.display(),
                temp_root.display()
            )
        })?;
        create_private_dir(&target_root)?;
        create_private_dir(&session_dir(&target_root))?;
        fs::rename(&temp_root, profile_dir(&target_root)).map_err(|error| {
            format!(
                "failed to move {} to {}: {error}",
                temp_root.display(),
                profile_dir(&target_root).display()
            )
        })?;
    } else {
        remove_managed_profile(&target_root);
        create_private_dir(&target_root)?;
        create_private_dir(&session_dir(&target_root))?;
        fs::rename(&source_root, profile_dir(&target_root)).map_err(|error| {
            format!(
                "failed to move {} to {}: {error}",
                source_root.display(),
                profile_dir(&target_root).display()
            )
        })?;
    }

    account.account_root = target_root;
    account.browser = Some(browser);
    account.credential_source = CursorCredentialSource::ManagedProfile;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Browser, Config};
    use chrono::Utc;
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
            credential_source: CursorCredentialSource::ImportedBrowserProfile,
            browser: Some(Browser::Brave),
            display_name: None,
            plan: None,
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }
    }

    #[test]
    fn upsert_keeps_one_account_per_email() {
        let mut config = Config::default();
        config
            .cursor_managed_accounts
            .push(account("User@example.com"));
        upsert_managed_account(&mut config, account("user@example.com"));
        assert_eq!(config.cursor_managed_accounts.len(), 1);
        let expected = stable_storage_id_from_normalized_email("user@example.com");
        assert_eq!(
            config.active_cursor_account_id.as_deref(),
            Some(managed_account_id(&expected).as_str())
        );
    }

    #[test]
    fn directory_naming_is_deterministic() {
        let email = normalized_email("User+test@example.com");
        let id = stable_storage_id_from_normalized_email(&email);
        assert_eq!(managed_account_dir(&id), managed_account_dir(&id));
    }

    #[test]
    fn sync_drops_legacy_token_only_accounts() {
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
                credential_source: CursorCredentialSource::ImportedBrowserProfile,
                browser: Some(Browser::Brave),
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
}
