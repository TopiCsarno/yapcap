// SPDX-License-Identifier: MPL-2.0

use super::refresh::load_account_status;
use crate::auth::{email_from_claude_credentials, load_claude_auth_from_config_dir};
use crate::config::{Config, ManagedClaudeAccountConfig, paths};
use chrono::Utc;
use dirs::home_dir;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const CLAUDE_CREDENTIALS_FILE: &str = ".credentials.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeAccount {
    pub id: String,
    pub label: String,
    pub email: Option<String>,
    pub organization: Option<String>,
    pub subscription_type: Option<String>,
    pub config_dir: PathBuf,
}

pub fn discover_accounts(config: &Config) -> Vec<ClaudeAccount> {
    let mut accounts = Vec::new();
    for managed in &config.claude_managed_accounts {
        let Ok(auth) = load_claude_auth_from_config_dir(&managed.config_dir) else {
            continue;
        };
        let email = managed.email.clone();
        let discovered = ClaudeAccount {
            id: managed.id.clone(),
            label: email.clone().unwrap_or_else(|| managed.label.clone()),
            email,
            organization: managed.organization.clone(),
            subscription_type: auth
                .subscription_type
                .or_else(|| managed.subscription_type.clone()),
            config_dir: managed.config_dir.clone(),
        };
        match discovered.email.as_deref().map(normalized_email) {
            Some(email_key) => {
                if let Some(index) = accounts.iter().position(|existing: &ClaudeAccount| {
                    existing.email.as_deref().map(normalized_email) == Some(email_key.clone())
                }) {
                    let existing = &accounts[index];
                    if prefer_account(
                        existing,
                        &discovered,
                        config.active_claude_account_id.as_deref(),
                    ) {
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

pub fn apply_login_account(config: &mut Config, account: ManagedClaudeAccountConfig) {
    let account_id = account.id.clone();
    config.active_claude_account_id = Some(account_id.clone());
    config
        .claude_managed_accounts
        .retain(|existing| existing.id != account_id);
    config.claude_managed_accounts.push(account);
    dedupe_managed_accounts(config);
}

pub fn sync_imported_account(config: &mut Config) -> Result<bool, String> {
    let mut changed = false;
    changed |= recover_orphan_managed_claude_dirs(config)?;
    changed |= import_external_claude_config(config)?;
    if changed {
        dedupe_managed_accounts(config);
    }
    changed |= ensure_single_claude_active(config);
    Ok(changed)
}

pub fn sync_managed_accounts(config: &mut Config) -> bool {
    let mut changed = false;
    for account in &mut config.claude_managed_accounts {
        let status = load_account_status(&account.config_dir).ok();

        if let Some(ref status) = status {
            if let Some(e) = status.email.as_ref().filter(|e| !e.is_empty())
                && account.email.as_ref() != Some(e)
            {
                account.email = Some(e.clone());
                changed = true;
            }
            if status.organization.is_some() && account.organization != status.organization {
                account.organization.clone_from(&status.organization);
                changed = true;
            }
            if status.subscription_type.is_some()
                && account.subscription_type != status.subscription_type
            {
                account
                    .subscription_type
                    .clone_from(&status.subscription_type);
                changed = true;
            }
        }

        let mut email_from_credentials = false;
        if account.email.as_deref().is_none_or(str::is_empty)
            && let Ok(auth) = load_claude_auth_from_config_dir(&account.config_dir)
            && let Some(email) = email_from_claude_credentials(&auth)
        {
            account.email = Some(email);
            email_from_credentials = true;
            changed = true;
        }

        let mut email_from_usage_api = false;
        if account.email.as_deref().is_none_or(str::is_empty)
            && let Some(email) = super::blocking_fetch_usage_email(&account.config_dir)
        {
            account.email = Some(email);
            email_from_usage_api = true;
            changed = true;
        }

        if let Some(email) = account.email.as_ref() {
            let should_sync_label = status
                .as_ref()
                .and_then(|s| s.email.as_deref())
                .is_some_and(|e| !e.is_empty())
                || email_from_credentials
                || email_from_usage_api;
            if should_sync_label && account.label != *email {
                account.label.clone_from(email);
                changed = true;
            }
        }

        let had_extra_files = fs::read_dir(&account.config_dir)
            .ok()
            .is_some_and(|entries| {
                entries.flatten().any(|entry| {
                    entry
                        .file_name()
                        .to_str()
                        .is_none_or(|name| name != CLAUDE_CREDENTIALS_FILE)
                })
            });
        if had_extra_files && prune_managed_claude_config(&account.config_dir).is_ok() {
            changed = true;
        }
    }
    dedupe_managed_accounts(config) || changed
}

fn find_managed_with_same_access_token<'a>(
    config: &'a Config,
    access_token: &str,
) -> Option<&'a ManagedClaudeAccountConfig> {
    config.claude_managed_accounts.iter().find(|account| {
        load_claude_auth_from_config_dir(&account.config_dir)
            .ok()
            .is_some_and(|auth| auth.access_token == access_token)
    })
}

fn recover_orphan_managed_claude_dirs(config: &mut Config) -> Result<bool, String> {
    let root = paths().claude_accounts_dir;
    if !root.exists() {
        return Ok(false);
    }
    let known_ids: HashSet<String> = config
        .claude_managed_accounts
        .iter()
        .map(|a| a.id.clone())
        .collect();
    let known_dirs: HashSet<PathBuf> = config
        .claude_managed_accounts
        .iter()
        .filter_map(|a| a.config_dir.canonicalize().ok())
        .collect();

    let mut changed = false;
    for entry in fs::read_dir(&root)
        .map_err(|error| format!("failed to read {}: {error}", root.display()))?
    {
        let entry = entry.map_err(|error| format!("failed to read {}: {error}", root.display()))?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with("claude-") || name_str.starts_with("pending-") {
            continue;
        }
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let meta = fs::symlink_metadata(&path)
            .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if known_ids.contains(name_str.as_ref()) {
            continue;
        }
        if path
            .canonicalize()
            .ok()
            .is_some_and(|c| known_dirs.contains(&c))
        {
            continue;
        }
        let Ok(auth) = load_claude_auth_from_config_dir(&path) else {
            continue;
        };
        if find_managed_with_same_access_token(config, &auth.access_token).is_some() {
            continue;
        }

        let status = load_account_status(&path).ok();
        let mut email = status.as_ref().and_then(|s| s.email.clone());
        if email.is_none() {
            email = email_from_claude_credentials(&auth);
        }
        if find_matching_account(config, email.as_deref()).is_some() {
            continue;
        }

        let organization = status.as_ref().and_then(|s| s.organization.clone());
        let subscription_type = status
            .as_ref()
            .and_then(|s| s.subscription_type.clone())
            .or_else(|| auth.subscription_type.clone());
        let label = email
            .clone()
            .unwrap_or_else(|| "Claude account".to_string());
        let now = Utc::now();
        config
            .claude_managed_accounts
            .push(ManagedClaudeAccountConfig {
                id: name_str.to_string(),
                label,
                config_dir: path,
                email,
                organization,
                subscription_type,
                created_at: now,
                updated_at: now,
                last_authenticated_at: Some(now),
            });
        changed = true;
    }
    Ok(changed)
}

fn import_external_claude_config(config: &mut Config) -> Result<bool, String> {
    const PROFILE_SCOPE: &str = "user:profile";

    let Some(source) = external_claude_config_dir_candidate() else {
        return Ok(false);
    };
    if !source.exists() {
        return Ok(false);
    }
    let managed_root = paths().claude_accounts_dir;
    if is_path_within(&source, &managed_root) {
        return Ok(false);
    }
    let Ok(auth) = load_claude_auth_from_config_dir(&source) else {
        return Ok(false);
    };
    if !auth.scopes.iter().any(|scope| scope == PROFILE_SCOPE) {
        return Ok(false);
    }

    if let Some(existing) = find_managed_with_same_access_token(config, &auth.access_token) {
        if config.active_claude_account_id.is_none() {
            config.active_claude_account_id = Some(existing.id.clone());
            return Ok(true);
        }
        return Ok(false);
    }

    let status = load_account_status(&source).ok();
    let email = status
        .as_ref()
        .and_then(|s| s.email.clone())
        .or_else(|| email_from_claude_credentials(&auth));

    create_private_dir(&managed_root)?;
    if let Some(existing) = find_matching_account(config, email.as_deref()).cloned() {
        if load_claude_auth_from_config_dir(&existing.config_dir).is_ok() {
            if config.active_claude_account_id.is_none() {
                config.active_claude_account_id = Some(existing.id.clone());
                return Ok(true);
            }
            return Ok(false);
        }

        let import_id = new_account_id();
        let pending_dir = managed_root.join(format!("pending-import-{import_id}"));
        copy_minimal_claude_config(&source, &pending_dir)?;
        commit_pending_dir(&managed_root, &pending_dir, &existing.config_dir)?;

        let now = Utc::now();
        apply_login_account(
            config,
            ManagedClaudeAccountConfig {
                id: existing.id,
                label: email.clone().unwrap_or_else(|| existing.label.clone()),
                config_dir: existing.config_dir,
                email,
                organization: status
                    .as_ref()
                    .and_then(|s| s.organization.clone())
                    .or(existing.organization),
                subscription_type: status
                    .as_ref()
                    .and_then(|s| s.subscription_type.clone())
                    .or(existing.subscription_type),
                created_at: existing.created_at,
                updated_at: now,
                last_authenticated_at: Some(now),
            },
        );
        return Ok(true);
    }

    let account_id = new_account_id();
    let pending_dir = managed_root.join(format!("pending-import-{account_id}"));
    let target_dir = managed_root.join(&account_id);
    copy_minimal_claude_config(&source, &pending_dir)?;
    commit_pending_dir(&managed_root, &pending_dir, &target_dir)?;

    let now = Utc::now();
    apply_login_account(
        config,
        ManagedClaudeAccountConfig {
            id: account_id.clone(),
            label: email
                .clone()
                .unwrap_or_else(|| "Claude account".to_string()),
            config_dir: target_dir,
            email,
            organization: status.as_ref().and_then(|s| s.organization.clone()),
            subscription_type: status
                .as_ref()
                .and_then(|s| s.subscription_type.clone())
                .or_else(|| auth.subscription_type.clone()),
            created_at: now,
            updated_at: now,
            last_authenticated_at: Some(now),
        },
    );
    Ok(true)
}

fn ensure_single_claude_active(config: &mut Config) -> bool {
    if config.claude_managed_accounts.len() != 1 {
        return false;
    }
    let id = config.claude_managed_accounts[0].id.clone();
    if config.active_claude_account_id.as_deref() == Some(id.as_str()) {
        return false;
    }
    config.active_claude_account_id = Some(id);
    true
}

fn external_claude_config_dir_candidate() -> Option<PathBuf> {
    if let Some(raw) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        let trimmed = PathBuf::from(&raw);
        if trimmed.as_os_str().is_empty() {
            return home_dir().map(|home| home.join(".claude"));
        }
        return Some(trimmed);
    }
    home_dir().map(|home| home.join(".claude"))
}

pub(crate) fn prune_managed_claude_config(config_dir: &Path) -> Result<(), String> {
    let credentials_path = config_dir.join(CLAUDE_CREDENTIALS_FILE);
    let credentials = fs::read(&credentials_path)
        .map_err(|error| format!("failed to read {}: {error}", credentials_path.display()))?;

    for entry in fs::read_dir(config_dir)
        .map_err(|error| format!("failed to read {}: {error}", config_dir.display()))?
    {
        let entry =
            entry.map_err(|error| format!("failed to read {}: {error}", config_dir.display()))?;
        let path = entry.path();
        if path
            .file_name()
            .is_some_and(|name| name == CLAUDE_CREDENTIALS_FILE)
        {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
        if file_type.is_dir() {
            fs::remove_dir_all(&path)
                .map_err(|error| format!("failed to remove {}: {error}", path.display()))?;
        } else {
            fs::remove_file(&path)
                .map_err(|error| format!("failed to remove {}: {error}", path.display()))?;
        }
    }

    fs::write(&credentials_path, credentials)
        .map_err(|error| format!("failed to write {}: {error}", credentials_path.display()))
}

fn copy_minimal_claude_config(source: &Path, target: &Path) -> Result<(), String> {
    create_private_dir(target)?;
    let source_credentials = source.join(CLAUDE_CREDENTIALS_FILE);
    let target_credentials = target.join(CLAUDE_CREDENTIALS_FILE);
    fs::copy(&source_credentials, &target_credentials).map_err(|error| {
        format!(
            "failed to copy {} to {}: {error}",
            source_credentials.display(),
            target_credentials.display()
        )
    })?;
    Ok(())
}

fn is_path_within(path: &Path, root: &Path) -> bool {
    let Ok(path) = path.canonicalize() else {
        return false;
    };
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    path.starts_with(&root)
}

pub fn dedupe_managed_accounts(config: &mut Config) -> bool {
    let original_active = config.active_claude_account_id.clone();
    let mut active_id = original_active.clone();
    let original_len = config.claude_managed_accounts.len();
    let mut deduped = Vec::new();

    for account in config.claude_managed_accounts.drain(..) {
        let Some(email_key) = account.email.as_deref().map(normalized_email) else {
            deduped.push(account);
            continue;
        };

        if let Some(index) = deduped
            .iter()
            .position(|existing: &ManagedClaudeAccountConfig| {
                existing.email.as_deref().map(normalized_email) == Some(email_key.clone())
            })
        {
            let existing = deduped.remove(index);
            let keep_existing =
                prefer_managed_account(&existing, &account, original_active.as_deref());
            let (mut winner, loser) = if keep_existing {
                (existing, account)
            } else {
                (account, existing)
            };
            let loser_id = loser.id.clone();
            let winner_id = winner.id.clone();
            merge_account_metadata(&mut winner, &loser);
            if active_id.as_deref() == Some(loser_id.as_str()) {
                active_id = Some(winner_id);
            }
            deduped.push(winner);
            continue;
        }

        deduped.push(account);
    }

    let changed = deduped.len() != original_len || active_id != original_active;
    config.claude_managed_accounts = deduped;
    config.active_claude_account_id = active_id;
    changed
}

pub fn remove_managed_config_dir(config_dir: &Path) {
    let root = paths().claude_accounts_dir;
    let Ok(root) = root.canonicalize() else {
        return;
    };
    let Ok(metadata) = fs::symlink_metadata(config_dir) else {
        return;
    };
    if metadata.file_type().is_symlink() {
        tracing::warn!(path = %config_dir.display(), "refusing to delete symlinked claude account config dir");
        return;
    }
    let Ok(config_dir) = config_dir.canonicalize() else {
        return;
    };
    if !config_dir.starts_with(&root) {
        tracing::warn!(path = %config_dir.display(), root = %root.display(), "refusing to delete claude account outside managed root");
        return;
    }
    if let Err(error) = fs::remove_dir_all(&config_dir) {
        tracing::warn!(path = %config_dir.display(), error = %error, "failed to delete claude account config dir");
    }
}

pub(crate) fn find_matching_account<'a>(
    config: &'a Config,
    email: Option<&str>,
) -> Option<&'a ManagedClaudeAccountConfig> {
    let email = email?;
    let email = normalized_email(email);
    config
        .claude_managed_accounts
        .iter()
        .find(|account| account.email.as_deref().map(normalized_email) == Some(email.clone()))
}

pub(crate) fn managed_config_dir(account_id: &str) -> PathBuf {
    paths().claude_accounts_dir.join(account_id)
}

pub(crate) fn create_private_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path)
        .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
    set_private_dir_permissions(path)?;
    Ok(())
}

