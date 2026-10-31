// SPDX-License-Identifier: MPL-2.0

use super::*;

#[test]
fn failed_login_starts_in_editing_without_a_panic_path() {
    let state = CommandCodeLoginState::failed("Fixture failure".to_string());
    assert_eq!(state.status, CommandCodeLoginStatus::Editing);
    assert_eq!(state.error.as_deref(), Some("Fixture failure"));
    assert!(!state.reauth);
    assert!(state.api_key.is_empty());
    assert!(state.login_url.is_none());
}

#[test]
fn login_state_and_events_redact_api_keys_in_debug_output() {
    let mut state = prepare(&Config::default(), None).unwrap();
    state.api_key = "sensitive-fixture-key".to_string();
    let event = CommandCodeLoginEvent::ApiKeyChanged(state.api_key.clone());
    assert!(!format!("{state:?}").contains("sensitive-fixture-key"));
    assert!(!format!("{event:?}").contains("sensitive-fixture-key"));
}

#[test]
fn reauthentication_preserves_account_identity_label_and_creation_time() {
    let _env = crate::test_support::test_env();
    let created_at = Utc::now() - chrono::Duration::days(20);
    let existing = ManagedCommandCodeAccountConfig {
        id: "fixture-account".to_string(),
        label: "Work".to_string(),
        created_at,
        updated_at: created_at,
        last_authenticated_at: None,
    };
    let config = Config {
        command_code_managed_accounts: vec![existing],
        ..Config::default()
    };
    let state = prepare(&config, Some("fixture-account")).unwrap();
    let account = commit(
        &config,
        &state,
        CommandCodeLoginSuccess {
            api_key: "fixture-key".to_string(),
            label: "Different label".to_string(),
        },
    )
    .unwrap();
    assert_eq!(account.id, "fixture-account");
    assert_eq!(account.label, "Work");
    assert_eq!(account.created_at, created_at);
    assert_eq!(
        super::account::load_key(&account.id).unwrap(),
        "fixture-key"
    );
}

#[test]
fn deleted_reauthentication_target_is_not_recreated() {
    let _env = crate::test_support::test_env();
    let mut state = prepare(&Config::default(), None).unwrap();
    state.reauth = true;
    assert!(
        commit(
            &Config::default(),
            &state,
            CommandCodeLoginSuccess {
                api_key: "fixture-key".to_string(),
                label: "Fixture".to_string(),
            }
        )
        .is_err()
    );
    assert!(super::account::load_key(&state.account_id).is_err());
}

#[cfg(unix)]
#[test]
fn new_api_key_files_have_owner_only_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let _env = crate::test_support::test_env();
    let state = prepare(&Config::default(), None).unwrap();
    let account = commit(
        &Config::default(),
        &state,
        CommandCodeLoginSuccess {
            api_key: "fixture-key".to_string(),
            label: "Fixture".to_string(),
        },
    )
    .unwrap();
    let dir = crate::config::paths()
        .command_code_accounts_dir
        .join(account.id);
    assert_eq!(
        std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(dir.join("api_key.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn duplicate_key_is_rejected_without_overwriting_the_original() {
    let _env = crate::test_support::test_env();
    let mut config = Config::default();
    let first = prepare(&config, None).unwrap();
    let account = commit(
        &config,
        &first,
        CommandCodeLoginSuccess {
            api_key: "fixture-key".to_string(),
            label: "First".to_string(),
        },
    )
    .unwrap();
    super::account::apply(&mut config, account);
    let second = prepare(&config, None).unwrap();
    assert!(
        commit(
            &config,
            &second,
            CommandCodeLoginSuccess {
                api_key: "fixture-key".to_string(),
                label: "Second".to_string(),
            }
        )
        .is_err()
    );
    assert_eq!(
        super::account::load_key(&first.account_id).unwrap(),
        "fixture-key"
    );
    assert!(super::account::load_key(&second.account_id).is_err());
}
