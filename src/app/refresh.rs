// SPDX-License-Identifier: MPL-2.0

use crate::app::Message;
use crate::config::Config;
use crate::demo_env;
use crate::model::{AppState, AuthState, ProviderId};
use crate::providers::registry;
use crate::runtime;
use cosmic::app::Task;

pub fn refresh_provider_tasks(config: &Config, state: &mut AppState) -> Task<Message> {
    reconcile_host_active_accounts(config, state);

    let mut tasks = Vec::new();

    for provider in ProviderId::ALL {
        let task = refresh_provider_task(config, state, provider);
        if task.units() > 0 {
            tasks.push(task);
        }
    }

    if tasks.is_empty() {
        Task::none()
    } else {
        Task::batch(tasks)
    }
}

fn reconcile_host_active_accounts(config: &Config, state: &mut AppState) {
    if demo_env::is_active() {
        return;
    }
    if let Some(provider) = state.provider_mut(ProviderId::Codex) {
        provider.system_active_account_id =
            registry::codex_system_active_account_id(&config.codex_managed_accounts);
    }
    if let Some(provider) = state.provider_mut(ProviderId::Claude) {
        provider.system_active_account_id =
            registry::claude_system_active_account_id(&config.claude_managed_accounts);
    }
    if let Some(provider) = state.provider_mut(ProviderId::Gemini) {
        provider.system_active_account_id =
            registry::gemini_system_active_account_id(&config.gemini_managed_accounts);
    }
}

pub fn refresh_provider_task(
    config: &Config,
    state: &mut AppState,
    provider: ProviderId,
) -> Task<Message> {
    if demo_env::is_active() {
        return Task::none();
    }
    let enabled = config.provider_enabled(provider);
    let already_refreshing = state
        .provider(provider)
        .is_some_and(|entry| entry.is_refreshing);
    state.mark_provider_refreshing(provider, enabled);

    let ready = state
        .provider(provider)
        .is_some_and(|entry| entry.account_status == crate::model::AccountSelectionStatus::Ready);
    if !enabled || !ready || already_refreshing {
        return Task::none();
    }

    let config = config.clone();
    let previous = state.provider(provider).cloned();
    let previous_accounts = state
        .accounts_for(provider)
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();

    for account in &previous_accounts {
        if account.auth_state == AuthState::ActionRequired {
            tracing::info!(
                provider = provider.label(),
                account_id = %account.account_id,
                "skipping refresh for account requiring user action"
            );
        }
    }

    let account_ids =
        account_ids_to_refresh(&config, provider, previous.as_ref(), &previous_accounts);
    if account_ids.is_empty() {
        return Task::none();
    }

    let tasks: Vec<Task<Message>> = account_ids
        .into_iter()
        .map(|account_id| {
            let config = config.clone();
            let previous = previous.clone();
            let previous_accounts = previous_accounts.clone();
            Task::perform(
                async move {
                    runtime::refresh_account(
                        config,
                        provider,
                        account_id,
                        previous,
                        previous_accounts,
                    )
                    .await
                },
                |result| cosmic::Action::App(Message::ProviderRefreshed(Box::new(result))),
            )
        })
        .collect();

    Task::batch(tasks)
}

pub fn refresh_provider_account_statuses_task(
    config: &Config,
    state: &AppState,
    provider: ProviderId,
) -> Task<Message> {
    if !config.provider_enabled(provider) || !registry::supports_background_status_refresh(provider)
    {
        return Task::none();
    }

    let config = config.clone();
    let previous_accounts = state
        .accounts_for(provider)
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    Task::perform(
        async move {
            runtime::refresh_provider_account_statuses(provider, config, previous_accounts).await
        },
        move |accounts| {
            cosmic::Action::App(Message::ProviderAccountStatusesRefreshed(
                provider, accounts,
            ))
        },
    )
}