pub(crate) fn new_account_id() -> String {
    let millis = Utc::now().timestamp_millis();
    format!("claude-{millis}-{}", std::process::id())
}

pub(crate) fn commit_pending_dir(
    managed_root: &Path,
    pending_dir: &Path,
    stable_dir: &Path,
) -> Result<(), String> {
    validate_managed_stable_dir(managed_root, stable_dir)?;
    if !stable_dir.exists() {
        return fs::rename(pending_dir, stable_dir)
            .map_err(|error| format!("failed to commit Claude account: {error}"));
    }

    let backup_dir = stable_dir.with_file_name(format!(
        ".{}.backup-{}",
        stable_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("claude-account"),
        Utc::now().timestamp_millis()
    ));

    fs::rename(stable_dir, &backup_dir)
        .map_err(|error| format!("failed to stage existing Claude account: {error}"))?;
    if let Err(error) = fs::rename(pending_dir, stable_dir) {
        let _ = fs::rename(&backup_dir, stable_dir);
        return Err(format!("failed to commit Claude account: {error}"));
    }
    if let Err(error) = fs::remove_dir_all(&backup_dir) {
        tracing::warn!(path = %backup_dir.display(), error = %error, "failed to remove old Claude account backup");
    }
    Ok(())
}

pub(crate) fn normalized_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

