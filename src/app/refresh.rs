// SPDX-License-Identifier: MPL-2.0

use crate::app::Message;
use crate::config::Config;
use crate::demo_env;
use crate::model::{AppState, AuthState, ProviderId};
use crate::providers::registry;
use crate::runtime::{self, RefreshProcessContext};
use chrono::Utc;
use cosmic::app::Task;

pub(super) struct RefreshSkipDiagnostics {
    pub account_status: &'static str,
    pub selected_account_count: usize,
    pub stored_account_count: usize,
    pub action_required_account_count: usize,
    pub deferred_account_count: usize,
    pub provider_error: Option<String>,
}

impl RefreshSkipDiagnostics {
    #[must_use]
    pub fn for_provider(state: &AppState, provider: ProviderId) -> Self {
        let provider_state = state.provider(provider);
        let accounts = state.accounts_for(provider);
        Self {
            account_status: provider_state
                .map(|entry| account_status_label(entry.account_status.clone()))
                .unwrap_or("missing_provider_state"),
            selected_account_count: provider_state
                .map(|entry| entry.selected_account_ids.len())
                .unwrap_or_default(),
            stored_account_count: accounts.len(),
            action_required_account_count: accounts
                .iter()
                .filter(|account| account.auth_state == AuthState::ActionRequired)
                .count(),
            deferred_account_count: accounts
                .iter()
                .filter(|account| account.is_backing_off())
                .count(),
            provider_error: provider_state.and_then(|entry| entry.error.clone()),
        }
    }

    #[must_use]
    pub fn not_ready_reason(&self) -> &'static str {
        match self.account_status {
            "login_required" => "login_required",
            "selection_required" => "selection_required",
            "unavailable" => {
                if self.stored_account_count == 0 {
                    "no_accounts"
                } else {
                    "account_unavailable"
                }
            }
            "missing_provider_state" => "missing_provider_state",
            _ => "account_status_not_ready",
        }
    }
}

#[cfg(test)]
pub fn refresh_provider_tasks(config: &Config, state: &mut AppState) -> Task<Message> {
    reconcile_host_active_accounts(config, state);

    let tasks = ProviderId::ALL
        .into_iter()
        .map(|provider| refresh_provider_task(config, state, provider))
        .filter(|task| task.units() > 0)
        .collect::<Vec<_>>();

    if tasks.is_empty() {
        Task::none()
    } else {
        Task::batch(tasks)
    }
}

#[cfg(test)]
pub fn automatic_refresh_provider_tasks(config: &Config, state: &mut AppState) -> Task<Message> {
    automatic_refresh_provider_tasks_for_process(config, state, None)
}

pub(super) fn automatic_refresh_provider_tasks_for_process(
    config: &Config,
    state: &mut AppState,
    process: Option<RefreshProcessContext>,
) -> Task<Message> {
    reconcile_host_active_accounts(config, state);

    let providers = ProviderId::ALL
        .into_iter()
        .filter(|provider| selected_account_refresh_due(config, state, *provider))
        .collect::<Vec<_>>();
    let tasks = providers
        .into_iter()
        .map(|provider| {
            refresh_provider_task_for_process(config, state, provider, process.clone(), false)
        })
        .filter(|task| task.units() > 0)
        .collect::<Vec<_>>();

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
    for provider in ProviderId::ALL {
        if let Some(provider_state) = state.provider_mut(provider) {
            provider_state.system_active_account_id =
                registry::system_active_account_id(provider, config);
        }
    }
}

pub(super) fn selected_account_refresh_due(
    config: &Config,
    state: &AppState,
    provider: ProviderId,
) -> bool {
    if !state.provider(provider).is_some_and(|entry| entry.enabled) {
        return false;
    }
    let Some(entry) = state.provider(provider) else {
        return true;
    };
    if entry.is_refreshing || entry.account_status != crate::model::AccountSelectionStatus::Ready {
        return false;
    }

    let account_ids = if config.show_all_accounts {
        let ids = state
            .accounts_for(provider)
            .into_iter()
            .map(|account| account.account_id.as_str())
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return false;
        }
        ids
    } else {
        if entry.selected_account_ids.is_empty() {
            return false;
        }
        entry
            .selected_account_ids
            .iter()
            .map(String::as_str)
            .collect()
    };

    let interval = chrono::Duration::seconds(
        config
            .refresh_interval_seconds
            .min(i64::MAX as u64)
            .cast_signed(),
    );
    let now = Utc::now();
    account_ids.iter().any(|account_id| {
        let Some(account) = state
            .provider_accounts
            .iter()
            .find(|account| account.provider == provider && account.account_id == *account_id)
        else {
            return true;
        };
        if account.is_backing_off() {
            return false;
        }
        account
            .last_success_at
            .is_none_or(|last_success_at| now - last_success_at >= interval)
    })
}

