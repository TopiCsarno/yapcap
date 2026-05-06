// SPDX-License-Identifier: MPL-2.0

use crate::account_storage::ProviderAccountStorage;
use crate::config::{ManagedClaudeAccountConfig, paths};
use crate::providers::claude::account::normalized_email;
use std::path::Path;

pub(crate) fn system_active_account_id(
    managed_accounts: &[ManagedClaudeAccountConfig],
    claude_json_path: &Path,
) -> Option<String> {
    let content = std::fs::read_to_string(claude_json_path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;
    let oauth = v.get("oauthAccount")?;
    let uuid = oauth
        .get("accountUuid")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let oauth_email = oauth
        .get("emailAddress")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(normalized_email);

    let storage = ProviderAccountStorage::new(paths().claude_accounts_dir);
    if let Some(uid) = uuid {
        return managed_accounts.iter().find_map(|account| {
            let metadata = storage.load_metadata(&account.id).ok()?;
            (metadata.provider_account_id.as_deref() == Some(uid)).then(|| account.id.clone())
        });
    }

    for account in managed_accounts {
        let Ok(metadata) = storage.load_metadata(&account.id) else {
            continue;
        };
        if let Some(ref oe) = oauth_email
            && normalized_email(&metadata.email) == *oe
        {
            return Some(account.id.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_storage::{NewProviderAccount, ProviderAccountTokens};
    use crate::config::paths;
    use crate::model::ProviderId;
    use crate::test_support;
    use chrono::Utc;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_state_root(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("yapcap-{name}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn stored_managed_account(
        storage: &ProviderAccountStorage,
        email: &str,
        provider_account_id: Option<&str>,
    ) -> ManagedClaudeAccountConfig {
        let stored = storage
            .create_account(NewProviderAccount {
                provider: ProviderId::Claude,
                email: email.to_string(),
                provider_account_id: provider_account_id.map(str::to_string),
                organization_id: None,
                organization_name: None,
                tokens: ProviderAccountTokens {
                    access_token: "a".to_string(),
                    refresh_token: "r".to_string(),
                    expires_at: Utc::now(),
                    scope: vec![],
                    token_id: None,
                },
                snapshot: None,
            })
            .unwrap();
        ManagedClaudeAccountConfig {
            id: stored.metadata.account_id.clone(),
            label: email.to_string(),
            config_dir: paths()
                .claude_accounts_dir
                .join(&stored.metadata.account_id),
            email: Some(email.to_string()),
            organization: None,
            subscription_type: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_authenticated_at: None,
        }
    }

    #[test]
    fn system_active_matches_metadata_provider_account_id() {
        let _guard = test_support::env_lock();
        let state_root = temp_state_root("claude-host-session");
        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }
        let claude_json = state_root.join(".claude.json");
        fs::write(
            &claude_json,
            r#"{"oauthAccount":{"accountUuid":"acct-9z","emailAddress":" u@x.org "}}"#,
        )
        .unwrap();
        let storage = ProviderAccountStorage::new(paths().claude_accounts_dir.clone());
        let managed = stored_managed_account(&storage, "u@x.org", Some("acct-9z"));
        let active = system_active_account_id(std::slice::from_ref(&managed), &claude_json);
        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }
        assert_eq!(active.as_deref(), Some(managed.id.as_str()));
    }

    #[test]
    fn system_active_matches_email_when_uuid_absent() {
        let _guard = test_support::env_lock();
        let state_root = temp_state_root("claude-host-session-email");
        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }
        let claude_json = state_root.join(".claude.json");
        fs::write(
            &claude_json,
            r#"{"oauthAccount":{"emailAddress":" USER@X.ORG "}}"#,
        )
        .unwrap();
        let storage = ProviderAccountStorage::new(paths().claude_accounts_dir.clone());
        let managed = stored_managed_account(&storage, "user@x.org", Some("acct-tracked"));
        let active = system_active_account_id(std::slice::from_ref(&managed), &claude_json);
        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }
        assert_eq!(active.as_deref(), Some(managed.id.as_str()));
    }

    #[test]
    fn system_active_does_not_fallback_to_email_when_uuid_is_untracked() {
        let _guard = test_support::env_lock();
        let state_root = temp_state_root("claude-host-session-untracked");
        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }
        let claude_json = state_root.join(".claude.json");
        fs::write(
            &claude_json,
            r#"{"oauthAccount":{"accountUuid":"acct-untracked","emailAddress":"user@x.org"}}"#,
        )
        .unwrap();
        let storage = ProviderAccountStorage::new(paths().claude_accounts_dir.clone());
        let managed = stored_managed_account(&storage, "user@x.org", Some("acct-tracked"));
        let active = system_active_account_id(std::slice::from_ref(&managed), &claude_json);
        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }
        assert_eq!(active, None);
    }
}