fn prefer_account(
    existing: &ClaudeAccount,
    candidate: &ClaudeAccount,
    active_id: Option<&str>,
) -> bool {
    if active_id == Some(existing.id.as_str()) {
        return true;
    }
    if active_id == Some(candidate.id.as_str()) {
        return false;
    }
    existing.id >= candidate.id
}

fn prefer_managed_account(
    existing: &ManagedClaudeAccountConfig,
    candidate: &ManagedClaudeAccountConfig,
    active_id: Option<&str>,
) -> bool {
    if active_id == Some(existing.id.as_str()) {
        return true;
    }
    if active_id == Some(candidate.id.as_str()) {
        return false;
    }
    let existing_auth = existing.last_authenticated_at.or(Some(existing.updated_at));
    let candidate_auth = candidate
        .last_authenticated_at
        .or(Some(candidate.updated_at));
    existing_auth >= candidate_auth
}

fn merge_account_metadata(
    target: &mut ManagedClaudeAccountConfig,
    source: &ManagedClaudeAccountConfig,
) {
    if target.email.is_none() {
        target.email.clone_from(&source.email);
    }
    if target.organization.is_none() {
        target.organization.clone_from(&source.organization);
    }
    if target.subscription_type.is_none() {
        target
            .subscription_type
            .clone_from(&source.subscription_type);
    }
    if target.email.is_none() && source.email.is_some() {
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

fn validate_managed_stable_dir(managed_root: &Path, stable_dir: &Path) -> Result<(), String> {
    if stable_dir
        .symlink_metadata()
        .is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return Err("refusing to replace symlinked Claude account config dir".to_string());
    }

    let root = managed_root
        .canonicalize()
        .map_err(|error| format!("failed to resolve Claude account root: {error}"))?;
    let stable = if stable_dir.exists() {
        stable_dir
            .canonicalize()
            .map_err(|error| format!("failed to resolve Claude account dir: {error}"))?
    } else {
        let parent = stable_dir
            .parent()
            .ok_or_else(|| "Claude account dir has no parent".to_string())?
            .canonicalize()
            .map_err(|error| format!("failed to resolve Claude account parent: {error}"))?;
        parent.join(
            stable_dir
                .file_name()
                .ok_or_else(|| "Claude account dir has no file name".to_string())?,
        )
    };
    if stable == root || !stable.starts_with(&root) {
        return Err("refusing to replace Claude account outside managed root".to_string());
    }
    Ok(())
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = fs::Permissions::from_mode(0o700);
    fs::set_permissions(path, permissions)
        .map_err(|error| format!("failed to secure {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::load_claude_auth_from_config_dir;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("yapcap-{name}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_credentials(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join(".credentials.json"),
            r#"{
  "claudeAiOauth": {
    "accessToken": "tok-import",
    "expiresAt": 1776609779660,
    "scopes": ["user:profile"],
    "subscriptionType": "pro"
  }
}"#,
        )
        .unwrap();
    }

    fn managed_account(id: &str, email: Option<&str>) -> ManagedClaudeAccountConfig {
        let now = Utc::now();
        ManagedClaudeAccountConfig {
            id: id.to_string(),
            label: "Claude account".to_string(),
            config_dir: PathBuf::from(format!("/tmp/{id}")),
            email: email.map(str::to_string),
            organization: None,
            subscription_type: None,
            created_at: now,
            updated_at: now,
            last_authenticated_at: Some(now),
        }
    }

    #[test]
    fn dedupes_managed_accounts_by_email() {
        let mut config = Config {
            active_claude_account_id: Some("claude-1".to_string()),
            claude_managed_accounts: vec![
                managed_account("claude-1", Some("user@example.com")),
                managed_account("claude-2", Some("USER@example.com")),
            ],
            ..Config::default()
        };

        let changed = dedupe_managed_accounts(&mut config);

        assert!(changed);
        assert_eq!(config.claude_managed_accounts.len(), 1);
        assert_eq!(config.active_claude_account_id.as_deref(), Some("claude-1"));
    }

    #[test]
    fn recovers_managed_claude_dir_when_config_empty() {
        let _guard = env_lock().lock().unwrap();
        let state_root = temp_dir("claude-recover-state");
        let skip_external = temp_dir("claude-recover-no-external");
        unsafe {
            std::env::set_var("CLAUDE_CONFIG_DIR", &skip_external);
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }

        let account_dir = paths().claude_accounts_dir.join("claude-recover-test");
        write_credentials(&account_dir);

        let mut config = Config::default();
        let changed = sync_imported_account(&mut config).unwrap();

        unsafe {
            std::env::remove_var("CLAUDE_CONFIG_DIR");
            std::env::remove_var("XDG_STATE_HOME");
        }

        assert!(changed);
        assert_eq!(config.claude_managed_accounts.len(), 1);
        assert_eq!(config.claude_managed_accounts[0].id, "claude-recover-test");
        assert_eq!(
            config.active_claude_account_id.as_deref(),
            Some("claude-recover-test")
        );
        assert!(load_claude_auth_from_config_dir(&account_dir).is_ok());
    }

    #[test]
    fn imports_external_claude_config_into_managed_storage() {
        let _guard = env_lock().lock().unwrap();
        let source = temp_dir("claude-import-source");
        write_credentials(&source);
        fs::create_dir_all(source.join(".git")).unwrap();
        fs::write(source.join(".git").join("config"), "junk").unwrap();
        fs::write(source.join("extra.txt"), "hello").unwrap();
        let state_root = temp_dir("claude-import-state");

        unsafe {
            std::env::set_var("CLAUDE_CONFIG_DIR", &source);
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }

        let mut config = Config::default();
        let changed = sync_imported_account(&mut config).unwrap();

        unsafe {
            std::env::remove_var("CLAUDE_CONFIG_DIR");
            std::env::remove_var("XDG_STATE_HOME");
        }

        assert!(changed);
        assert_eq!(config.claude_managed_accounts.len(), 1);
        let account = &config.claude_managed_accounts[0];
        assert_eq!(
            config.active_claude_account_id.as_deref(),
            Some(account.id.as_str())
        );
        assert!(account.config_dir.join(".credentials.json").exists());
        assert!(!account.config_dir.join("extra.txt").exists());
        assert!(!account.config_dir.join(".git").exists());
    }

    #[test]
    fn sync_managed_accounts_fills_email_from_access_token_when_missing() {
        use base64::Engine;

        let _guard = env_lock().lock().unwrap();
        let dir = temp_dir("claude-sync-email-from-jwt");
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("{}");
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"email":"jwt@example.com"}"#);
        let jwt = format!("{header}.{payload}.sig");
        fs::write(
            dir.join(".credentials.json"),
            format!(
                r#"{{"claudeAiOauth":{{"accessToken":"{jwt}","expiresAt":1776609779660,"scopes":["user:profile"],"subscriptionType":"pro"}}}}"#
            ),
        )
        .unwrap();

        let now = Utc::now();
        let mut config = Config {
            claude_managed_accounts: vec![ManagedClaudeAccountConfig {
                id: "claude-test".to_string(),
                label: "Claude account".to_string(),
                config_dir: dir,
                email: None,
                organization: None,
                subscription_type: None,
                created_at: now,
                updated_at: now,
                last_authenticated_at: Some(now),
            }],
            ..Config::default()
        };

        assert!(sync_managed_accounts(&mut config));
        assert_eq!(
            config.claude_managed_accounts[0].email.as_deref(),
            Some("jwt@example.com")
        );
        assert_eq!(config.claude_managed_accounts[0].label, "jwt@example.com");
    }

    #[test]
    fn repeated_external_import_does_not_add_duplicate_account() {
        use base64::Engine;

        let _guard = env_lock().lock().unwrap();
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("{}");
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"email":"same@example.com"}"#);
        let jwt = format!("{header}.{payload}.sig");

        let source = temp_dir("claude-dup-source");
        fs::write(
            source.join(".credentials.json"),
            format!(
                r#"{{"claudeAiOauth":{{"accessToken":"{jwt}","expiresAt":1776609779660,"scopes":["user:profile"],"subscriptionType":"pro"}}}}"#
            ),
        )
        .unwrap();

        let state_root = temp_dir("claude-dup-state");
        unsafe {
            std::env::set_var("CLAUDE_CONFIG_DIR", &source);
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }

        let mut config = Config::default();
        assert!(sync_imported_account(&mut config).unwrap());
        assert_eq!(config.claude_managed_accounts.len(), 1);
        let first_id = config.claude_managed_accounts[0].id.clone();

        assert!(!sync_imported_account(&mut config).unwrap());
        assert_eq!(config.claude_managed_accounts.len(), 1);
        assert_eq!(config.claude_managed_accounts[0].id, first_id);

        unsafe {
            std::env::remove_var("CLAUDE_CONFIG_DIR");
            std::env::remove_var("XDG_STATE_HOME");
        }
    }

    #[test]
    fn prune_managed_claude_config_keeps_only_credentials() {
        let dir = temp_dir("claude-prune");
        write_credentials(&dir);
        fs::write(dir.join("extra.txt"), "hello").unwrap();
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::write(dir.join(".git").join("config"), "junk").unwrap();

        prune_managed_claude_config(&dir).unwrap();

        assert!(dir.join(".credentials.json").exists());
        assert!(!dir.join("extra.txt").exists());
        assert!(!dir.join(".git").exists());
    }
}
