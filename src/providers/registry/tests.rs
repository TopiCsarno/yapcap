use super::*;
use crate::config::Config;
use crate::providers::interface::ProviderAccountAction;

#[test]
fn providers_expose_expected_capabilities() {
    assert_eq!(
        capabilities(ProviderId::Codex),
        ProviderCapabilities {
            supports_background_status_refresh: false,
            requires_auth_prompt_on_auth_failure: false,
        }
    );
    assert_eq!(
        capabilities(ProviderId::Claude),
        ProviderCapabilities {
            supports_background_status_refresh: false,
            requires_auth_prompt_on_auth_failure: false,
        }
    );
    assert_eq!(
        capabilities(ProviderId::Cursor),
        ProviderCapabilities {
            supports_background_status_refresh: true,
            requires_auth_prompt_on_auth_failure: true,
        }
    );
    assert_eq!(
        capabilities(ProviderId::Kimi),
        ProviderCapabilities {
            supports_background_status_refresh: false,
            requires_auth_prompt_on_auth_failure: false,
        }
    );
    assert_eq!(
        capabilities(ProviderId::OpenCodeGo),
        ProviderCapabilities {
            supports_background_status_refresh: false,
            requires_auth_prompt_on_auth_failure: false,
        }
    );
    assert_eq!(
        capabilities(ProviderId::Grok),
        ProviderCapabilities {
            supports_background_status_refresh: false,
            requires_auth_prompt_on_auth_failure: false,
        }
    );
}

#[test]
fn grok_provider_registered_and_discovers_accounts() {
    let empty_config = Config::default();
    let descriptors = discover_accounts(ProviderId::Grok, &empty_config);
    assert!(descriptors.is_empty());
    assert!(!capabilities(ProviderId::Grok).supports_background_status_refresh);

    let mut config = Config::default();
    config
        .grok_managed_accounts
        .push(crate::config::ManagedGrokAccountConfig {
            id: "grok-1".to_string(),
            label: "Grok User".to_string(),
            email: Some("grok@example.com".to_string()),
            config_dir: std::path::PathBuf::from("/tmp/grok-1"),
            provider_account_id: Some("user-1".to_string()),
            team_id: None,
            plan: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            last_authenticated_at: None,
        });
    let descriptors = discover_accounts(ProviderId::Grok, &config);
    assert_eq!(descriptors.len(), 1);
    assert_eq!(descriptors[0].account_id, "grok-1");
    assert_eq!(descriptors[0].label, "Grok User");
    assert_eq!(login_kind(ProviderId::Grok), ProviderLoginKind::Grok);
}

#[test]
fn cursor_supports_background_status_refresh() {
    assert!(supports_background_status_refresh(ProviderId::Cursor));
    assert!(!supports_background_status_refresh(ProviderId::Codex));
    assert!(!supports_background_status_refresh(ProviderId::Claude));
}

#[test]
fn cursor_requires_reauth_prompt_on_auth_error() {
    assert!(auth_error_requires_reauth_prompt(ProviderId::Cursor));
    assert!(!auth_error_requires_reauth_prompt(ProviderId::Codex));
    assert!(!auth_error_requires_reauth_prompt(ProviderId::Claude));
}

#[test]
fn each_provider_resolves_accounts() {
    let config = Config::default();
    for provider in ProviderId::ALL {
        let accounts = discover_accounts(provider, &config);
        assert!(
            accounts.is_empty(),
            "default config should have no accounts for {provider:?}"
        );
    }
}