fn account_ids_to_refresh(
    config: &Config,
    provider: ProviderId,
    previous: Option<&crate::model::ProviderRuntimeState>,
    previous_accounts: &[crate::model::ProviderAccountRuntimeState],
) -> Vec<String> {
    let config_ids = config.selected_account_ids(provider);
    let candidate_ids = if !config_ids.is_empty() {
        config_ids.to_vec()
    } else if let Some(prev_id) = previous.and_then(|p| p.selected_account_ids.first()) {
        vec![prev_id.clone()]
    } else {
        registry::discover_accounts(provider, config)
            .into_iter()
            .next()
            .map(|a| vec![a.account_id])
            .unwrap_or_default()
    };

    candidate_ids
        .into_iter()
        .filter(|id| {
            !previous_accounts.iter().any(|a| {
                &a.account_id == id
                    && (a.is_rate_limited() || a.auth_state == AuthState::ActionRequired)
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_storage::{
        NewProviderAccount, ProviderAccountStorage, ProviderAccountTokens,
    };
    use crate::config::{ManagedClaudeAccountConfig, paths};
    use crate::model::AccountSelectionStatus;
    use crate::test_support;
    use chrono::Utc;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn mark_all_ready(state: &mut AppState) {
        for provider in &mut state.providers {
            provider.account_status = AccountSelectionStatus::Ready;
            provider.selected_account_ids = vec!["default".to_string()];
        }
    }

    fn temp_state_root(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("yapcap-refresh-{name}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn stored_claude_account(
        storage: &ProviderAccountStorage,
        email: &str,
        provider_account_id: &str,
    ) -> ManagedClaudeAccountConfig {
        let stored = storage
            .create_account(NewProviderAccount {
                provider: ProviderId::Claude,
                email: email.to_string(),
                provider_account_id: Some(provider_account_id.to_string()),
                organization_id: None,
                organization_name: None,
                tokens: ProviderAccountTokens {
                    access_token: "access".to_string(),
                    refresh_token: "refresh".to_string(),
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
    fn refresh_tasks_mark_enabled_providers_refreshing() {
        let config = Config::default();
        let mut state = AppState::empty();
        mark_all_ready(&mut state);

        let _tasks = refresh_provider_tasks(&config, &mut state);

        for provider in ProviderId::ALL {
            let entry = state.provider(provider).unwrap();
            assert!(entry.enabled);
            assert!(entry.is_refreshing);
        }
    }

    #[test]
    fn refresh_tasks_skip_disabled_provider() {
        let config = Config {
            cursor_enabled: false,
            ..Config::default()
        };
        let mut state = AppState::empty();
        mark_all_ready(&mut state);
        for p in &mut state.providers {
            p.enabled = config.provider_enabled(p.provider);
        }

        let _tasks = refresh_provider_tasks(&config, &mut state);

        let cursor = state.provider(ProviderId::Cursor).unwrap();
        assert!(!cursor.enabled);
        assert!(!cursor.is_refreshing);
    }

    #[test]
    fn refresh_tasks_skip_already_refreshing_provider() {
        let config = Config::default();
        let mut state = AppState::empty();
        mark_all_ready(&mut state);
        state.mark_provider_refreshing(ProviderId::Codex, true);

        let _tasks = refresh_provider_tasks(&config, &mut state);

        assert!(state.provider(ProviderId::Codex).unwrap().is_refreshing);
    }

    #[test]
    fn refresh_tasks_reconcile_claude_host_active_account_before_fetching() {
        let mut env = test_support::test_env();
        let state_root = temp_state_root("claude-active");
        env.set("HOME", state_root.as_os_str());
        env.set("XDG_STATE_HOME", state_root.as_os_str());
        env.remove("FLATPAK_ID");
        let storage = ProviderAccountStorage::new(paths().claude_accounts_dir.clone());
        let account_a = stored_claude_account(&storage, "a@example.com", "acct-a");
        let account_b = stored_claude_account(&storage, "b@example.com", "acct-b");
        fs::write(
            state_root.join(".claude.json"),
            r#"{"oauthAccount":{"accountUuid":"acct-b","emailAddress":"b@example.com"}}"#,
        )
        .unwrap();
        let config = Config {
            claude_managed_accounts: vec![account_a, account_b.clone()],
            selected_claude_account_ids: vec![account_b.id.clone()],
            ..Config::default()
        };
        let mut state = AppState::empty();
        mark_all_ready(&mut state);

        let _tasks = refresh_provider_tasks(&config, &mut state);

        assert_eq!(
            state
                .provider(ProviderId::Claude)
                .and_then(|p| p.system_active_account_id.as_deref()),
            Some(account_b.id.as_str())
        );
    }
}