#[cfg(test)]
pub fn refresh_provider_task(
    config: &Config,
    state: &mut AppState,
    provider: ProviderId,
) -> Task<Message> {
    refresh_provider_task_for_process(config, state, provider, None, false)
}

pub(super) fn refresh_provider_task_for_process(
    config: &Config,
    state: &mut AppState,
    provider: ProviderId,
    process: Option<RefreshProcessContext>,
    force: bool,
) -> Task<Message> {
    if demo_env::is_active() {
        return Task::none();
    }
    tracing::info!(
        process_id = process
            .as_ref()
            .map(|process| process.process_id.as_str())
            .unwrap_or("unknown"),
        owner_status = process
            .as_ref()
            .map(|process| process.owner_status)
            .unwrap_or("unknown"),
        provider = provider.label(),
        "provider refresh scheduled"
    );
    let enabled = state.provider(provider).is_some_and(|entry| entry.enabled);
    let already_refreshing = state
        .provider(provider)
        .is_some_and(|entry| entry.is_refreshing);
    let ready = state
        .provider(provider)
        .is_some_and(|entry| entry.account_status == crate::model::AccountSelectionStatus::Ready);
    if !enabled || !ready || already_refreshing {
        state.mark_provider_refreshing(provider, enabled);
        let diagnostics = RefreshSkipDiagnostics::for_provider(state, provider);
        if !enabled {
            tracing::info!(
                provider = provider.label(),
                skip_reason = "disabled",
                "provider refresh skipped because provider is disabled"
            );
        } else if already_refreshing {
            tracing::info!(
                provider = provider.label(),
                skip_reason = "already_refreshing",
                account_status = diagnostics.account_status,
                selected_account_count = diagnostics.selected_account_count,
                stored_account_count = diagnostics.stored_account_count,
                "provider refresh skipped because provider is already refreshing"
            );
        } else {
            let skip_reason = diagnostics.not_ready_reason();
            if skip_reason == "login_required" {
                tracing::info!(
                    provider = provider.label(),
                    skip_reason,
                    account_status = diagnostics.account_status,
                    selected_account_count = diagnostics.selected_account_count,
                    stored_account_count = diagnostics.stored_account_count,
                    action_required_account_count = diagnostics.action_required_account_count,
                    deferred_account_count = diagnostics.deferred_account_count,
                    provider_error = diagnostics.provider_error.as_deref(),
                    "provider refresh ignored because provider has no account"
                );
            } else {
                tracing::info!(
                    provider = provider.label(),
                    skip_reason,
                    account_status = diagnostics.account_status,
                    selected_account_count = diagnostics.selected_account_count,
                    stored_account_count = diagnostics.stored_account_count,
                    action_required_account_count = diagnostics.action_required_account_count,
                    deferred_account_count = diagnostics.deferred_account_count,
                    provider_error = diagnostics.provider_error.as_deref(),
                    "provider refresh skipped because selected account state is not ready"
                );
            }
        }
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
            tracing::debug!(
                provider = provider.label(),
                account_id = %account.account_id,
                "skipping refresh for account requiring user action"
            );
        }
    }

    let account_ids = account_ids_to_refresh(
        &config,
        provider,
        previous.as_ref(),
        &previous_accounts,
        force,
    );
    if account_ids.is_empty() {
        tracing::info!(
            provider = provider.label(),
            skip_reason = "no_refreshable_accounts",
            selected_account_count = previous
                .as_ref()
                .map(|provider| provider.selected_account_ids.len())
                .unwrap_or_default(),
            stored_account_count = previous_accounts.len(),
            action_required_account_count = previous_accounts
                .iter()
                .filter(|account| account.auth_state == AuthState::ActionRequired)
                .count(),
            deferred_account_count = previous_accounts
                .iter()
                .filter(|account| account.is_backing_off())
                .count(),
            "provider refresh skipped because no accounts are refreshable"
        );
        return Task::none();
    }

    state.mark_provider_refreshing(provider, enabled);

    let tasks: Vec<Task<Message>> = account_ids
        .into_iter()
        .map(|account_id| {
            let config = config.clone();
            let previous = previous.clone();
            let previous_accounts = previous_accounts.clone();
            let process = process.clone();
            Task::perform(
                async move {
                    runtime::refresh_account(
                        config,
                        provider,
                        enabled,
                        account_id,
                        previous,
                        previous_accounts,
                        process,
                    )
                    .await
                },
                |result| cosmic::Action::App(Message::ProviderRefreshed(Box::new(result))),
            )
        })
        .collect();

    Task::batch(tasks)
}

