// SPDX-License-Identifier: MPL-2.0

use super::{AppModel, Message, ProviderId, Task, login, registry, runtime};
use crate::shared_state::RefreshRequestReason;

pub(super) fn delete_account(
    app: &mut AppModel,
    provider: ProviderId,
    account_id: &str,
) -> Task<Message> {
    let mut deleted = false;
    app.write_config(|new_config| {
        deleted = registry::delete_account(provider, account_id, new_config);
        if deleted {
            registry::sync_selected_ids_with_discoveries(new_config, provider);
        }
    });
    if !deleted {
        log_account_delete_ignored(&app.process_info.id, provider, account_id);
        return Task::none();
    }

    log_account_deleted(&app.process_info.id, provider, account_id);
    runtime::reconcile_provider(&app.config, &app.detection, &mut app.state, provider);
    app.persist_runtime_if_owner("account_deleted");

    if app
        .state
        .provider(provider)
        .is_some_and(|provider| provider.account_status == super::AccountSelectionStatus::Ready)
    {
        return app.request_provider_refresh(provider, RefreshRequestReason::AccountAction);
    }
    Task::none()
}

pub(super) fn start_login(app: &mut AppModel, provider: ProviderId) -> Task<Message> {
    match provider {
        ProviderId::Codex => login::start_login::<login::CodexLoginFlow>(app),
        ProviderId::Claude => login::start_login::<login::ClaudeLoginFlow>(app),
        ProviderId::Gemini => login::start_login::<login::GeminiLoginFlow>(app),
        ProviderId::Copilot => login::start_login::<login::CopilotLoginFlow>(app),
        ProviderId::Minimax => login::start_login::<login::MinimaxLoginFlow>(app),
        ProviderId::Kimi => login::start_login::<login::KimiLoginFlow>(app),
        ProviderId::Antigravity => login::start_login::<login::AntigravityLoginFlow>(app),
        ProviderId::OpenCodeGo => login::start_login::<login::OpenCodeGoLoginFlow>(app),
        ProviderId::Grok => login::start_login::<login::GrokLoginFlow>(app),
        ProviderId::Cursor => Task::none(),
    }
}

pub(super) fn import_from_opencode(
    app: &mut AppModel,
    provider: ProviderId,
    target_account_id: Option<String>,
) -> Task<Message> {
    match provider {
        ProviderId::Codex => login::start_with::<login::CodexLoginFlow>(app, move |config| {
            crate::providers::codex::prepare_opencode_import(config, target_account_id)
        }),
        ProviderId::Copilot => login::start_with::<login::CopilotLoginFlow>(app, move |config| {
            crate::providers::copilot::prepare_opencode_import(config, target_account_id)
        }),
        _ => Task::none(),
    }
}

pub(super) fn restore_from_opencode(
    app: &mut AppModel,
    provider: ProviderId,
    account_id: String,
) -> Task<Message> {
    import_from_opencode(app, provider, Some(account_id))
}

pub(super) fn import_from_grok(
    app: &mut AppModel,
    target_account_id: Option<String>,
) -> Task<Message> {
    login::start_with::<login::GrokLoginFlow>(app, move |config| {
        crate::providers::grok::prepare_host_import(config, target_account_id)
    })
}

pub(super) fn restore_from_grok(app: &mut AppModel, account_id: String) -> Task<Message> {
    import_from_grok(app, Some(account_id))
}

pub(super) fn cancel_login(app: &mut AppModel, provider: ProviderId) {
    match provider {
        ProviderId::Codex => login::cancel_login::<login::CodexLoginFlow>(app),
        ProviderId::Claude => login::cancel_login::<login::ClaudeLoginFlow>(app),
        ProviderId::Gemini => login::cancel_login::<login::GeminiLoginFlow>(app),
        ProviderId::Copilot => login::cancel_login::<login::CopilotLoginFlow>(app),
        ProviderId::Minimax => login::cancel_login::<login::MinimaxLoginFlow>(app),
        ProviderId::Kimi => login::cancel_login::<login::KimiLoginFlow>(app),
        ProviderId::Antigravity => login::cancel_login::<login::AntigravityLoginFlow>(app),
        ProviderId::OpenCodeGo => login::cancel_login::<login::OpenCodeGoLoginFlow>(app),
        ProviderId::Grok => login::cancel_login::<login::GrokLoginFlow>(app),
        ProviderId::Cursor => {}
    }
}