#[test]
fn host_aware_providers_resolve_system_active_account_id() {
    use crate::account_storage::{
        NewProviderAccount, ProviderAccountStorage, ProviderAccountTokens,
    };
    use crate::config::{
        ManagedClaudeAccountConfig, ManagedCodexAccountConfig, ManagedCursorAccountConfig,
        ManagedGeminiAccountConfig, ManagedGrokAccountConfig, ManagedKimiAccountConfig,
        ManagedMinimaxAccountConfig, ManagedOpenCodeGoAccountConfig, paths,
    };
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use chrono::Utc;
    use rusqlite::Connection;
    use std::fs;
    use std::path::PathBuf;

    let mut env = crate::test_support::test_env();
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let state = temp.path().join("state");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&state).unwrap();
    env.set("HOME", &home);
    env.set("XDG_STATE_HOME", &state);
    env.set("MINIMAX_API_KEY", "test-minimax-key");
    env.set("KIMI_API_KEY", "test-kimi-key");
    env.set("OPENCODE_GO_API_KEY", "test-opencode-go-key");
    env.remove("FLATPAK_ID");

    let codex_dir = home.join(".codex");
    fs::create_dir_all(&codex_dir).unwrap();
    let id_token = "eyJhbGciOiJSUzI1NiJ9.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOiB7ImNoYXRncHRfYWNjb3VudF9pZCI6ICJhY2N0LWFiYy0xMjMifX0.fakesig";
    fs::write(
        codex_dir.join("auth.json"),
        format!(r#"{{"tokens":{{"id_token":"{id_token}"}}}}"#),
    )
    .unwrap();

    let cursor_dir = home.join(".config/Cursor/User/globalStorage");
    fs::create_dir_all(&cursor_dir).unwrap();
    let cursor_jwt_payload = URL_SAFE_NO_PAD.encode(
        format!(
            r#"{{"sub":"auth0|cursor-user","exp":{}}}"#,
            (Utc::now() + chrono::Duration::hours(1)).timestamp()
        )
        .as_bytes(),
    );
    let cursor_jwt = format!("header.{cursor_jwt_payload}.signature");
    let cursor_db = Connection::open(cursor_dir.join("state.vscdb")).unwrap();
    cursor_db
        .execute_batch("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value TEXT)")
        .unwrap();
    cursor_db
        .execute(
            "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
            rusqlite::params!["cursorAuth/accessToken", cursor_jwt],
        )
        .unwrap();
    cursor_db
        .execute(
            "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
            rusqlite::params!["cursorAuth/refreshToken", "cursor-refresh"],
        )
        .unwrap();

    let gemini_dir = home.join(".gemini");
    fs::create_dir_all(&gemini_dir).unwrap();
    fs::write(
        gemini_dir.join("google_accounts.json"),
        r#"{"active":"alice@example.com"}"#,
    )
    .unwrap();

    let storage = ProviderAccountStorage::new(paths().claude_accounts_dir.clone());
    let stored = storage
        .create_account(NewProviderAccount {
            provider: ProviderId::Claude,
            email: "claude@example.com".to_string(),
            provider_account_id: Some("acct-uuid".to_string()),
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
    fs::write(
        home.join(".claude.json"),
        r#"{"oauthAccount":{"accountUuid":"acct-uuid"}}"#,
    )
    .unwrap();

    let cursor_storage = ProviderAccountStorage::new(paths().cursor_accounts_dir.clone());
    cursor_storage
        .replace_account(
            "cursor-1".to_string(),
            NewProviderAccount {
                provider: ProviderId::Cursor,
                email: "cursor@example.com".to_string(),
                provider_account_id: None,
                organization_id: None,
                organization_name: None,
                tokens: ProviderAccountTokens {
                    access_token: cursor_jwt,
                    refresh_token: "cursor-refresh".to_string(),
                    expires_at: Utc::now() + chrono::Duration::hours(1),
                    scope: vec![],
                    token_id: Some("cursor-user".to_string()),
                },
                snapshot: None,
            },
        )
        .unwrap();

    let grok_dir = home.join(".grok");
    fs::create_dir_all(&grok_dir).unwrap();
    fs::write(
        grok_dir.join("auth.json"),
        r#"{"https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828":{"key":"grok-token","user_id":"grok-user"}}"#,
    )
    .unwrap();

    let config = Config {
        codex_managed_accounts: vec![ManagedCodexAccountConfig {
            id: "codex-1".to_string(),
            label: "Codex".to_string(),
            codex_home: PathBuf::from("/tmp"),
            email: None,
            provider_account_id: Some("acct-abc-123".to_string()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_authenticated_at: None,
        }],
        claude_managed_accounts: vec![ManagedClaudeAccountConfig {
            id: stored.metadata.account_id.clone(),
            label: "Claude".to_string(),
            config_dir: paths()
                .claude_accounts_dir
                .join(&stored.metadata.account_id),
            email: Some("claude@example.com".to_string()),
            organization: None,
            subscription_type: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_authenticated_at: None,
        }],
        cursor_managed_accounts: vec![ManagedCursorAccountConfig {
            id: "cursor-1".to_string(),
            email: "cursor@example.com".to_string(),
            label: "cursor@example.com".to_string(),
            account_root: paths().cursor_accounts_dir.join("cursor-1"),
            display_name: None,
            plan: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_authenticated_at: None,
        }],
        gemini_managed_accounts: vec![ManagedGeminiAccountConfig {
            id: "gemini-1".to_string(),
            label: "Gemini".to_string(),
            account_root: PathBuf::from("/tmp/gemini-1"),
            email: "alice@example.com".to_string(),
            sub: "sub".to_string(),
            hd: None,
            last_tier_id: None,
            last_cloudaicompanion_project: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_authenticated_at: None,
        }],
        minimax_managed_accounts: vec![ManagedMinimaxAccountConfig {
            id: "minimax-1".to_string(),
            label: "Minimax".to_string(),
            api_key_source: "env:MINIMAX_API_KEY".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_authenticated_at: None,
        }],
        kimi_managed_accounts: vec![ManagedKimiAccountConfig {
            id: "kimi-1".to_string(),
            label: "Kimi".to_string(),
            api_key_source: "env:KIMI_API_KEY".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_authenticated_at: None,
        }],
        opencode_go_managed_accounts: vec![ManagedOpenCodeGoAccountConfig {
            id: "opencode-go-1".to_string(),
            label: "OpenCode Go".to_string(),
            api_key_source: "env:OPENCODE_GO_API_KEY".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_authenticated_at: None,
        }],
        grok_managed_accounts: vec![ManagedGrokAccountConfig {
            id: "grok-1".to_string(),
            label: "Grok".to_string(),
            config_dir: paths().grok_accounts_dir.join("grok-1"),
            email: Some("grok@example.com".to_string()),
            provider_account_id: Some("grok-user".to_string()),
            team_id: None,
            plan: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_authenticated_at: None,
        }],
        ..Config::default()
    };

    let expectations = [
        (ProviderId::Codex, true),
        (ProviderId::Claude, true),
        (ProviderId::Gemini, true),
        (ProviderId::Cursor, true),
        (ProviderId::Copilot, false),
        (ProviderId::Minimax, true),
        (ProviderId::Kimi, true),
        (ProviderId::OpenCodeGo, true),
        (ProviderId::Grok, true),
    ];
    for (provider, expect_some) in expectations {
        let result = system_active_account_id(provider, &config);
        assert_eq!(
            result.is_some(),
            expect_some,
            "provider {provider:?} expected has_active={expect_some}, got {result:?}"
        );
    }

    let mut state = crate::model::AppState::empty();
    reconcile_provider_accounts(ProviderId::Cursor, &config, &mut state);
    assert_eq!(
        state
            .provider(ProviderId::Cursor)
            .and_then(|provider| provider.system_active_account_id.as_deref()),
        Some("cursor-managed:cursor-1")
    );

    reconcile_provider_accounts(ProviderId::Grok, &config, &mut state);
    assert_eq!(
        state
            .provider(ProviderId::Grok)
            .and_then(|provider| provider.system_active_account_id.as_deref()),
        Some("grok-1")
    );
}

#[test]
fn opencode_go_system_active_account_id_matches_opencode_auth_file() {
    use crate::config::{ManagedOpenCodeGoAccountConfig, paths};
    use crate::providers::opencode_auth::OPENCODE_AUTH_PATH_ENV;
    use crate::providers::opencode_go::storage::write_api_key_at;
    use chrono::Utc;
    use std::fs;

    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("state");
    let auth_path = temp.path().join("auth.json");
    let mut env = crate::test_support::test_env();
    env.set("XDG_STATE_HOME", &state);
    env.set(OPENCODE_AUTH_PATH_ENV, &auth_path);
    env.remove("OPENCODE_API_KEY");
    env.remove("OPENCODE_GO_API_KEY");

    fs::write(
        &auth_path,
        r#"{"opencode-go":{"type":"api","key":"test-key"}}"#,
    )
    .unwrap();
    write_api_key_at(&paths().opencode_go_accounts_dir, "go-1", "test-key").unwrap();
    let config = Config {
        opencode_go_managed_accounts: vec![ManagedOpenCodeGoAccountConfig {
            id: "go-1".to_string(),
            label: "OpenCode Go".to_string(),
            api_key_source: "stored".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_authenticated_at: None,
        }],
        ..Config::default()
    };

    assert_eq!(
        system_active_account_id(ProviderId::OpenCodeGo, &config),
        Some("go-1".to_string())
    );
}

#[test]
fn every_provider_descriptor_declares_supported_account_actions() {
    use crate::account_storage::{
        NewProviderAccount, ProviderAccountStorage, ProviderAccountTokens,
    };
    use crate::config::{
        ManagedAntigravityAccountConfig, ManagedClaudeAccountConfig, ManagedCodexAccountConfig,
        ManagedCopilotAccountConfig, ManagedCursorAccountConfig, ManagedGeminiAccountConfig,
        ManagedGrokAccountConfig, ManagedKimiAccountConfig, ManagedMinimaxAccountConfig,
        ManagedOpenCodeGoAccountConfig, paths,
    };
    use crate::providers::opencode_auth::{OPENCODE_AUTH_CONTENT_ENV, OPENCODE_AUTH_PATH_ENV};
    use std::path::PathBuf;

    let mut env = crate::test_support::test_env();
    let opencode_root = tempfile::tempdir().unwrap();
    let home = opencode_root.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    env.set("HOME", &home);
    env.remove(OPENCODE_AUTH_CONTENT_ENV);
    env.set(
        OPENCODE_AUTH_PATH_ENV,
        opencode_root.path().join("auth.json"),
    );

    let now = chrono::Utc::now();
    let codex_id = "codex-1";
    let cursor_id = "cursor-1";
    let codex_storage = ProviderAccountStorage::new(paths().codex_accounts_dir.clone());
    codex_storage
        .replace_account(
            codex_id.to_string(),
            NewProviderAccount {
                provider: ProviderId::Codex,
                email: "codex@example.com".to_string(),
                provider_account_id: None,
                organization_id: None,
                organization_name: None,
                tokens: ProviderAccountTokens {
                    access_token: "access".to_string(),
                    refresh_token: "refresh".to_string(),
                    expires_at: now + chrono::Duration::hours(1),
                    scope: Vec::new(),
                    token_id: None,
                },
                snapshot: None,
            },
        )
        .unwrap();
    let cursor_storage = ProviderAccountStorage::new(paths().cursor_accounts_dir.clone());
    cursor_storage
        .replace_account(
            cursor_id.to_string(),
            NewProviderAccount {
                provider: ProviderId::Cursor,
                email: "cursor@example.com".to_string(),
                provider_account_id: None,
                organization_id: None,
                organization_name: None,
                tokens: ProviderAccountTokens {
                    access_token: "access".to_string(),
                    refresh_token: "refresh".to_string(),
                    expires_at: now + chrono::Duration::hours(1),
                    scope: Vec::new(),
                    token_id: Some("cursor-user".to_string()),
                },
                snapshot: None,
            },
        )
        .unwrap();

    let config = Config {
        codex_managed_accounts: vec![ManagedCodexAccountConfig {
            id: codex_id.to_string(),
            label: "Codex account".to_string(),
            codex_home: paths().codex_accounts_dir.join(codex_id),
            email: Some("codex@example.com".to_string()),
            provider_account_id: None,
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }],
        claude_managed_accounts: vec![ManagedClaudeAccountConfig {
            id: "claude-1".to_string(),
            label: "Claude account".to_string(),
            config_dir: PathBuf::from("/tmp/claude-1"),
            email: Some("claude@example.com".to_string()),
            organization: None,
            subscription_type: None,
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }],
        cursor_managed_accounts: vec![ManagedCursorAccountConfig {
            id: cursor_id.to_string(),
            email: "cursor@example.com".to_string(),
            label: "cursor@example.com".to_string(),
            account_root: paths().cursor_accounts_dir.join(cursor_id),
            display_name: None,
            plan: None,
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }],
        gemini_managed_accounts: vec![ManagedGeminiAccountConfig {
            id: "gemini-1".to_string(),
            label: "Gemini account".to_string(),
            account_root: PathBuf::from("/tmp/gemini-1"),
            email: "gemini@example.com".to_string(),
            sub: "gemini-sub".to_string(),
            hd: None,
            last_tier_id: None,
            last_cloudaicompanion_project: None,
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }],
        copilot_managed_accounts: vec![ManagedCopilotAccountConfig {
            id: "copilot-1".to_string(),
            label: "Copilot account".to_string(),
            github_user_id: 1,
            login: "copilot".to_string(),
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }],
        minimax_managed_accounts: vec![ManagedMinimaxAccountConfig {
            id: "minimax-1".to_string(),
            label: "Minimax account".to_string(),
            api_key_source: "stored".to_string(),
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }],
        kimi_managed_accounts: vec![ManagedKimiAccountConfig {
            id: "kimi-1".to_string(),
            label: "Kimi account".to_string(),
            api_key_source: "stored".to_string(),
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }],
        antigravity_managed_accounts: vec![ManagedAntigravityAccountConfig {
            id: "antigravity-1".to_string(),
            label: "Antigravity account".to_string(),
            account_root: PathBuf::from("/tmp/antigravity-1"),
            email: "antigravity@example.com".to_string(),
            sub: "antigravity-sub".to_string(),
            last_tier_id: None,
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }],
        opencode_go_managed_accounts: vec![ManagedOpenCodeGoAccountConfig {
            id: "opencode-go-1".to_string(),
            label: "OpenCode Go account".to_string(),
            api_key_source: "stored".to_string(),
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }],
        grok_managed_accounts: vec![ManagedGrokAccountConfig {
            id: "grok-1".to_string(),
            label: "Grok account".to_string(),
            config_dir: PathBuf::from("/tmp/grok-1"),
            email: Some("grok@example.com".to_string()),
            provider_account_id: Some("grok-sub".to_string()),
            team_id: None,
            plan: None,
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }],
        ..Config::default()
    };

    let expected = [
        (
            ProviderId::Codex,
            vec![
                ProviderAccountAction::Delete,
                ProviderAccountAction::Reauthenticate,
            ],
        ),
        (
            ProviderId::Claude,
            vec![
                ProviderAccountAction::Delete,
                ProviderAccountAction::Reauthenticate,
            ],
        ),
        (
            ProviderId::Cursor,
            vec![ProviderAccountAction::Delete, ProviderAccountAction::Rescan],
        ),
        (
            ProviderId::Gemini,
            vec![
                ProviderAccountAction::Delete,
                ProviderAccountAction::Reauthenticate,
            ],
        ),
        (
            ProviderId::Copilot,
            vec![
                ProviderAccountAction::Delete,
                ProviderAccountAction::Reauthenticate,
            ],
        ),
        (
            ProviderId::Minimax,
            vec![
                ProviderAccountAction::Delete,
                ProviderAccountAction::Reauthenticate,
            ],
        ),
        (
            ProviderId::Kimi,
            vec![
                ProviderAccountAction::Delete,
                ProviderAccountAction::Reauthenticate,
            ],
        ),
        (
            ProviderId::Antigravity,
            vec![
                ProviderAccountAction::Delete,
                ProviderAccountAction::Reauthenticate,
            ],
        ),
        (
            ProviderId::OpenCodeGo,
            vec![
                ProviderAccountAction::Delete,
                ProviderAccountAction::Reauthenticate,
            ],
        ),
        (
            ProviderId::Grok,
            vec![
                ProviderAccountAction::Delete,
                ProviderAccountAction::Reauthenticate,
            ],
        ),
    ];

    for (provider, actions) in &expected {
        let account = discover_accounts(*provider, &config)
            .pop()
            .unwrap_or_else(|| panic!("{provider:?} should be discovered"));
        assert_eq!(
            account.actions, *actions,
            "unexpected actions for {provider:?}"
        );
    }

    let mut state = crate::model::AppState::empty();
    for (provider, _) in &expected {
        let account = discover_accounts(*provider, &config)
            .pop()
            .unwrap_or_else(|| panic!("{provider:?} should be discovered"));
        state.upsert_account(crate::model::ProviderAccountRuntimeState::empty(
            *provider,
            account.account_id,
            account.label,
        ));
    }
    for (provider, actions) in &expected {
        let facts = prepare_account_facts(*provider, &config, &state);
        assert_eq!(
            facts.len(),
            1,
            "expected one prepared account for {provider:?}"
        );
        assert_eq!(
            &facts[0].actions, actions,
            "unexpected facts for {provider:?}"
        );
    }
}

#[test]
fn codex_recovery_actions_distinguish_sign_in_from_opencode_restore() {
    use crate::config::ManagedCodexAccountConfig;
    use crate::providers::opencode_auth::OPENCODE_AUTH_CONTENT_ENV;
    use std::path::PathBuf;

    let mut env = crate::test_support::test_env();
    env.set(
        OPENCODE_AUTH_CONTENT_ENV,
        r#"{"openai":{"type":"oauth","access":"access","refresh":"refresh","expires":4102444800000}}"#,
    );
    let now = chrono::Utc::now();
    let config = Config {
        codex_managed_accounts: vec![ManagedCodexAccountConfig {
            id: "codex-missing-credentials".to_string(),
            label: "Codex account".to_string(),
            codex_home: PathBuf::from("/tmp/codex-missing-credentials"),
            email: Some("codex@example.com".to_string()),
            provider_account_id: None,
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }],
        ..Config::default()
    };

    let account = discover_accounts(ProviderId::Codex, &config)
        .pop()
        .expect("Codex metadata should remain discoverable when credentials are missing");

    assert_eq!(
        account.actions,
        vec![
            ProviderAccountAction::Delete,
            ProviderAccountAction::Reauthenticate,
            ProviderAccountAction::RestoreFromOpenCode,
        ]
    );
}

#[test]
fn copilot_recovery_prefers_opencode_restore_when_available() {
    use crate::config::ManagedCopilotAccountConfig;
    use crate::providers::opencode_auth::OPENCODE_AUTH_CONTENT_ENV;

    let mut env = crate::test_support::test_env();
    env.set(
        OPENCODE_AUTH_CONTENT_ENV,
        r#"{"github-copilot":{"type":"oauth","access":"access","refresh":"refresh","expires":0}}"#,
    );
    let now = chrono::Utc::now();
    let config = Config {
        copilot_managed_accounts: vec![ManagedCopilotAccountConfig {
            id: "copilot-1".to_string(),
            label: "Copilot account".to_string(),
            github_user_id: 1,
            login: "copilot".to_string(),
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }],
        ..Config::default()
    };

    let account = discover_accounts(ProviderId::Copilot, &config)
        .pop()
        .expect("Copilot account should be discovered");
    assert_eq!(
        account.actions,
        vec![
            ProviderAccountAction::Delete,
            ProviderAccountAction::Reauthenticate,
            ProviderAccountAction::RestoreFromOpenCode,
        ]
    );
}

#[test]
fn grok_recovery_offers_host_restore_when_available() {
    let mut env = crate::test_support::test_env();
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let grok_dir = home.join(".grok");
    std::fs::create_dir_all(&grok_dir).unwrap();
    env.set("HOME", &home);
    env.remove("FLATPAK_ID");

    std::fs::write(
        grok_dir.join("auth.json"),
        r#"{"https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828":{"key":"token","user_id":"uid"}}"#,
    )
    .unwrap();

    let now = chrono::Utc::now();
    let config = Config {
        grok_managed_accounts: vec![crate::config::ManagedGrokAccountConfig {
            id: "grok-1".to_string(),
            label: "Grok account".to_string(),
            config_dir: std::path::PathBuf::from("/tmp/grok-1"),
            email: Some("grok@example.com".to_string()),
            provider_account_id: Some("uid".to_string()),
            team_id: None,
            plan: None,
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }],
        ..Config::default()
    };

    let account = discover_accounts(ProviderId::Grok, &config)
        .pop()
        .expect("Grok account should be discovered");
    assert_eq!(
        account.actions,
        vec![
            ProviderAccountAction::Delete,
            ProviderAccountAction::Reauthenticate,
            ProviderAccountAction::RestoreFromGrok,
        ]
    );
}

#[test]
fn grok_delete_account_removes_from_config_and_storage() {
    use crate::account_storage::{
        NewProviderAccount, ProviderAccountStorage, ProviderAccountTokens,
    };

    let mut env = crate::test_support::test_env();
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("state");
    env.set("XDG_STATE_HOME", &state);

    let storage = ProviderAccountStorage::new(crate::config::paths().grok_accounts_dir);
    storage
        .create_account(NewProviderAccount {
            provider: ProviderId::Grok,
            email: "grok@example.com".to_string(),
            provider_account_id: Some("uid".to_string()),
            organization_id: None,
            organization_name: None,
            tokens: ProviderAccountTokens {
                access_token: "a".to_string(),
                refresh_token: "r".to_string(),
                expires_at: chrono::Utc::now(),
                scope: vec![],
                token_id: None,
            },
            snapshot: None,
        })
        .unwrap();

    let now = chrono::Utc::now();
    let mut config = Config {
        grok_managed_accounts: vec![crate::config::ManagedGrokAccountConfig {
            id: "grok-to-delete".to_string(),
            label: "Grok account".to_string(),
            config_dir: std::path::PathBuf::from("/tmp/grok"),
            email: Some("grok@example.com".to_string()),
            provider_account_id: Some("uid".to_string()),
            team_id: None,
            plan: None,
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }],
        selected_grok_account_ids: vec!["grok-to-delete".to_string()],
        ..Config::default()
    };

    assert!(delete_account(
        ProviderId::Grok,
        "grok-to-delete",
        &mut config
    ));
    assert!(config.grok_managed_accounts.is_empty());
    assert!(config.selected_grok_account_ids.is_empty());
    assert!(!delete_account(
        ProviderId::Grok,
        "nonexistent",
        &mut config
    ));
}
