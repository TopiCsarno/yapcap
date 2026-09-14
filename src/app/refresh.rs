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

#[cfg(test)]
pub(super) fn automatic_refresh_provider_tasks_for_process(
    config: &Config,
    state: &mut AppState,
    process: Option<RefreshProcessContext>,
) -> Task<Message> {
    reconcile_host_active_accounts(config, state);

    let providers = ProviderId::ALL
        .into_iter()
        .filter(|provider| provider_refresh_due(config, state, *provider))
        .collect::<Vec<_>>();
    let tasks = providers
        .into_iter()
        .map(|provider| {
            refresh_provider_task_for_process(
                config,
                state,
                provider,
                process.clone(),
                false,
                0,
                eligible_refresh_account_ids(config, provider, state, false),
            )
        })
        .filter(|task| task.units() > 0)
        .collect::<Vec<_>>();

    if tasks.is_empty() {
        Task::none()
    } else {
        Task::batch(tasks)
    }
}

pub(super) fn reconcile_host_active_accounts(config: &Config, state: &mut AppState) {
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

pub(super) fn provider_refresh_due(
    config: &Config,
    state: &AppState,
    provider: ProviderId,
) -> bool {
    if !state.provider(provider).is_some_and(|entry| entry.enabled) {
        return false;
    }
    let Some(entry) = state.provider(provider) else {
        return false;
    };
    let candidate_ids = eligible_refresh_account_ids(config, provider, state, false);
    let flagged_ids = eligible_flagged_refresh_account_ids(config, provider, state, false);
    let singleton_adoption =
        config.selected_account_ids(provider).is_empty() && candidate_ids.len() == 1;
    if entry.is_refreshing
        || candidate_ids.is_empty()
        || (entry.account_status != crate::model::AccountSelectionStatus::Ready
            && flagged_ids.is_empty()
            && !singleton_adoption)
    {
        return false;
    }

    let interval = chrono::Duration::seconds(
        config
            .refresh_interval_seconds
            .min(i64::MAX as u64)
            .cast_signed(),
    );
    let now = Utc::now();
    candidate_ids.iter().any(|account_id| {
        let Some(account) = state
            .provider_accounts
            .iter()
            .find(|account| account.provider == provider && account.account_id == *account_id)
        else {
            return true;
        };
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
    let account_ids = eligible_refresh_account_ids(config, provider, state, false);
    refresh_provider_task_for_process(config, state, provider, None, false, 0, account_ids)
}

pub(super) fn refresh_provider_task_for_process(
    config: &Config,
    state: &mut AppState,
    provider: ProviderId,
    process: Option<RefreshProcessContext>,
    _force: bool,
    batch_token: u64,
    account_ids: Vec<String>,
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
    if !enabled || (batch_token == 0 && (!ready || already_refreshing)) {
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
                        runtime::RefreshAccountInput {
                            provider,
                            enabled,
                            account_id,
                            batch_token,
                            previous,
                            previous_accounts,
                            process,
                        },
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

pub(super) fn eligible_refresh_account_ids(
    config: &Config,
    provider: ProviderId,
    state: &AppState,
    force: bool,
) -> Vec<String> {
    let configured_ids = refresh_account_ids(
        config.selected_account_ids(provider),
        config.panel_account_ids(provider),
    );
    let discovered = registry::discover_accounts(provider, config);
    let candidate_ids = if !configured_ids.is_empty() {
        configured_ids
    } else if discovered.len() == 1 {
        vec![discovered[0].account_id.clone()]
    } else {
        Vec::new()
    };

    candidate_ids
        .into_iter()
        .filter(|id| {
            discovered.iter().any(|account| account.account_id == *id)
                && !state.accounts_for(provider).iter().any(|a| {
                    &a.account_id == id
                        && ((!force && a.is_backing_off())
                            || a.auth_state == AuthState::ActionRequired)
                })
        })
        .collect()
}

pub(super) fn eligible_flagged_refresh_account_ids(
    config: &Config,
    provider: ProviderId,
    state: &AppState,
    force: bool,
) -> Vec<String> {
    let eligible = eligible_refresh_account_ids(config, provider, state, force);
    config
        .panel_account_ids(provider)
        .iter()
        .filter(|id| eligible.contains(id))
        .cloned()
        .collect()
}

fn refresh_account_ids(selected_ids: &[String], panel_ids: &[String]) -> Vec<String> {
    selected_ids
        .iter()
        .chain(panel_ids)
        .fold(Vec::new(), |mut ids, account_id| {
            if !ids.contains(account_id) {
                ids.push(account_id.clone());
            }
            ids
        })
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
    fn refresh_tasks_skip_undiscoverable_default_ids() {
        let _env = test_env_without_demo();
        let config = Config::default();
        let mut state = AppState::empty();
        mark_all_ready(&mut state);

        let _tasks = refresh_provider_tasks(&config, &mut state);

        for provider in ProviderId::ALL {
            let entry = state.provider(provider).unwrap();
            assert!(entry.enabled);
            assert!(!entry.is_refreshing);
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
    fn automatic_refresh_tasks_skip_missing_selected_account() {
        let _env = test_env_without_demo();
        let config = Config::default();
        let mut state = AppState::empty();
        mark_all_ready(&mut state);

        let _tasks = automatic_refresh_provider_tasks(&config, &mut state);

        assert!(!state.provider(ProviderId::Codex).unwrap().is_refreshing);
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
    fn automatic_refresh_tasks_skip_undiscoverable_stale_selected_account() {
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

        assert!(!state.provider(ProviderId::Codex).unwrap().is_refreshing);
    }

    #[test]
    fn automatic_refresh_tasks_skip_undiscoverable_stale_flagged_account() {
        let _env = test_env_without_demo();
        let config = Config {
            panel_codex_account_ids: vec!["flagged".to_string()],
            ..Config::default()
        };
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
        state.upsert_account(crate::model::ProviderAccountRuntimeState {
            provider: ProviderId::Codex,
            account_id: "flagged".to_string(),
            label: "Flagged Codex".to_string(),
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

        assert!(!state.provider(ProviderId::Codex).unwrap().is_refreshing);
    }

    #[test]
    fn automatic_refresh_tasks_skip_when_selected_and_flagged_accounts_are_fresh() {
        let _env = test_env_without_demo();
        let config = Config {
            panel_codex_account_ids: vec!["flagged".to_string()],
            ..Config::default()
        };
        let mut state = AppState::empty();
        mark_all_ready(&mut state);
        for account_id in ["default", "flagged"] {
            state.upsert_account(crate::model::ProviderAccountRuntimeState {
                provider: ProviderId::Codex,
                account_id: account_id.to_string(),
                label: account_id.to_string(),
                source_label: None,
                last_success_at: Some(Utc::now()),
                snapshot: None,
                health: crate::model::ProviderHealth::Ok,
                auth_state: AuthState::Ready,
                error: None,
                retry_after: None,
                consecutive_failures: 0,
            });
        }

        let _tasks = automatic_refresh_provider_tasks(&config, &mut state);

        assert!(!state.provider(ProviderId::Codex).unwrap().is_refreshing);
    }

    #[test]
    fn account_ids_to_refresh_unions_selected_and_flagged_ids_in_order() {
        let config = Config {
            selected_codex_account_ids: vec!["selected".to_string(), "duplicate".to_string()],
            panel_codex_account_ids: vec!["duplicate".to_string(), "flagged".to_string()],
            ..Config::default()
        };

        let account_ids = refresh_account_ids(
            config.selected_account_ids(ProviderId::Codex),
            config.panel_account_ids(ProviderId::Codex),
        );

        assert_eq!(
            account_ids,
            vec![
                "selected".to_string(),
                "duplicate".to_string(),
                "flagged".to_string()
            ]
        );
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
    fn forced_refresh_skips_undiscoverable_account() {
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

        let task = refresh_provider_task_for_process(
            &config,
            &mut state,
            ProviderId::Codex,
            None,
            true,
            0,
            Vec::new(),
        );

        assert_eq!(task.units(), 0);
        assert!(!state.provider(ProviderId::Codex).unwrap().is_refreshing);
    }

    #[test]
    fn forced_refresh_skips_undiscoverable_opencode_go_account() {
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
            0,
            Vec::new(),
        );

        assert_eq!(task.units(), 0);
        assert!(
            !state
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

        let task = refresh_provider_task_for_process(
            &config,
            &mut state,
            ProviderId::Codex,
            None,
            true,
            0,
            Vec::new(),
        );

        assert_eq!(task.units(), 0);
        assert!(!state.provider(ProviderId::Codex).unwrap().is_refreshing);
    }

    #[test]
    fn refresh_tasks_skip_already_refreshing_provider() {
        let _env = test_env_without_demo();
        let config = Config::default();
        let mut state = AppState::empty();
        mark_all_ready(&mut state);
        state.provider_mut(ProviderId::Codex).unwrap().is_refreshing = true;

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
