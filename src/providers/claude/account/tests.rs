use super::*;
use crate::auth::load_claude_auth_from_config_dir;
use crate::test_support;
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
fn recovers_managed_claude_dir_when_config_empty() {
    let _guard = test_support::env_lock();
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
        config.selected_claude_account_ids.as_slice(),
        ["claude-recover-test"]
    );
    assert!(load_claude_auth_from_config_dir(&account_dir).is_ok());
}

#[test]
fn imports_external_claude_config_into_managed_storage() {
    let _guard = test_support::env_lock();
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
        config.selected_claude_account_ids.as_slice(),
        [account.id.as_str()]
    );
    assert!(account.config_dir.join(".credentials.json").exists());
    assert!(!account.config_dir.join("extra.txt").exists());
    assert!(!account.config_dir.join(".git").exists());
}

#[test]
fn sync_managed_accounts_fills_email_from_access_token_when_missing() {
    use base64::Engine;

    let _guard = test_support::env_lock();
    let dir = temp_dir("claude-sync-email-from-jwt");
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("{}");
    let payload =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"email":"jwt@example.com"}"#);
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

    let _guard = test_support::env_lock();
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("{}");
    let payload =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"email":"same@example.com"}"#);
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

#[test]
fn discover_accounts_includes_accounts_without_email() {
    let dir = temp_dir("claude-discover-no-email");
    write_credentials(&dir);

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
    write_credentials(&dir);

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