pub(super) fn reauthenticate(
    app: &mut AppModel,
    provider: ProviderId,
    account_id: &str,
) -> Task<Message> {
    match provider {
        ProviderId::Codex => login::reauthenticate::<login::CodexLoginFlow>(app, account_id),
        ProviderId::Claude => login::reauthenticate::<login::ClaudeLoginFlow>(app, account_id),
        ProviderId::Gemini => login::reauthenticate::<login::GeminiLoginFlow>(app, account_id),
        ProviderId::Copilot => login::reauthenticate::<login::CopilotLoginFlow>(app, account_id),
        ProviderId::Minimax => login::reauthenticate::<login::MinimaxLoginFlow>(app, account_id),
        ProviderId::Kimi => login::reauthenticate::<login::KimiLoginFlow>(app, account_id),
        ProviderId::Antigravity => {
            login::reauthenticate::<login::AntigravityLoginFlow>(app, account_id)
        }
        ProviderId::OpenCodeGo => {
            login::reauthenticate::<login::OpenCodeGoLoginFlow>(app, account_id)
        }
        ProviderId::Grok => login::reauthenticate::<login::GrokLoginFlow>(app, account_id),
        ProviderId::Cursor => app.reauthenticate_cursor_account(account_id),
    }
}

pub(super) fn sync_metadata_after_refresh(app: &mut AppModel, provider: ProviderId) {
    match provider {
        ProviderId::Codex => {
            app.update_codex_metadata_from_state();
            app.clear_codex_legacy_snapshot_after_success();
        }
        ProviderId::Claude => {
            app.update_claude_metadata_from_state();
            app.clear_claude_legacy_snapshot_after_success();
        }
        ProviderId::Cursor => {
            app.update_cursor_metadata_from_state();
            app.update_cursor_active_account();
        }
        ProviderId::Gemini
        | ProviderId::Copilot
        | ProviderId::Minimax
        | ProviderId::Kimi
        | ProviderId::Antigravity
        | ProviderId::OpenCodeGo
        | ProviderId::Grok => {}
    }
}

pub(super) fn sync_metadata_after_status_refresh(app: &mut AppModel, provider: ProviderId) {
    if provider == ProviderId::Cursor {
        app.update_cursor_metadata_from_state();
        app.update_cursor_active_account();
    }
}

fn log_account_delete_ignored(process_id: &str, provider: ProviderId, account_id: &str) {
    tracing::info!(
        process_id,
        provider = provider.label(),
        account_id,
        "account deletion ignored because account was not configured"
    );
}

