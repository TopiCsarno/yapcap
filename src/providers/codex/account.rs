// SPDX-License-Identifier: MPL-2.0

use crate::account_selection::select_account_after_login;
use crate::account_storage::{
    AccountStorageError, ProviderAccountMetadata, ProviderAccountStorage, ProviderAccountTokens,
};
use crate::config::{Config, ManagedCodexAccountConfig, managed_codex_account_dir, paths};
use crate::model::ProviderId;
use chrono::Utc;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexAccount {
    pub id: String,
    pub label: String,
    pub email: Option<String>,
    pub provider_account_id: Option<String>,
    pub codex_home: PathBuf,
    pub credentials_available: bool,
    pub credentials_error: Option<String>,
}

pub fn discover_accounts(config: &Config) -> Vec<CodexAccount> {
    let storage = ProviderAccountStorage::new(paths().codex_accounts_dir);
    let mut accounts = Vec::new();
    for managed in &config.codex_managed_accounts {
        let metadata_result = storage.load_metadata(&managed.id);
        let tokens_result = storage.load_tokens(&managed.id);
        let (credentials_available, credentials_error) =
            credential_status(&metadata_result, &tokens_result);
        let metadata = metadata_result.ok();
        let email = metadata
            .as_ref()
            .map(|metadata| metadata.email.clone())
            .filter(|email| !email.is_empty())
            .or_else(|| managed.email.clone().filter(|email| !email.is_empty()));
        let label = email.clone().unwrap_or_else(|| {
            if managed.label.trim().is_empty() {
                "Codex account".to_string()
            } else {
                managed.label.clone()
            }
        });
        let provider_account_id = metadata
            .as_ref()
            .and_then(|metadata| metadata.provider_account_id.clone())
            .or_else(|| managed.provider_account_id.clone());
        let Ok(codex_home) = storage.account_dir(&managed.id) else {
            continue;
        };
        let discovered = CodexAccount {
            id: managed.id.clone(),
            label,
            email,
            provider_account_id,
            codex_home,
            credentials_available,
            credentials_error,
        };
        match discovered.email.as_deref().map(normalized_email) {
            Some(email_key) => {
                if let Some(index) = accounts.iter().position(|existing: &CodexAccount| {
                    same_identity(
                        existing.email.as_deref(),
                        existing.provider_account_id.as_deref(),
                        Some(&email_key),
                        discovered.provider_account_id.as_deref(),
                    )
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

fn credential_status(
    metadata: &Result<ProviderAccountMetadata, AccountStorageError>,
    tokens: &Result<ProviderAccountTokens, AccountStorageError>,
) -> (bool, Option<String>) {
    let non_missing_error = [metadata.as_ref().err(), tokens.as_ref().err()]
        .into_iter()
        .flatten()
        .find(|error| !error.is_missing());
    if let Some(error) = non_missing_error {
        return (false, Some(error.to_string()));
    }
    (metadata.is_ok() && tokens.is_ok(), None)
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
    config
        .codex_managed_accounts
        .retain(|existing| existing.id != account_id);
    config.codex_managed_accounts.push(account);
    dedupe_managed_accounts(config);
    select_account_after_login(config, ProviderId::Codex, account_id);
}

pub fn normalized_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

pub(crate) fn find_matching_account<'a>(
    config: &'a Config,
    email: Option<&str>,
    provider_account_id: Option<&str>,
) -> Option<&'a ManagedCodexAccountConfig> {
    let email = email?;
    let email = normalized_email(email);
    config.codex_managed_accounts.iter().find(|account| {
        same_identity(
            account.email.as_deref(),
            account.provider_account_id.as_deref(),
            Some(&email),
            provider_account_id,
        )
    })
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
                same_identity(
                    existing.email.as_deref(),
                    existing.provider_account_id.as_deref(),
                    Some(&email_key),
                    account.provider_account_id.as_deref(),
                )
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

fn same_identity(
    existing_email: Option<&str>,
    existing_provider_account_id: Option<&str>,
    candidate_email: Option<&str>,
    candidate_provider_account_id: Option<&str>,
) -> bool {
    match (existing_provider_account_id, candidate_provider_account_id) {
        (Some(existing), Some(candidate)) => existing == candidate,
        (None, None) => {
            existing_email.map(normalized_email) == candidate_email.map(normalized_email)
        }
        _ => false,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_storage::{
        NewProviderAccount, ProviderAccountStorage, ProviderAccountTokens,
    };
    use crate::model::ProviderId;
    use crate::test_support;
    use std::fs;
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
    fn apply_login_account_keeps_existing_distinct_account() {
        let now = Utc::now();
        let existing = ManagedCodexAccountConfig {
            id: "codex-a".to_string(),
            label: "a@example.com".to_string(),
            codex_home: PathBuf::from("/tmp/codex-a"),
            email: Some("a@example.com".to_string()),
            provider_account_id: Some("acct-a".to_string()),
            created_at: now,
            updated_at: now,
            last_authenticated_at: Some(now),
        };
        let mut config = Config {
            codex_managed_accounts: vec![existing],
            selected_codex_account_ids: vec!["codex-a".to_string()],
            ..Config::default()
        };
        let added = ManagedCodexAccountConfig {
            id: "codex-b".to_string(),
            label: "b@example.com".to_string(),
            codex_home: PathBuf::from("/tmp/codex-b"),
            email: Some("b@example.com".to_string()),
            provider_account_id: Some("acct-b".to_string()),
            created_at: now,
            updated_at: now,
            last_authenticated_at: Some(now),
        };

        apply_login_account(&mut config, added);

        assert_eq!(config.codex_managed_accounts.len(), 2);
        assert!(
            config
                .codex_managed_accounts
                .iter()
                .any(|account| account.id == "codex-a")
        );
        assert_eq!(config.selected_codex_account_ids, vec!["codex-b"]);
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
    fn preserves_same_email_accounts_with_different_known_id_states() {
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

        assert!(!changed);
        assert_eq!(config.codex_managed_accounts.len(), 2);
        assert_eq!(config.selected_codex_account_ids.as_slice(), ["codex-b"]);
    }

    #[test]
    fn discover_accounts_preserves_different_provider_account_ids() {
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

        assert_eq!(accounts.len(), 2);
        assert!(
            accounts
                .iter()
                .any(|account| account.id == account_a.account_ref.account_id)
        );
        assert!(
            accounts
                .iter()
                .any(|account| account.id == account_b.account_ref.account_id)
        );
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
        assert!(accounts[0].credentials_available);
        assert!(accounts[0].credentials_error.is_none());
    }

    #[test]
    fn discover_accounts_keeps_managed_account_when_credentials_are_missing() {
        let _guard = test_support::env_lock();
        let state_root = temp_dir("codex-missing-credentials-state");

        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }

        let storage = ProviderAccountStorage::new(paths().codex_accounts_dir);
        let stored = create_stored_account(&storage, "user@example.com", "acct-123");
        fs::remove_file(stored.account_dir.join("tokens.json")).unwrap();
        let now = Utc::now();
        let config = Config {
            selected_codex_account_ids: vec![stored.account_ref.account_id.clone()],
            codex_managed_accounts: vec![ManagedCodexAccountConfig {
                id: stored.account_ref.account_id.clone(),
                label: "old label".to_string(),
                codex_home: stored.account_dir,
                email: Some("user@example.com".to_string()),
                provider_account_id: Some("acct-123".to_string()),
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
        assert_eq!(accounts[0].id, stored.account_ref.account_id);
        assert_eq!(accounts[0].label, "user@example.com");
        assert_eq!(accounts[0].email.as_deref(), Some("user@example.com"));
        assert_eq!(accounts[0].provider_account_id.as_deref(), Some("acct-123"));
        assert!(!accounts[0].credentials_available);
        assert!(accounts[0].credentials_error.is_none());
    }

    #[test]
    fn discover_accounts_does_not_treat_malformed_credentials_as_missing() {
        let state_root = temp_dir("codex-malformed-credentials-state");
        let mut env = test_support::test_env();
        env.set("XDG_STATE_HOME", &state_root);

        let storage = ProviderAccountStorage::new(paths().codex_accounts_dir);
        let stored = create_stored_account(&storage, "user@example.com", "acct-123");
        fs::write(stored.account_dir.join("metadata.json"), b"not json").unwrap();
        let now = Utc::now();
        let account_id = stored.account_ref.account_id.clone();
        let config = Config {
            codex_managed_accounts: vec![ManagedCodexAccountConfig {
                id: account_id,
                label: "old label".to_string(),
                codex_home: stored.account_dir,
                email: Some("user@example.com".to_string()),
                provider_account_id: Some("acct-123".to_string()),
                created_at: now,
                updated_at: now,
                last_authenticated_at: Some(now),
            }],
            ..Config::default()
        };

        let accounts = discover_accounts(&config);

        assert_eq!(accounts.len(), 1);
        assert!(!accounts[0].credentials_available);
        assert!(
            accounts[0]
                .credentials_error
                .as_deref()
                .is_some_and(|error| { error.contains("failed to parse account file") })
        );
    }

    #[test]
    fn discover_accounts_prioritizes_malformed_tokens_over_missing_metadata() {
        let state_root = temp_dir("codex-missing-metadata-malformed-tokens-state");
        let mut env = test_support::test_env();
        env.set("XDG_STATE_HOME", &state_root);

        let storage = ProviderAccountStorage::new(paths().codex_accounts_dir);
        let stored = create_stored_account(&storage, "user@example.com", "acct-123");
        fs::remove_file(stored.account_dir.join("metadata.json")).unwrap();
        fs::write(stored.account_dir.join("tokens.json"), b"not json").unwrap();
        let now = Utc::now();
        let account_id = stored.account_ref.account_id.clone();
        let config = Config {
            codex_managed_accounts: vec![ManagedCodexAccountConfig {
                id: account_id,
                label: "old label".to_string(),
                codex_home: stored.account_dir,
                email: Some("user@example.com".to_string()),
                provider_account_id: Some("acct-123".to_string()),
                created_at: now,
                updated_at: now,
                last_authenticated_at: Some(now),
            }],
            ..Config::default()
        };

        let accounts = discover_accounts(&config);

        assert_eq!(accounts.len(), 1);
        assert!(!accounts[0].credentials_available);
        assert!(
            accounts[0]
                .credentials_error
                .as_deref()
                .is_some_and(|error| { error.contains("failed to parse account file") })
        );
    }

    #[cfg(unix)]
    #[test]
    fn discover_accounts_does_not_treat_symlinked_credentials_as_missing() {
        use std::os::unix::fs::symlink;

        let state_root = temp_dir("codex-symlinked-credentials-state");
        let mut env = test_support::test_env();
        env.set("XDG_STATE_HOME", &state_root);

        let storage = ProviderAccountStorage::new(paths().codex_accounts_dir);
        let stored = create_stored_account(&storage, "user@example.com", "acct-123");
        let external_metadata = state_root.join("external-metadata.json");
        fs::write(&external_metadata, "{}").unwrap();
        fs::remove_file(stored.account_dir.join("metadata.json")).unwrap();
        symlink(external_metadata, stored.account_dir.join("metadata.json")).unwrap();
        let now = Utc::now();
        let account_id = stored.account_ref.account_id.clone();
        let config = Config {
            codex_managed_accounts: vec![ManagedCodexAccountConfig {
                id: account_id,
                label: "old label".to_string(),
                codex_home: stored.account_dir,
                email: Some("user@example.com".to_string()),
                provider_account_id: Some("acct-123".to_string()),
                created_at: now,
                updated_at: now,
                last_authenticated_at: Some(now),
            }],
            ..Config::default()
        };

        let accounts = discover_accounts(&config);

        assert_eq!(accounts.len(), 1);
        assert!(!accounts[0].credentials_available);
        assert!(
            accounts[0]
                .credentials_error
                .as_deref()
                .is_some_and(|error| { error.contains("symlinked") })
        );
    }

    #[cfg(unix)]
    #[test]
    fn discover_accounts_prioritizes_symlinked_tokens_over_missing_metadata() {
        use std::os::unix::fs::symlink;

        let state_root = temp_dir("codex-missing-metadata-symlinked-tokens-state");
        let mut env = test_support::test_env();
        env.set("XDG_STATE_HOME", &state_root);

        let storage = ProviderAccountStorage::new(paths().codex_accounts_dir);
        let stored = create_stored_account(&storage, "user@example.com", "acct-123");
        let external_tokens = state_root.join("external-tokens.json");
        fs::write(&external_tokens, "{}").unwrap();
        fs::remove_file(stored.account_dir.join("metadata.json")).unwrap();
        fs::remove_file(stored.account_dir.join("tokens.json")).unwrap();
        symlink(external_tokens, stored.account_dir.join("tokens.json")).unwrap();
        let now = Utc::now();
        let account_id = stored.account_ref.account_id.clone();
        let config = Config {
            codex_managed_accounts: vec![ManagedCodexAccountConfig {
                id: account_id,
                label: "old label".to_string(),
                codex_home: stored.account_dir,
                email: Some("user@example.com".to_string()),
                provider_account_id: Some("acct-123".to_string()),
                created_at: now,
                updated_at: now,
                last_authenticated_at: Some(now),
            }],
            ..Config::default()
        };

        let accounts = discover_accounts(&config);

        assert_eq!(accounts.len(), 1);
        assert!(!accounts[0].credentials_available);
        assert!(
            accounts[0]
                .credentials_error
                .as_deref()
                .is_some_and(|error| { error.contains("symlinked") })
        );
    }
}
