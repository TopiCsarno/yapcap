// SPDX-License-Identifier: MPL-2.0

use crate::auth::{codex_home, email_from_id_token, load_codex_auth_from_home};
use crate::config::{Config, ManagedCodexAccountConfig, paths};
use chrono::Utc;
use std::fs;
use std::path::{Path, PathBuf};

const AUTH_FILE_NAME: &str = "auth.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexAccount {
    pub id: String,
    pub label: String,
    pub email: Option<String>,
    pub provider_account_id: Option<String>,
    pub codex_home: PathBuf,
}

pub fn discover_accounts(config: &Config) -> Vec<CodexAccount> {
    let mut accounts = Vec::new();
    for managed in &config.codex_managed_accounts {
        let Ok(auth) = load_codex_auth_from_home(&managed.codex_home) else {
            continue;
        };
        let email = auth
            .id_token
            .as_deref()
            .and_then(email_from_id_token)
            .or_else(|| managed.email.clone());
        let label = email.clone().unwrap_or_else(|| "Codex account".to_string());
        let discovered = CodexAccount {
            id: managed.id.clone(),
            label,
            email,
            provider_account_id: managed.provider_account_id.clone(),
            codex_home: managed.codex_home.clone(),
        };
        match discovered.email.as_deref().map(normalized_email) {
            Some(email_key) => {
                if let Some(index) = accounts.iter().position(|existing: &CodexAccount| {
                    existing.email.as_deref().map(normalized_email) == Some(email_key.clone())
                }) {
                    let existing = &accounts[index];
                    if prefer_account(
                        existing,
                        &discovered,
                        config.active_codex_account_id.as_deref(),
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

pub fn sync_imported_account(config: &mut Config) -> Result<bool, String> {
    let mut changed = dedupe_managed_accounts(config);
    let prefer_imported = config.active_codex_account_id.as_deref() == Some("system")
        || config.active_codex_account_id.is_none();

    let Ok(source_home) = codex_home() else {
        if config.active_codex_account_id.as_deref() == Some("system") {
            config.active_codex_account_id = None;
            return Ok(true);
        }
        return Ok(changed);
    };

    let Ok(source_auth) = load_codex_auth_from_home(&source_home) else {
        if config.active_codex_account_id.as_deref() == Some("system") {
            config.active_codex_account_id = None;
            return Ok(true);
        }
        return Ok(changed);
    };

    let managed_root = paths().codex_accounts_dir;
    if is_path_within(&source_home, &managed_root) {
        if config.active_codex_account_id.as_deref() == Some("system") {
            config.active_codex_account_id = None;
            changed = true;
        }
        return Ok(changed);
    }

    let email = source_auth
        .id_token
        .as_deref()
        .and_then(email_from_id_token);

    create_private_dir(&managed_root)?;
    if let Some(existing) = find_matching_account(config, email.as_deref()).cloned() {
        if load_codex_auth_from_home(&existing.codex_home).is_ok() {
            if prefer_imported
                && config.active_codex_account_id.as_deref() != Some(existing.id.as_str())
            {
                config.active_codex_account_id = Some(existing.id);
                changed = true;
            }
            return Ok(changed);
        }

        let pending_home = pending_import_home(&managed_root);
        copy_minimal_codex_home(&source_home, &pending_home)?;
        commit_pending_home(&pending_home, &existing.codex_home)?;

        let now = Utc::now();
        apply_login_account(
            config,
            ManagedCodexAccountConfig {
                id: existing.id,
                label: email.clone().unwrap_or_else(|| existing.label.clone()),
                codex_home: existing.codex_home,
                email,
                provider_account_id: source_auth.account_id,
                created_at: existing.created_at,
                updated_at: now,
                last_authenticated_at: Some(now),
            },
        );
        return Ok(true);
    }

    let account_id = new_account_id();
    let pending_home = pending_import_home(&managed_root);
    let target_home = managed_home(&account_id);
    copy_minimal_codex_home(&source_home, &pending_home)?;
    commit_pending_home(&pending_home, &target_home)?;

    let now = Utc::now();
    apply_login_account(
        config,
        ManagedCodexAccountConfig {
            id: account_id.clone(),
            label: email.clone().unwrap_or_else(|| "Codex account".to_string()),
            codex_home: target_home,
            email,
            provider_account_id: source_auth.account_id,
            created_at: now,
            updated_at: now,
            last_authenticated_at: Some(now),
        },
    );
    Ok(true)
}

pub fn apply_login_account(config: &mut Config, account: ManagedCodexAccountConfig) {
    let account_id = account.id.clone();
    config.active_codex_account_id = Some(account_id.clone());
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

pub(crate) fn managed_home(account_id: &str) -> PathBuf {
    paths().codex_accounts_dir.join(account_id)
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

fn pending_import_home(root: &Path) -> PathBuf {
    root.join(format!("pending-import-{}", new_account_id()))
}

pub(crate) fn commit_pending_home(pending_home: &Path, target_home: &Path) -> Result<(), String> {
    if !target_home.exists() {
        fs::rename(pending_home, target_home)
            .map_err(|error| format!("failed to commit Codex account: {error}"))?;
        return Ok(());
    }

    let backup_home = replacement_backup_path(target_home);
    fs::rename(target_home, &backup_home).map_err(|error| {
        format!("failed to prepare existing Codex account replacement: {error}")
    })?;
    if let Err(error) = fs::rename(pending_home, target_home) {
        let _ = fs::rename(&backup_home, target_home);
        return Err(format!("failed to replace Codex account: {error}"));
    }
    let _ = fs::remove_dir_all(&backup_home);
    Ok(())
}

pub(crate) fn replacement_backup_path(target_home: &Path) -> PathBuf {
    let name = target_home
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("codex-account");
    target_home.with_file_name(format!(
        "{name}.replacing-{}",
        Utc::now().timestamp_millis()
    ))
}

fn dedupe_managed_accounts(config: &mut Config) -> bool {
    let original_active = config.active_codex_account_id.clone();
    let mut active_id = original_active.clone();
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

    if active_id.as_deref() == Some("system") {
        active_id = None;
    }
    let changed = deduped.len() != original_len || active_id != original_active;
    config.codex_managed_accounts = deduped;
    config.active_codex_account_id = active_id;
    changed
}

fn prefer_account(
    existing: &CodexAccount,
    candidate: &CodexAccount,
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
    existing: &ManagedCodexAccountConfig,
    candidate: &ManagedCodexAccountConfig,
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

fn copy_minimal_codex_home(source: &Path, target: &Path) -> Result<(), String> {
    create_private_dir(target)?;
    let source_auth = source.join(AUTH_FILE_NAME);
    let target_auth = target.join(AUTH_FILE_NAME);
    fs::copy(&source_auth, &target_auth).map_err(|error| {
        format!(
            "failed to copy {} to {}: {error}",
            source_auth.display(),
            target_auth.display()
        )
    })?;
    Ok(())
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

fn is_path_within(path: &Path, root: &Path) -> bool {
    let Ok(path) = path.canonicalize() else {
        return false;
    };
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    path.starts_with(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("yapcap-{name}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn id_token(email: &str) -> String {
        let payload = format!(r#"{{"email":"{email}"}}"#);
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload);
        format!("x.{payload}.y")
    }

    fn write_auth(home: &Path, account_id: &str, email: &str) {
        fs::create_dir_all(home).unwrap();
        fs::write(
            home.join("auth.json"),
            format!(
                r#"{{"tokens":{{"access_token":"access","account_id":"{account_id}","id_token":"{}"}}}}"#,
                id_token(email)
            ),
        )
        .unwrap();
    }

    #[test]
    fn imports_external_codex_home_into_managed_storage() {
        let _guard = env_lock().lock().unwrap();
        let home = temp_dir("codex-import-source");
        write_auth(&home, "acct-123", "user@example.com");
        let state_root = temp_dir("codex-import-state");

        unsafe {
            std::env::set_var("CODEX_HOME", &home);
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }

        let mut config = Config::default();
        let changed = sync_imported_account(&mut config).unwrap();

        unsafe {
            std::env::remove_var("CODEX_HOME");
            std::env::remove_var("XDG_STATE_HOME");
        }

        assert!(changed);
        assert_eq!(config.codex_managed_accounts.len(), 1);
        let account = &config.codex_managed_accounts[0];
        assert_eq!(
            config.active_codex_account_id.as_deref(),
            Some(account.id.as_str())
        );
        assert_eq!(account.provider_account_id.as_deref(), Some("acct-123"));
        assert_eq!(account.email.as_deref(), Some("user@example.com"));
        assert!(account.codex_home.join("auth.json").exists());
        assert!(!account.codex_home.join("extra.txt").exists());
    }

    #[test]
    fn reuses_existing_managed_account_for_same_email() {
        let _guard = env_lock().lock().unwrap();
        let home = temp_dir("codex-import-email-source");
        write_auth(&home, "acct-456", "user@example.com");
        let managed_home = temp_dir("codex-import-email-managed");
        write_auth(&managed_home, "acct-123", "user@example.com");

        unsafe {
            std::env::set_var("CODEX_HOME", &home);
        }

        let now = Utc::now();
        let mut config = Config::default();
        config
            .codex_managed_accounts
            .push(ManagedCodexAccountConfig {
                id: "codex-existing".to_string(),
                label: "user@example.com".to_string(),
                codex_home: managed_home,
                email: Some("user@example.com".to_string()),
                provider_account_id: Some("acct-123".to_string()),
                created_at: now,
                updated_at: now,
                last_authenticated_at: Some(now),
            });

        let changed = sync_imported_account(&mut config).unwrap();

        unsafe {
            std::env::remove_var("CODEX_HOME");
        }

        assert!(changed);
        assert_eq!(config.codex_managed_accounts.len(), 1);
        assert_eq!(config.codex_managed_accounts[0].id, "codex-existing");
        assert_eq!(
            config.active_codex_account_id.as_deref(),
            Some("codex-existing")
        );
    }

    #[test]
    fn repairs_existing_managed_account_when_home_was_cleared() {
        let _guard = env_lock().lock().unwrap();
        let home = temp_dir("codex-import-repair-source");
        write_auth(&home, "acct-456", "user@example.com");
        let state_root = temp_dir("codex-import-repair-state");
        let missing_managed_home = state_root.join("yapcap/codex-accounts/codex-existing");

        unsafe {
            std::env::set_var("CODEX_HOME", &home);
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }

        let now = Utc::now();
        let mut config = Config {
            active_codex_account_id: Some("codex-existing".to_string()),
            codex_managed_accounts: vec![ManagedCodexAccountConfig {
                id: "codex-existing".to_string(),
                label: "user@example.com".to_string(),
                codex_home: missing_managed_home.clone(),
                email: Some("user@example.com".to_string()),
                provider_account_id: Some("acct-123".to_string()),
                created_at: now,
                updated_at: now,
                last_authenticated_at: Some(now),
            }],
            ..Default::default()
        };

        let changed = sync_imported_account(&mut config).unwrap();

        unsafe {
            std::env::remove_var("CODEX_HOME");
            std::env::remove_var("XDG_STATE_HOME");
        }

        assert!(changed);
        assert_eq!(config.codex_managed_accounts.len(), 1);
        let account = &config.codex_managed_accounts[0];
        assert_eq!(account.id, "codex-existing");
        assert_eq!(account.provider_account_id.as_deref(), Some("acct-456"));
        assert!(account.codex_home.join("auth.json").exists());
        assert!(!account.codex_home.join("extra.txt").exists());
    }

    #[test]
    fn dedupes_existing_same_email_accounts() {
        let now = Utc::now();
        let home_a = temp_dir("codex-dedupe-a");
        let home_b = temp_dir("codex-dedupe-b");
        write_auth(&home_a, "acct-123", "user@example.com");
        write_auth(&home_b, "acct-999", "USER@example.com");

        let mut config = Config {
            active_codex_account_id: Some("codex-b".to_string()),
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
        assert_eq!(config.active_codex_account_id.as_deref(), Some("codex-b"));
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
        let home_a = temp_dir("codex-discover-a");
        let home_b = temp_dir("codex-discover-b");
        write_auth(&home_a, "acct-123", "user@example.com");
        write_auth(&home_b, "acct-999", "USER@example.com");

        let config = Config {
            active_codex_account_id: Some("codex-b".to_string()),
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
                    provider_account_id: Some("acct-999".to_string()),
                    created_at: now,
                    updated_at: now + chrono::TimeDelta::seconds(1),
                    last_authenticated_at: Some(now + chrono::TimeDelta::seconds(1)),
                },
            ],
            ..Config::default()
        };

        let accounts = discover_accounts(&config);

        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, "codex-b");
    }

    #[test]
    fn replacement_backup_path_keeps_target_parent() {
        let target = PathBuf::from("/tmp/yapcap/codex-123");
        let backup = replacement_backup_path(&target);

        assert_eq!(backup.parent(), target.parent());
        assert!(
            backup
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("codex-123.replacing-"))
        );
    }
}
