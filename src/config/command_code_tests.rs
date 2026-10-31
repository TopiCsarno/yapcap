// SPDX-License-Identifier: MPL-2.0

use super::*;

#[test]
fn legacy_configuration_keeps_accounts_and_selections_without_new_fields() {
    let now = Utc::now();
    let config = Config {
        selected_codex_account_ids: vec!["existing-codex".to_string()],
        codex_managed_accounts: vec![ManagedCodexAccountConfig {
            id: "existing-codex".to_string(),
            label: "Existing Codex".to_string(),
            codex_home: PathBuf::from("/fixture/codex"),
            email: Some("fixture@example.invalid".to_string()),
            provider_account_id: Some("fixture-user".to_string()),
            created_at: now,
            updated_at: now,
            last_authenticated_at: Some(now),
        }],
        selected_opencode_go_account_ids: vec!["existing-go".to_string()],
        opencode_go_managed_accounts: vec![ManagedOpenCodeGoAccountConfig {
            id: "existing-go".to_string(),
            label: "Existing Go".to_string(),
            api_key_source: "stored".to_string(),
            created_at: now,
            updated_at: now,
            last_authenticated_at: Some(now),
        }],
        ..Config::default()
    };
    let mut legacy = serde_json::to_value(&config).unwrap();
    let object = legacy.as_object_mut().unwrap();
    for key in [
        "command_code_enablement",
        "command_code_managed_accounts",
        "selected_command_code_account_ids",
    ] {
        object.remove(key);
    }
    let restored: Config = serde_json::from_value(legacy).unwrap();
    assert_eq!(restored, config);
    assert_eq!(Config::VERSION, 600);
    assert_eq!(APP_ID, "io.github.TopiCsarno.YapCap");
}

#[test]
fn missing_new_cosmic_keys_do_not_reset_existing_account_selections() {
    let _env = crate::test_support::test_env();
    let context = cosmic_config_context(APP_ID, Config::VERSION).unwrap();
    context
        .set(
            "selected_codex_account_ids",
            vec!["existing-codex".to_string()],
        )
        .unwrap();
    let restored = Config::get_entry(&context).unwrap_or_else(|(_, partial)| partial);
    assert_eq!(restored.selected_codex_account_ids, ["existing-codex"]);
    assert!(restored.command_code_managed_accounts.is_empty());
    assert!(restored.selected_command_code_account_ids.is_empty());
}

#[test]
fn command_code_watcher_update_does_not_replace_existing_accounts() {
    let mut config = Config {
        selected_codex_account_ids: vec!["existing".to_string()],
        ..Config::default()
    };
    let now = Utc::now();
    let update = Config {
        command_code_enablement: ProviderEnablement::Enabled,
        selected_command_code_account_ids: vec!["fixture-command-code".to_string()],
        command_code_managed_accounts: vec![ManagedCommandCodeAccountConfig {
            id: "fixture-command-code".to_string(),
            label: "Fixture".to_string(),
            created_at: now,
            updated_at: now,
            last_authenticated_at: Some(now),
        }],
        ..Config::default()
    };
    config.apply_watcher_update(
        update,
        &[
            "command_code_enablement",
            "selected_command_code_account_ids",
            "command_code_managed_accounts",
        ],
    );
    assert_eq!(config.selected_codex_account_ids, ["existing"]);
    assert_eq!(
        config.selected_command_code_account_ids,
        ["fixture-command-code"]
    );
    assert_eq!(config.command_code_managed_accounts.len(), 1);
}

#[test]
fn writing_new_entries_leaves_existing_configuration_keys_unchanged() {
    let _env = crate::test_support::test_env();
    let context = cosmic_config_context(APP_ID, Config::VERSION).unwrap();
    let old = Config {
        selected_codex_account_ids: vec!["existing".to_string()],
        ..Config::default()
    };
    old.write_entry(&context).unwrap();
    let mut new = old.clone();
    new.command_code_enablement = ProviderEnablement::Enabled;
    new.selected_command_code_account_ids
        .push("fixture-command-code".to_string());
    write_changed_config_entries(&context, &old, &new).unwrap();
    assert_eq!(
        context
            .get::<Vec<String>>("selected_codex_account_ids")
            .unwrap(),
        ["existing"]
    );
    assert_eq!(
        context
            .get::<Vec<String>>("selected_command_code_account_ids")
            .unwrap(),
        ["fixture-command-code"]
    );
    assert_eq!(
        context
            .get::<ProviderEnablement>("command_code_enablement")
            .unwrap(),
        ProviderEnablement::Enabled
    );
}

#[test]
fn account_metadata_never_contains_the_key() {
    let now = Utc::now();
    let account = ManagedCommandCodeAccountConfig {
        id: "fixture-command-code".to_string(),
        label: "Fixture".to_string(),
        created_at: now,
        updated_at: now,
        last_authenticated_at: None,
    };
    let value = serde_json::to_value(account).unwrap();
    assert!(value.get("api_key").is_none());
    assert!(value.get("access_token").is_none());
}

#[test]
fn existing_provider_serialization_is_unchanged() {
    for (provider, expected) in [
        (ProviderId::Codex, "codex"),
        (ProviderId::Claude, "claude"),
        (ProviderId::Cursor, "cursor"),
        (ProviderId::Antigravity, "antigravity"),
        (ProviderId::Gemini, "gemini"),
        (ProviderId::Copilot, "copilot"),
        (ProviderId::Minimax, "minimax"),
        (ProviderId::Zai, "zai"),
        (ProviderId::Kimi, "kimi"),
        (ProviderId::OpenCodeGo, "open_code_go"),
        (ProviderId::Grok, "grok"),
        (ProviderId::CommandCode, "command_code"),
    ] {
        assert_eq!(serde_json::to_value(provider).unwrap(), expected);
    }
}
