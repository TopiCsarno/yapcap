use super::*;
use crate::config::paths;
use crate::test_support;
use chrono::Utc;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("yapcap-{name}-{nanos}"));
    fs::create_dir_all(&path).unwrap();
    path
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
        selected_claude_account_ids: vec!["claude-1".to_string()],
        claude_managed_accounts: vec![
            managed_account("claude-1", Some("user@example.com")),
            managed_account("claude-2", Some("USER@example.com")),
        ],
        ..Config::default()
    };

    let changed = dedupe_managed_accounts(&mut config);

    assert!(changed);
    assert_eq!(config.claude_managed_accounts.len(), 1);
    assert_eq!(config.selected_claude_account_ids.as_slice(), ["claude-1"]);
}

#[test]
fn discover_accounts_does_not_create_claude_accounts() {
    let _guard = test_support::env_lock();
    let state_root = temp_dir("claude-no-create-state");
    unsafe {
        std::env::set_var("XDG_STATE_HOME", &state_root);
    }

    let config = Config::default();
    let accounts = discover_accounts(&config);
    let claude_accounts_dir = paths().claude_accounts_dir.clone();
    unsafe {
        std::env::remove_var("XDG_STATE_HOME");
    }

    assert!(accounts.is_empty());
    assert!(!claude_accounts_dir.exists());
}

#[test]
fn discover_accounts_includes_accounts_without_email() {
    let dir = temp_dir("claude-discover-no-email");

    let config = Config {
        claude_managed_accounts: vec![ManagedClaudeAccountConfig {
            id: "claude-no-email".to_string(),
            label: "Claude account".to_string(),
            config_dir: dir,
            email: None,
            organization: None,
            subscription_type: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_authenticated_at: None,
        }],
        ..Config::default()
    };

    let accounts = discover_accounts(&config);

    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].id, "claude-no-email");
    assert_eq!(accounts[0].email, None);
}

#[test]
fn discover_accounts_includes_accounts_with_email() {
    let dir = temp_dir("claude-discover-with-email");

    let config = Config {
        claude_managed_accounts: vec![ManagedClaudeAccountConfig {
            id: "claude-with-email".to_string(),
            label: "user@example.com".to_string(),
            config_dir: dir,
            email: Some("user@example.com".to_string()),
            organization: None,
            subscription_type: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_authenticated_at: None,
        }],
        ..Config::default()
    };

    let accounts = discover_accounts(&config);

    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].email.as_deref(), Some("user@example.com"));
}
