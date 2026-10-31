// SPDX-License-Identifier: MPL-2.0

use super::*;
use crate::model::ProviderId;
use serde_json::json;
mod balances;
mod http;
mod subscription;

fn credits() -> Value {
    json!({
        "credits": {"monthlyCredits": 12.5, "purchasedCredits": 3, "freeCredits": 1},
        "windowLimits": {
            "limited": true,
            "fiveHour": {"used": 5, "cap": 20, "resetAt": 1793455200},
            "weekly": {"used": 30, "cap": 60, "resetAt": 1793455200000i64}
        }
    })
}

#[test]
fn command_code_cli_auth_file_is_detected() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join(".commandcode")).unwrap();
    std::fs::write(home.path().join(".commandcode/auth.json"), "{}").unwrap();
    assert!(crate::detection::detect(home.path()).detected(ProviderId::CommandCode));
}

#[test]
fn imports_only_the_requested_cli_key_without_changing_the_source() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("auth.json");
    let contents =
        r#"{"apiKey":"fixture-cli-key","userName":"fixture","otherSecret":"not-imported"}"#;
    std::fs::write(&path, contents).unwrap();
    assert_eq!(account::import_key_at(&path).unwrap(), "fixture-cli-key");
    assert_eq!(std::fs::read_to_string(path).unwrap(), contents);
}

#[test]
fn malformed_cli_credentials_never_echo_the_input() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("auth.json");
    std::fs::write(&path, r#"{"apiKey": "sensitive-fixture-data""#).unwrap();
    assert!(
        !account::import_key_at(&path)
            .unwrap_err()
            .contains("sensitive-fixture-data")
    );
    assert!(account::normalize_key("fixture-key\r\nInjected: value").is_err());
}

#[test]
fn registry_discovers_and_deletes_only_command_code_accounts() {
    let _env = crate::test_support::test_env();
    let now = chrono::Utc::now();
    let mut config = crate::config::Config {
        command_code_managed_accounts: vec![crate::config::ManagedCommandCodeAccountConfig {
            id: "fixture-command-code".to_string(),
            label: "Fixture".to_string(),
            created_at: now,
            updated_at: now,
            last_authenticated_at: Some(now),
        }],
        selected_command_code_account_ids: vec!["fixture-command-code".to_string()],
        selected_codex_account_ids: vec!["existing-codex".to_string()],
        ..crate::config::Config::default()
    };
    account::write_key("fixture-command-code", "fixture-key").unwrap();
    let descriptors =
        crate::providers::registry::discover_accounts(ProviderId::CommandCode, &config);
    assert_eq!(descriptors.len(), 1);
    assert_eq!(descriptors[0].account_id, "fixture-command-code");
    assert!(crate::providers::registry::delete_account(
        ProviderId::CommandCode,
        "fixture-command-code",
        &mut config
    ));
    assert!(config.command_code_managed_accounts.is_empty());
    assert!(config.selected_command_code_account_ids.is_empty());
    assert_eq!(config.selected_codex_account_ids, ["existing-codex"]);
    assert!(account::load_key("fixture-command-code").is_err());
}

#[cfg(unix)]
#[test]
fn cli_import_refuses_symlinks() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.json");
    let link = root.path().join("auth.json");
    std::fs::write(&source, r#"{"apiKey":"fixture-key"}"#).unwrap();
    std::os::unix::fs::symlink(source, &link).unwrap();
    assert!(account::import_key_at(&link).is_err());
}