fn account_status_label(status: crate::model::AccountSelectionStatus) -> &'static str {
    match status {
        crate::model::AccountSelectionStatus::Ready => "ready",
        crate::model::AccountSelectionStatus::LoginRequired => "login_required",
        crate::model::AccountSelectionStatus::SelectionRequired => "selection_required",
        crate::model::AccountSelectionStatus::Unavailable => "unavailable",
    }
}

#[must_use]
pub fn should_refresh_account_statuses(state: &AppState, provider: ProviderId) -> bool {
    state.provider(provider).is_some_and(|entry| entry.enabled)
        && registry::supports_background_status_refresh(provider)
        && !state.accounts_for(provider).is_empty()
}

pub fn refresh_provider_account_statuses_task(
    config: &Config,
    state: &AppState,
    provider: ProviderId,
) -> Task<Message> {
    if !should_refresh_account_statuses(state, provider) {
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
    force: bool,
) -> Vec<String> {
    refresh_candidate_account_ids(config, provider, previous, previous_accounts)
        .into_iter()
        .filter(|id| {
            !previous_accounts.iter().any(|a| {
                &a.account_id == id
                    && ((!force && a.is_backing_off()) || a.auth_state == AuthState::ActionRequired)
            })
        })
        .collect()
}

fn refresh_candidate_account_ids(
    config: &Config,
    provider: ProviderId,
    previous: Option<&crate::model::ProviderRuntimeState>,
    previous_accounts: &[crate::model::ProviderAccountRuntimeState],
) -> Vec<String> {
    if config.show_all_accounts {
        let stored_ids = previous_accounts
            .iter()
            .map(|account| account.account_id.clone())
            .collect::<Vec<_>>();
        if !stored_ids.is_empty() {
            return stored_ids;
        }
        return registry::discover_accounts(provider, config)
            .into_iter()
            .map(|account| account.account_id)
            .collect();
    }

    let config_ids = config.selected_account_ids(provider);
    if !config_ids.is_empty() {
        config_ids.to_vec()
    } else if let Some(prev_id) = previous.and_then(|p| p.selected_account_ids.first()) {
        vec![prev_id.clone()]
    } else {
        registry::discover_accounts(provider, config)
            .into_iter()
            .next()
            .map(|a| vec![a.account_id])
            .unwrap_or_default()
    }
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

    fn test_env_without_demo() -> test_support::TestEnv {
        let mut env = test_support::test_env();
        env.remove("YAPCAP_DEMO");
        env
    }

    fn runtime_account(
        provider: ProviderId,
        account_id: &str,
        last_success_at: Option<chrono::DateTime<Utc>>,
    ) -> crate::model::ProviderAccountRuntimeState {
        crate::model::ProviderAccountRuntimeState {
            provider,
            account_id: account_id.to_string(),
            label: account_id.to_string(),
            source_label: None,
            last_success_at,
            snapshot: None,
            health: crate::model::ProviderHealth::Ok,
            auth_state: AuthState::Ready,
            error: None,
            retry_after: None,
            consecutive_failures: 0,
        }
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
        let _env = test_env_without_demo();
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
        let _env = test_env_without_demo();
        let config = Config {
            cursor_enablement: crate::config::ProviderEnablement::Disabled,
            ..Config::default()
        };
        let mut state = AppState::empty();
        mark_all_ready(&mut state);
        for p in &mut state.providers {
            p.enabled = p.provider != ProviderId::Cursor;
        }

        let _tasks = refresh_provider_tasks(&config, &mut state);

        let cursor = state.provider(ProviderId::Cursor).unwrap();
        assert!(!cursor.enabled);
        assert!(!cursor.is_refreshing);
    }

    #[test]
    fn automatic_refresh_tasks_refresh_missing_selected_account() {
        let _env = test_env_without_demo();
        let config = Config::default();
        let mut state = AppState::empty();
        mark_all_ready(&mut state);

        let _tasks = automatic_refresh_provider_tasks(&config, &mut state);

        assert!(state.provider(ProviderId::Codex).unwrap().is_refreshing);
    }

    #[test]
    fn automatic_refresh_tasks_skip_fresh_selected_account() {
        let _env = test_env_without_demo();
        let config = Config::default();
        let mut state = AppState::empty();
        mark_all_ready(&mut state);
        state.upsert_account(crate::model::ProviderAccountRuntimeState {
            provider: ProviderId::Codex,
            account_id: "default".to_string(),
            label: "Codex".to_string(),
            source_label: None,
            last_success_at: Some(Utc::now()),
            snapshot: None,
            health: crate::model::ProviderHealth::Ok,
            auth_state: AuthState::Ready,
            error: None,
            retry_after: None,
            consecutive_failures: 0,
        });

        let _tasks = automatic_refresh_provider_tasks(&config, &mut state);

        assert!(!state.provider(ProviderId::Codex).unwrap().is_refreshing);
    }

    #[test]
    fn automatic_refresh_tasks_refresh_stale_selected_account() {
        let _env = test_env_without_demo();
        let config = Config::default();
        let mut state = AppState::empty();
        mark_all_ready(&mut state);
        state.upsert_account(crate::model::ProviderAccountRuntimeState {
            provider: ProviderId::Codex,
            account_id: "default".to_string(),
            label: "Codex".to_string(),
            source_label: None,
            last_success_at: Some(Utc::now() - chrono::Duration::minutes(10)),
            snapshot: None,
            health: crate::model::ProviderHealth::Ok,
            auth_state: AuthState::Ready,
            error: None,
            retry_after: None,
            consecutive_failures: 0,
        });

        let _tasks = automatic_refresh_provider_tasks(&config, &mut state);

        assert!(state.provider(ProviderId::Codex).unwrap().is_refreshing);
    }

    #[test]
    fn account_ids_to_refresh_returns_all_stored_accounts_when_show_all_accounts() {
        let _env = test_env_without_demo();
        let config = Config {
            show_all_accounts: true,
            selected_codex_account_ids: vec!["acct-a".to_string()],
            ..Config::default()
        };
        let accounts = vec![
            runtime_account(ProviderId::Codex, "acct-a", Some(Utc::now())),
            runtime_account(ProviderId::Codex, "acct-b", Some(Utc::now())),
        ];

        let ids = account_ids_to_refresh(&config, ProviderId::Codex, None, &accounts, false);

        assert_eq!(ids, vec!["acct-a".to_string(), "acct-b".to_string()]);
    }

    #[test]
    fn account_ids_to_refresh_returns_selected_only_when_not_show_all_accounts() {
        let _env = test_env_without_demo();
        let config = Config {
            show_all_accounts: false,
            selected_codex_account_ids: vec!["acct-a".to_string()],
            ..Config::default()
        };
        let accounts = vec![
            runtime_account(ProviderId::Codex, "acct-a", Some(Utc::now())),
            runtime_account(ProviderId::Codex, "acct-b", Some(Utc::now())),
        ];

        let ids = account_ids_to_refresh(&config, ProviderId::Codex, None, &accounts, false);

        assert_eq!(ids, vec!["acct-a".to_string()]);
    }

    #[test]
    fn selected_account_refresh_due_checks_all_accounts_only_when_show_all_accounts() {
        let _env = test_env_without_demo();
        let mut state = AppState::empty();
        mark_all_ready(&mut state);
        if let Some(entry) = state.provider_mut(ProviderId::Codex) {
            entry.selected_account_ids = vec!["acct-a".to_string()];
        }
        state.upsert_account(runtime_account(
            ProviderId::Codex,
            "acct-a",
            Some(Utc::now()),
        ));
        state.upsert_account(runtime_account(
            ProviderId::Codex,
            "acct-b",
            Some(Utc::now() - chrono::Duration::minutes(10)),
        ));

        let show_all = Config {
            show_all_accounts: true,
            selected_codex_account_ids: vec!["acct-a".to_string()],
            ..Config::default()
        };
        let selected_only = Config {
            show_all_accounts: false,
            selected_codex_account_ids: vec!["acct-a".to_string()],
            ..Config::default()
        };

        assert!(selected_account_refresh_due(
            &show_all,
            &state,
            ProviderId::Codex
        ));
        assert!(!selected_account_refresh_due(
            &selected_only,
            &state,
            ProviderId::Codex
        ));
    }

    #[test]
    fn automatic_refresh_tasks_skip_backing_off_selected_account() {
        let _env = test_env_without_demo();
        let config = Config::default();
        let mut state = AppState::empty();
        mark_all_ready(&mut state);
        state.upsert_account(crate::model::ProviderAccountRuntimeState {
            provider: ProviderId::Codex,
            account_id: "default".to_string(),
            label: "Codex".to_string(),
            source_label: None,
            last_success_at: None,
            snapshot: None,
            health: crate::model::ProviderHealth::Error,
            auth_state: AuthState::Error,
            error: Some("boom".to_string()),
            retry_after: Some(Utc::now() + chrono::Duration::minutes(5)),
            consecutive_failures: 1,
        });

        let _tasks = automatic_refresh_provider_tasks(&config, &mut state);

        assert!(!state.provider(ProviderId::Codex).unwrap().is_refreshing);
    }

    #[test]
    fn refresh_provider_task_does_not_wedge_when_all_accounts_backing_off() {
        let _env = test_env_without_demo();
        let config = Config::default();
        let mut state = AppState::empty();
        mark_all_ready(&mut state);
        state.upsert_account(crate::model::ProviderAccountRuntimeState {
            provider: ProviderId::Codex,
            account_id: "default".to_string(),
            label: "Codex".to_string(),
            source_label: None,
            last_success_at: None,
            snapshot: None,
            health: crate::model::ProviderHealth::Error,
            auth_state: AuthState::Error,
            error: Some("boom".to_string()),
            retry_after: Some(Utc::now() + chrono::Duration::minutes(5)),
            consecutive_failures: 1,
        });

        let task = refresh_provider_task(&config, &mut state, ProviderId::Codex);

        assert_eq!(task.units(), 0);
        assert!(!state.provider(ProviderId::Codex).unwrap().is_refreshing);
    }

    #[test]
    fn forced_refresh_retries_backing_off_account() {
        let _env = test_env_without_demo();
        let config = Config::default();
        let mut state = AppState::empty();
        mark_all_ready(&mut state);
        state.upsert_account(crate::model::ProviderAccountRuntimeState {
            provider: ProviderId::Codex,
            account_id: "default".to_string(),
            label: "Codex".to_string(),
            source_label: None,
            last_success_at: None,
            snapshot: None,
            health: crate::model::ProviderHealth::Error,
            auth_state: AuthState::Error,
            error: Some("boom".to_string()),
            retry_after: Some(Utc::now() + chrono::Duration::minutes(5)),
            consecutive_failures: 1,
        });

        let task =
            refresh_provider_task_for_process(&config, &mut state, ProviderId::Codex, None, true);

        assert!(task.units() > 0);
        assert!(state.provider(ProviderId::Codex).unwrap().is_refreshing);
    }

    #[test]
    fn forced_refresh_retries_opencode_go_entitlement_failure() {
        let _env = test_env_without_demo();
        let config = Config::default();
        let mut state = AppState::empty();
        mark_all_ready(&mut state);
        state.upsert_account(crate::model::ProviderAccountRuntimeState {
            provider: ProviderId::OpenCodeGo,
            account_id: "opencode-go-1".to_string(),
            label: "OpenCode Go".to_string(),
            source_label: None,
            last_success_at: None,
            snapshot: None,
            health: crate::model::ProviderHealth::Error,
            auth_state: AuthState::Error,
            error: Some("OpenCode Go subscription required".to_string()),
            retry_after: Some(Utc::now() + chrono::Duration::minutes(5)),
            consecutive_failures: 1,
        });

        let task = refresh_provider_task_for_process(
            &config,
            &mut state,
            ProviderId::OpenCodeGo,
            None,
            true,
        );

        assert!(task.units() > 0);
        assert!(
            state
                .provider(ProviderId::OpenCodeGo)
                .unwrap()
                .is_refreshing
        );
    }

    #[test]
    fn forced_refresh_still_skips_action_required_account() {
        let _env = test_env_without_demo();
        let config = Config::default();
        let mut state = AppState::empty();
        mark_all_ready(&mut state);
        state.upsert_account(crate::model::ProviderAccountRuntimeState {
            provider: ProviderId::Codex,
            account_id: "default".to_string(),
            label: "Codex".to_string(),
            source_label: None,
            last_success_at: None,
            snapshot: None,
            health: crate::model::ProviderHealth::Error,
            auth_state: AuthState::ActionRequired,
            error: Some("login required".to_string()),
            retry_after: None,
            consecutive_failures: 1,
        });

        let task =
            refresh_provider_task_for_process(&config, &mut state, ProviderId::Codex, None, true);

        assert_eq!(task.units(), 0);
        assert!(!state.provider(ProviderId::Codex).unwrap().is_refreshing);
    }

    #[test]
    fn refresh_tasks_skip_already_refreshing_provider() {
        let _env = test_env_without_demo();
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