fn log_account_deleted(process_id: &str, provider: ProviderId, account_id: &str) {
    tracing::info!(
        process_id,
        provider = provider.label(),
        account_id,
        "account deleted"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_storage::{
        NewProviderAccount, ProviderAccountStorage, ProviderAccountTokens,
    };
    use crate::config::paths;
    use crate::model::{ProviderAccountRuntimeState, ProviderHealth};
    use chrono::Utc;

    fn test_app() -> AppModel {
        super::super::tests::test_app(None)
    }

    fn seed_account_storage(dir: std::path::PathBuf, provider: ProviderId, id: &str, email: &str) {
        let storage = ProviderAccountStorage::new(dir);
        storage
            .replace_account(
                id.to_string(),
                NewProviderAccount {
                    provider,
                    email: email.to_string(),
                    provider_account_id: None,
                    organization_id: None,
                    organization_name: None,
                    tokens: ProviderAccountTokens {
                        access_token: "access".to_string(),
                        refresh_token: "refresh".to_string(),
                        expires_at: Utc::now() + chrono::Duration::hours(1),
                        scope: Vec::new(),
                        token_id: None,
                    },
                    snapshot: None,
                },
            )
            .unwrap();
    }

    #[test]
    fn delete_account_ignores_unknown_account_for_every_provider() {
        for provider in ProviderId::ALL {
            let _env = crate::test_support::test_env();
            let mut app = test_app();
            let task = delete_account(&mut app, provider, "does-not-exist");
            assert_eq!(task.units(), 0, "{provider:?} should not schedule work");
        }
    }

    #[test]
    fn sync_metadata_after_refresh_clears_codex_legacy_snapshot_once_healthy() {
        let _env = crate::test_support::test_env();
        let mut app = test_app();
        seed_account_storage(
            paths().codex_accounts_dir,
            ProviderId::Codex,
            "codex-1",
            "codex-1@example.com",
        );
        app.config.codex_managed_accounts.push(codex_account());
        app.config.selected_codex_account_ids = vec!["codex-1".to_string()];
        if let Some(provider) = app.state.provider_mut(ProviderId::Codex) {
            provider.selected_account_ids = vec!["codex-1".to_string()];
            provider.legacy_display_snapshot = Some(crate::model::UsageSnapshot {
                provider: ProviderId::Codex,
                source: "legacy".to_string(),
                updated_at: Utc::now(),
                headline: crate::model::UsageHeadline(0),
                windows: Vec::new(),
                provider_cost: None,
                extra_usage: None,
                identity: crate::model::ProviderIdentity::default(),
            });
        }
        let mut account = ProviderAccountRuntimeState::empty(ProviderId::Codex, "codex-1", "Codex");
        account.health = ProviderHealth::Ok;
        account.snapshot = Some(crate::model::UsageSnapshot {
            provider: ProviderId::Codex,
            source: "live".to_string(),
            updated_at: Utc::now(),
            headline: crate::model::UsageHeadline(0),
            windows: Vec::new(),
            provider_cost: None,
            extra_usage: None,
            identity: crate::model::ProviderIdentity::default(),
        });
        app.state.upsert_account(account);

        sync_metadata_after_refresh(&mut app, ProviderId::Codex);

        assert!(
            app.state
                .provider(ProviderId::Codex)
                .unwrap()
                .legacy_display_snapshot
                .is_none()
        );
    }

    #[test]
    fn sync_metadata_after_refresh_is_a_no_op_for_non_managed_providers() {
        let _env = crate::test_support::test_env();
        for provider in [ProviderId::Gemini, ProviderId::Copilot, ProviderId::Minimax] {
            let mut app = test_app();
            let before = app.state.provider(provider).unwrap().clone();
            sync_metadata_after_refresh(&mut app, provider);
            assert_eq!(app.state.provider(provider).unwrap(), &before);
        }
    }

    #[test]
    fn sync_metadata_after_status_refresh_only_touches_cursor() {
        let _env = crate::test_support::test_env();
        let mut app = test_app();
        if let Some(provider) = app.state.provider_mut(ProviderId::Cursor) {
            provider.selected_account_ids = vec!["cursor-managed:cursor-1".to_string()];
        }

        sync_metadata_after_status_refresh(&mut app, ProviderId::Codex);
        assert!(
            app.state
                .provider(ProviderId::Cursor)
                .unwrap()
                .active_account_id
                .is_none()
        );

        sync_metadata_after_status_refresh(&mut app, ProviderId::Cursor);
        assert_eq!(
            app.state
                .provider(ProviderId::Cursor)
                .unwrap()
                .active_account_id
                .as_deref(),
            Some("cursor-managed:cursor-1")
        );
    }

    fn codex_account() -> crate::config::ManagedCodexAccountConfig {
        crate::config::ManagedCodexAccountConfig {
            id: "codex-1".to_string(),
            label: "Codex account".to_string(),
            codex_home: paths().codex_accounts_dir.join("codex-1"),
            email: None,
            provider_account_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_authenticated_at: None,
        }
    }
}
