// SPDX-License-Identifier: MPL-2.0

use crate::config::{APP_ID, Config};
use crate::demo_env;
use crate::error::AppError;
use crate::model::{
    AccountSelectionStatus, AppState, AuthState, ProviderAccountRuntimeState, ProviderHealth,
    ProviderId, ProviderRuntimeState, UsageSnapshot,
};
use crate::providers;
use crate::shared_state::{SharedRuntimeState, SharedStateWriter};
use chrono::Utc;
use std::time::Duration;

pub(crate) const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
pub(crate) const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(debug_assertions)]
const DEBUG_OFFLINE_ENV: &str = "YAPCAP_DEBUG_OFFLINE";
#[cfg(debug_assertions)]
const DEBUG_OFFLINE_PROXY: &str = "http://127.0.0.1:9";

#[derive(Debug, Clone)]
pub struct ProviderRefreshResult {
    pub provider: ProviderRuntimeState,
    pub accounts: Vec<ProviderAccountRuntimeState>,
}

#[derive(Clone)]
pub struct RefreshProcessContext {
    pub process_id: String,
    pub owner_status: &'static str,
}

pub fn load_initial_state(
    config: &Config,
    detection: &crate::detection::DetectionSnapshot,
    shared_runtime: Option<SharedRuntimeState>,
) -> AppState {
    let mut state = match shared_runtime {
        Some(shared) => shared.app_state,
        None => {
            tracing::info!("shared runtime unavailable; starting from empty runtime state");
            AppState::empty()
        }
    };
    reconcile_state(config, detection, &mut state);
    state
}

pub fn persist_state_as(
    state: &AppState,
    reason: &'static str,
    writer: Option<SharedStateWriter<'_>>,
) {
    if demo_env::is_active() {
        return;
    }
    if let Err(error) = crate::shared_state::save_runtime_as(APP_ID, state, reason, writer) {
        tracing::error!(error = ?error, "failed to save shared runtime state");
    }
}

#[must_use]
pub fn http_client() -> reqwest::Client {
    let builder = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .connect_timeout(HTTP_CONNECT_TIMEOUT);
    #[cfg(debug_assertions)]
    let builder = apply_debug_offline_proxy(builder);

    builder.build().unwrap_or_else(|error| {
        tracing::warn!(error = %error, "failed to build timed HTTP client; using reqwest default");
        reqwest::Client::new()
    })
}

#[cfg(debug_assertions)]
fn apply_debug_offline_proxy(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    if !std::env::var(DEBUG_OFFLINE_ENV).is_ok_and(|value| debug_env_value_enabled(&value)) {
        return builder;
    }

    tracing::warn!(
        env = DEBUG_OFFLINE_ENV,
        "debug offline HTTP simulation enabled"
    );
    match reqwest::Proxy::all(DEBUG_OFFLINE_PROXY) {
        Ok(proxy) => builder.proxy(proxy),
        Err(error) => {
            tracing::warn!(error = %error, "failed to configure debug offline proxy");
            builder
        }
    }
}

#[cfg(any(debug_assertions, test))]
pub(crate) fn debug_env_value_enabled(value: &str) -> bool {
    let value = value.trim();
    !(value == "0"
        || value.eq_ignore_ascii_case("false")
        || value.eq_ignore_ascii_case("no")
        || value.eq_ignore_ascii_case("off"))
}

pub(crate) fn retry_backoff_secs(consecutive: u32) -> u64 {
    const BASE: u64 = 300;
    const CAP: u64 = 3600;
    let shift = consecutive.saturating_sub(1).min(12);
    BASE.saturating_mul(1u64 << shift).min(CAP)
}

pub(crate) const REFRESH_STALE_AFTER_SECS: i64 = 60;

const STALE_REFRESH_MESSAGE: &str = "Refresh timed out";

pub fn resolve_stale_refreshes(state: &mut AppState) -> bool {
    let now = Utc::now();
    let stale = chrono::Duration::seconds(REFRESH_STALE_AFTER_SECS);
    let mut stale_providers = Vec::new();
    for provider in &mut state.providers {
        if !provider.is_refreshing {
            continue;
        }
        if provider
            .refresh_started_at
            .is_some_and(|started| now - started < stale)
        {
            continue;
        }
        provider.is_refreshing = false;
        provider.refresh_started_at = None;
        provider.error = Some(STALE_REFRESH_MESSAGE.to_string());
        tracing::warn!(
            provider = provider.provider.label(),
            "resolved stale provider refresh as timed out"
        );
        stale_providers.push((provider.provider, provider.selected_account_ids.clone()));
    }
    for (provider, account_ids) in &stale_providers {
        for account in state.provider_accounts.iter_mut().filter(|account| {
            account.provider == *provider && account_ids.contains(&account.account_id)
        }) {
            account.health = ProviderHealth::Error;
            account.auth_state = AuthState::Error;
            account.error = Some(STALE_REFRESH_MESSAGE.to_string());
            account.consecutive_failures = account.consecutive_failures.saturating_add(1);
            let backoff = retry_backoff_secs(account.consecutive_failures);
            account.retry_after = Some(now + chrono::Duration::seconds(backoff.cast_signed()));
        }
    }
    !stale_providers.is_empty()
}

pub(crate) fn classify_auth_state(error: &AppError) -> AuthState {
    if error.requires_user_action() {
        AuthState::ActionRequired
    } else {
        AuthState::Error
    }
}

pub async fn refresh_provider_account_statuses(
    provider: ProviderId,
    config: Config,
    previous_accounts: Vec<ProviderAccountRuntimeState>,
) -> Vec<ProviderAccountRuntimeState> {
    providers::registry::refresh_account_statuses(provider, config, previous_accounts).await
}

pub async fn refresh_account(
    config: Config,
    provider: ProviderId,
    enabled: bool,
    account_id: String,
    previous: Option<ProviderRuntimeState>,
    previous_accounts: Vec<ProviderAccountRuntimeState>,
    process: Option<RefreshProcessContext>,
) -> ProviderRefreshResult {
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
        account_id = %account_id,
        "provider account refresh started"
    );
    let client = http_client();
    let accounts = providers::registry::discover_accounts(provider, &config);

    let Some(account) = accounts
        .iter()
        .find(|a| a.account_id == account_id)
        .or_else(|| accounts.first())
    else {
        return no_provider_accounts(provider, enabled, previous.as_ref());
    };

    let account = account.clone();
    let account_id = account.account_id.clone();
    let label = account.label.clone();
    let prev = previous_accounts
        .iter()
        .find(|a| a.account_id == account_id)
        .cloned();
    refresh_provider_account(
        provider,
        enabled,
        previous.as_ref(),
        prev.as_ref(),
        account_id,
        label,
        async move {
            providers::registry::fetch_account(&account, &client)
                .await
                .map(|s| (s.source.clone(), s))
        },
    )
    .await
}

fn no_provider_accounts(
    provider: ProviderId,
    enabled: bool,
    previous: Option<&ProviderRuntimeState>,
) -> ProviderRefreshResult {
    ProviderRefreshResult {
        provider: not_ready_provider(provider, enabled, previous),
        accounts: Vec::new(),
    }
}

fn not_ready_provider(
    provider: ProviderId,
    enabled: bool,
    previous: Option<&ProviderRuntimeState>,
) -> ProviderRuntimeState {
    if !enabled {
        return ProviderRuntimeState::disabled(provider);
    }
    let mut state = previous
        .cloned()
        .unwrap_or_else(|| ProviderRuntimeState::empty(provider));
    state.provider = provider;
    state.enabled = true;
    state.is_refreshing = false;
    state.selected_account_ids = Vec::new();
    state.account_status = AccountSelectionStatus::LoginRequired;
    state.error = Some("Login required".to_string());
    state
}

pub(crate) async fn refresh_provider_account<F>(
    provider: ProviderId,
    enabled: bool,
    previous: Option<&ProviderRuntimeState>,
    previous_account: Option<&ProviderAccountRuntimeState>,
    account_id: String,
    label: String,
    fetch: F,
) -> ProviderRefreshResult
where
    F: std::future::Future<Output = crate::error::Result<(String, UsageSnapshot), AppError>>,
{
    if !enabled {
        tracing::info!(
            provider = provider.label(),
            account_id = %account_id,
            "provider refresh skipped because provider is disabled"
        );
        return ProviderRefreshResult {
            provider: ProviderRuntimeState::disabled(provider),
            accounts: Vec::new(),
        };
    }

    let mut state = previous
        .cloned()
        .unwrap_or_else(|| ProviderRuntimeState::empty(provider));
    state.provider = provider;
    state.enabled = true;
    state.is_refreshing = true;
    if !state.selected_account_ids.contains(&account_id) {
        state.selected_account_ids.push(account_id.clone());
    }
    state.account_status = AccountSelectionStatus::Ready;
    state.error = None;

    let mut account = previous_account
        .cloned()
        .unwrap_or_else(|| ProviderAccountRuntimeState::empty(provider, account_id, label));

    match fetch.await {
        Ok((source_label, snapshot)) => {
            tracing::info!(
                provider = provider.label(),
                account_id = %account.account_id,
                source = source_label.as_str(),
                "provider refresh succeeded"
            );
            state.is_refreshing = false;
            state.refresh_started_at = None;
            state.error = None;
            account.health = ProviderHealth::Ok;
            account.auth_state = AuthState::Ready;
            account.source_label = Some(source_label);
            account.last_success_at = Some(Utc::now());
            account.snapshot = Some(snapshot);
            account.error = None;
            account.retry_after = None;
            account.consecutive_failures = 0;
            if provider == ProviderId::Claude
                && let Some(email) = account
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.identity.email.as_deref())
                    .filter(|e| !e.is_empty())
            {
                account.label = email.to_string();
            }
        }
        Err(error) => {
            if error.is_transient() {
                tracing::warn!(
                    provider = provider.label(),
                    account_id = %account.account_id,
                    error = %error,
                    "provider refresh skipped after transient error"
                );
            } else {
                tracing::error!(
                    provider = provider.label(),
                    account_id = %account.account_id,
                    error = %error,
                    "provider refresh failed"
                );
            }
            let user_message = error.user_message();
            state.is_refreshing = false;
            state.refresh_started_at = None;
            if providers::registry::auth_error_requires_reauth_prompt(provider)
                && error.requires_user_action()
            {
                state.account_status = AccountSelectionStatus::LoginRequired;
                state.error = Some("Login required".to_string());
            } else {
                state.error = Some(user_message.clone());
            }
            account.health = ProviderHealth::Error;
            account.auth_state = classify_auth_state(&error);
            account.error = Some(user_message);
            account.consecutive_failures = account.consecutive_failures.saturating_add(1);
            let backoff_secs = error
                .rate_limit_retry_after_secs()
                .unwrap_or_else(|| retry_backoff_secs(account.consecutive_failures));
            account.retry_after =
                Some(Utc::now() + chrono::Duration::seconds(backoff_secs.cast_signed()));
        }
    }

    ProviderRefreshResult {
        provider: state,
        accounts: vec![account],
    }
}

pub fn reconcile_state(
    config: &Config,
    detection: &crate::detection::DetectionSnapshot,
    state: &mut AppState,
) {
    reconcile_state_with_refresh(config, detection, state, false);
}

pub fn reconcile_shared_state(
    config: &Config,
    detection: &crate::detection::DetectionSnapshot,
    state: &mut AppState,
) {
    reconcile_state_with_refresh(config, detection, state, true);
}

fn reconcile_state_with_refresh(
    config: &Config,
    detection: &crate::detection::DetectionSnapshot,
    state: &mut AppState,
    preserve_refreshing: bool,
) {
    ensure_provider_states(state);
    for provider in ProviderId::ALL {
        providers::registry::reconcile_provider_accounts(provider, config, state);
    }
    for provider in &mut state.providers {
        provider.enabled =
            crate::provider_enablement::provider_enabled(config, detection, provider.provider);
        if !preserve_refreshing {
            provider.is_refreshing = false;
            provider.refresh_started_at = None;
        }
        if !provider.enabled {
            provider.account_status = AccountSelectionStatus::Unavailable;
            provider.selected_account_ids = Vec::new();
            provider.is_refreshing = false;
            provider.refresh_started_at = None;
        }
    }
}

pub fn reconcile_provider(
    config: &Config,
    detection: &crate::detection::DetectionSnapshot,
    state: &mut AppState,
    provider: ProviderId,
) {
    ensure_provider_states(state);
    providers::registry::reconcile_provider_accounts(provider, config, state);
    if let Some(entry) = state.provider_mut(provider) {
        entry.enabled = crate::provider_enablement::provider_enabled(config, detection, provider);
        entry.is_refreshing = false;
        entry.refresh_started_at = None;
        if !entry.enabled {
            entry.account_status = AccountSelectionStatus::Unavailable;
            entry.selected_account_ids = Vec::new();
        }
    }
}

pub(crate) fn mark_account_reauthenticated(
    state: &mut AppState,
    provider: ProviderId,
    account_id: &str,
) {
    if let Some(account) = state
        .provider_accounts
        .iter_mut()
        .find(|a| a.provider == provider && a.account_id == account_id)
    {
        account.health = ProviderHealth::Ok;
        account.auth_state = AuthState::Ready;
        account.error = None;
        account.retry_after = None;
        account.consecutive_failures = 0;
    }
}

fn ensure_provider_states(state: &mut AppState) {
    let mut ordered = Vec::with_capacity(ProviderId::ALL.len());
    for provider in ProviderId::ALL {
        let existing = state
            .providers
            .iter()
            .position(|entry| entry.provider == provider)
            .map(|index| state.providers.swap_remove(index));
        ordered.push(existing.unwrap_or_else(|| ProviderRuntimeState::empty(provider)));
    }
    state.providers = ordered;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_storage::{
        NewProviderAccount, ProviderAccountStorage, ProviderAccountTokens,
    };
    use crate::config::{ManagedCodexAccountConfig, paths};
    use crate::error::{ClaudeError, CodexError, OpenCodeGoError};
    use crate::model::{ProviderIdentity, UsageHeadline};
    use std::fs;

    fn snapshot() -> UsageSnapshot {
        UsageSnapshot {
            provider: ProviderId::Codex,
            source: "OAuth".to_string(),
            updated_at: Utc::now(),
            headline: UsageHeadline(0),
            windows: Vec::new(),
            provider_cost: None,
            extra_usage: None,
            identity: ProviderIdentity::default(),
        }
    }

    #[test]
    fn mark_account_reauthenticated_clears_failure_state_so_refresh_can_run() {
        let mut state = AppState::empty();
        let mut account = ProviderAccountRuntimeState::empty(
            ProviderId::Antigravity,
            "antigravity-1",
            "user@example.com",
        );
        account.snapshot = Some(snapshot());
        account.health = ProviderHealth::Error;
        account.auth_state = AuthState::ActionRequired;
        account.error = Some("antigravity token refresh returned HTTP 400".to_string());
        account.consecutive_failures = 2;
        account.retry_after = Some(Utc::now() + chrono::Duration::seconds(600));
        state.upsert_account(account);

        mark_account_reauthenticated(&mut state, ProviderId::Antigravity, "antigravity-1");

        let account = state
            .provider_accounts
            .iter()
            .find(|a| a.account_id == "antigravity-1")
            .unwrap();
        assert_eq!(account.auth_state, AuthState::Ready);
        assert_eq!(account.health, ProviderHealth::Ok);
        assert!(account.error.is_none());
        assert!(account.retry_after.is_none());
        assert_eq!(account.consecutive_failures, 0);
        assert!(!account.is_backing_off());
        assert!(account.snapshot.is_some());
    }

    #[test]
    fn classifies_unauthorized_as_action_required() {
        let state = classify_auth_state(&CodexError::Unauthorized.into());
        assert_eq!(state, AuthState::ActionRequired);
    }

    #[test]
    fn classifies_no_usage_data_as_error() {
        let state = classify_auth_state(&CodexError::NoUsageData.into());
        assert_eq!(state, AuthState::Error);
    }

    #[test]
    fn http_timeouts_are_correct() {
        assert_eq!(HTTP_CONNECT_TIMEOUT, Duration::from_secs(5));
        assert_eq!(HTTP_TIMEOUT, Duration::from_secs(20));
    }

    #[test]
    fn http_client_builds_without_panic() {
        let _client = http_client();
    }

    #[cfg(debug_assertions)]
    #[test]
    fn debug_env_value_enabled_accepts_common_false_values() {
        assert!(debug_env_value_enabled(""));
        assert!(debug_env_value_enabled("1"));
        assert!(debug_env_value_enabled("true"));
        assert!(!debug_env_value_enabled("0"));
        assert!(!debug_env_value_enabled("false"));
        assert!(!debug_env_value_enabled("NO"));
        assert!(!debug_env_value_enabled(" off "));
    }

    #[test]
    fn load_initial_state_returns_empty_when_shared_runtime_is_missing() {
        let config = Config::default();
        let state = load_initial_state(
            &config,
            &crate::detection::DetectionSnapshot::default(),
            None,
        );
        assert_eq!(state.providers.len(), ProviderId::ALL.len());
        for provider in &state.providers {
            assert!(!provider.is_refreshing);
        }
    }

    #[test]
    fn load_initial_state_includes_grok_in_canonical_order() {
        let config = Config::default();
        let state = load_initial_state(
            &config,
            &crate::detection::DetectionSnapshot::default(),
            None,
        );
        assert!(state.provider(ProviderId::Grok).is_some());
        let last_provider = state.providers.last().map(|p| p.provider);
        assert_eq!(last_provider, Some(ProviderId::Grok));
    }

    #[test]
    fn load_initial_state_reorders_stale_shared_runtime_into_canonical_order() {
        let config = Config::default();
        let mut shared_state = AppState::empty();
        shared_state.providers.rotate_left(1);
        shared_state
            .provider_mut(ProviderId::Antigravity)
            .unwrap()
            .is_refreshing = true;

        let state = load_initial_state(
            &config,
            &crate::detection::DetectionSnapshot::default(),
            Some(SharedRuntimeState::new(shared_state, 1)),
        );

        let order: Vec<ProviderId> = state.providers.iter().map(|entry| entry.provider).collect();
        assert_eq!(order, ProviderId::ALL.to_vec());
        assert!(state.provider(ProviderId::Antigravity).is_some());
    }

    #[test]
    fn load_initial_state_uses_shared_runtime_when_present() {
        let config = Config::default();
        let mut shared_state = AppState::empty();
        shared_state.upsert_provider(ProviderRuntimeState::disabled(ProviderId::Cursor));
        shared_state
            .provider_mut(ProviderId::Codex)
            .unwrap()
            .is_refreshing = true;
        let state = load_initial_state(
            &config,
            &crate::detection::DetectionSnapshot::default(),
            Some(SharedRuntimeState::new(shared_state, 1)),
        );

        assert_eq!(state.providers.len(), ProviderId::ALL.len());
        assert!(state.provider(ProviderId::Cursor).is_some());
        assert!(!state.provider(ProviderId::Codex).unwrap().is_refreshing);
    }

    #[test]
    fn reconcile_shared_state_preserves_refreshing_provider() {
        let config = Config {
            codex_enablement: crate::config::ProviderEnablement::Enabled,
            ..Config::default()
        };
        let mut state = AppState::empty();
        state.provider_mut(ProviderId::Codex).unwrap().is_refreshing = true;

        reconcile_shared_state(
            &config,
            &crate::detection::DetectionSnapshot::default(),
            &mut state,
        );

        assert!(state.provider(ProviderId::Codex).unwrap().is_refreshing);
    }

    #[test]
    fn reconcile_state_keeps_missing_codex_credentials_actionable() {
        let _guard = crate::test_support::env_lock();
        let state_root = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_STATE_HOME", state_root.path());
        }

        let storage = ProviderAccountStorage::new(paths().codex_accounts_dir);
        let stored = storage
            .create_account(NewProviderAccount {
                provider: ProviderId::Codex,
                email: "user@example.com".to_string(),
                provider_account_id: Some("acct-123".to_string()),
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
            })
            .unwrap();
        fs::remove_file(stored.account_dir.join("tokens.json")).unwrap();
        let now = Utc::now();
        let account_id = stored.account_ref.account_id.clone();
        let config = Config {
            selected_codex_account_ids: vec![account_id.clone()],
            codex_managed_accounts: vec![ManagedCodexAccountConfig {
                id: account_id.clone(),
                label: "user@example.com".to_string(),
                codex_home: stored.account_dir,
                email: Some("user@example.com".to_string()),
                provider_account_id: Some("acct-123".to_string()),
                created_at: now,
                updated_at: now,
                last_authenticated_at: Some(now),
            }],
            ..Config::default()
        };
        let mut state = AppState::empty();

        reconcile_state(
            &config,
            &crate::detection::DetectionSnapshot::default(),
            &mut state,
        );

        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }

        let account = state
            .provider_accounts
            .iter()
            .find(|account| account.account_id == account_id)
            .unwrap();
        assert_eq!(account.auth_state, AuthState::ActionRequired);
        assert!(
            account
                .error
                .as_deref()
                .is_some_and(|error| { error.contains("restore from OpenCode") })
        );
    }

    fn state_with_refreshing_provider(started_at: Option<chrono::DateTime<Utc>>) -> AppState {
        let mut state = AppState::empty();
        {
            let provider = state.provider_mut(ProviderId::Codex).unwrap();
            provider.is_refreshing = true;
            provider.refresh_started_at = started_at;
            provider.selected_account_ids = vec!["codex-1".to_string()];
        }
        state.upsert_account(ProviderAccountRuntimeState::empty(
            ProviderId::Codex,
            "codex-1",
            "Codex",
        ));
        state
    }

    #[test]
    fn resolve_stale_refreshes_resolves_orphaned_refresh() {
        let mut state = state_with_refreshing_provider(None);

        assert!(resolve_stale_refreshes(&mut state));

        let provider = state.provider(ProviderId::Codex).unwrap();
        assert!(!provider.is_refreshing);
        assert!(provider.refresh_started_at.is_none());
        let account = state
            .provider_accounts
            .iter()
            .find(|account| account.account_id == "codex-1")
            .unwrap();
        assert_eq!(account.consecutive_failures, 1);
        assert!(account.is_backing_off());
    }

    #[test]
    fn resolve_stale_refreshes_ignores_fresh_refresh() {
        let mut state = state_with_refreshing_provider(Some(Utc::now()));

        assert!(!resolve_stale_refreshes(&mut state));
        assert!(state.provider(ProviderId::Codex).unwrap().is_refreshing);
    }

    #[tokio::test]
    async fn refresh_provider_success_sets_ok_state() {
        let snap = snapshot();
        let result = refresh_provider_account(
            ProviderId::Codex,
            true,
            None,
            None,
            "codex-1".to_string(),
            "Codex".to_string(),
            async { Ok(("OAuth".to_string(), snap)) },
        )
        .await;
        let account = result.accounts.first().unwrap();

        assert_eq!(account.health, ProviderHealth::Ok);
        assert_eq!(account.auth_state, AuthState::Ready);
        assert!(account.snapshot.is_some());
        assert!(account.last_success_at.is_some());
        assert!(!result.provider.is_refreshing);
        assert!(result.provider.error.is_none());
    }

    #[tokio::test]
    async fn refresh_provider_error_keeps_previous_snapshot() {
        let previous = ProviderRuntimeState::empty(ProviderId::Codex);
        let mut previous_account =
            ProviderAccountRuntimeState::empty(ProviderId::Codex, "codex-1", "Codex");
        previous_account.snapshot = Some(snapshot());
        previous_account.last_success_at = Some(Utc::now());
        previous_account.source_label = Some("OAuth".to_string());

        let result = refresh_provider_account(
            ProviderId::Codex,
            true,
            Some(&previous),
            Some(&previous_account),
            "codex-1".to_string(),
            "Codex".to_string(),
            async { Err(AppError::from(CodexError::NoUsageData)) },
        )
        .await;
        let account = result.accounts.first().unwrap();

        assert!(account.snapshot.is_some());
        assert!(account.last_success_at.is_some());
        assert_eq!(account.source_label.as_deref(), Some("OAuth"));
        assert_eq!(account.auth_state, AuthState::Error);
        assert!(!result.provider.is_refreshing);
    }

    #[tokio::test]
    async fn claude_refresh_auth_failure_requires_action_and_keeps_previous_snapshot() {
        let previous = ProviderRuntimeState::empty(ProviderId::Claude);
        let mut previous_account =
            ProviderAccountRuntimeState::empty(ProviderId::Claude, "claude-1", "Claude");
        previous_account.snapshot = Some(snapshot());
        previous_account.last_success_at = Some(Utc::now());

        let result = refresh_provider_account(
            ProviderId::Claude,
            true,
            Some(&previous),
            Some(&previous_account),
            "claude-1".to_string(),
            "Claude".to_string(),
            async {
                Err(AppError::from(ClaudeError::TokenRefreshHttp {
                    status: 400,
                }))
            },
        )
        .await;
        let account = result.accounts.first().unwrap();

        assert_eq!(account.health, ProviderHealth::Error);
        assert_eq!(account.auth_state, AuthState::ActionRequired);
        assert!(account.snapshot.is_some());
        assert!(account.last_success_at.is_some());
    }

    #[tokio::test]
    async fn cursor_permanent_refresh_failure_requires_action_and_keeps_previous_snapshot() {
        let previous = ProviderRuntimeState::empty(ProviderId::Cursor);
        let mut previous_account =
            ProviderAccountRuntimeState::empty(ProviderId::Cursor, "cursor-1", "user@example.com");
        previous_account.snapshot = Some(snapshot());
        previous_account.last_success_at = Some(Utc::now());

        let result = refresh_provider_account(
            ProviderId::Cursor,
            true,
            Some(&previous),
            Some(&previous_account),
            "cursor-1".to_string(),
            "user@example.com".to_string(),
            async { Err(AppError::from(crate::error::CursorError::Unauthorized)) },
        )
        .await;
        let account = result.accounts.first().unwrap();

        assert_eq!(account.health, ProviderHealth::Error);
        assert_eq!(account.auth_state, AuthState::ActionRequired);
        assert_eq!(
            result.provider.account_status,
            AccountSelectionStatus::LoginRequired
        );
        assert!(account.snapshot.is_some());
        assert!(account.last_success_at.is_some());
    }

    #[tokio::test]
    async fn copilot_auth_failure_requires_action_and_keeps_previous_snapshot() {
        let previous = ProviderRuntimeState::empty(ProviderId::Copilot);
        let mut previous_account =
            ProviderAccountRuntimeState::empty(ProviderId::Copilot, "copilot-1", "octocat");
        previous_account.snapshot = Some(snapshot());
        previous_account.last_success_at = Some(Utc::now());

        let result = refresh_provider_account(
            ProviderId::Copilot,
            true,
            Some(&previous),
            Some(&previous_account),
            "copilot-1".to_string(),
            "octocat".to_string(),
            async { Err(AppError::from(crate::error::CopilotError::LoginRequired)) },
        )
        .await;
        let account = result.accounts.first().unwrap();

        assert_eq!(account.health, ProviderHealth::Error);
        assert_eq!(account.auth_state, AuthState::ActionRequired);
        assert!(account.snapshot.is_some());
        assert!(account.last_success_at.is_some());
    }

    #[tokio::test]
    async fn refresh_provider_transient_error_sets_error_state() {
        let result = refresh_provider_account(
            ProviderId::Claude,
            true,
            None,
            None,
            "default".to_string(),
            "Default".to_string(),
            async {
                Err(AppError::from(ClaudeError::RateLimited {
                    retry_after_secs: None,
                }))
            },
        )
        .await;
        let account = result.accounts.first().unwrap();

        assert_eq!(account.health, ProviderHealth::Error);
        assert!(!result.provider.is_refreshing);
    }

    #[tokio::test]
    async fn opencode_go_entitlement_failure_is_retryable_without_reauthentication() {
        let result = refresh_provider_account(
            ProviderId::OpenCodeGo,
            true,
            None,
            None,
            "opencode-go-1".to_string(),
            "OpenCode Go".to_string(),
            async { Err(AppError::from(OpenCodeGoError::EntitlementRequired)) },
        )
        .await;
        let account = result.accounts.first().unwrap();

        assert_eq!(account.auth_state, AuthState::Error);
        assert_eq!(
            account.error.as_deref(),
            Some("OpenCode Go subscription required")
        );
        assert_eq!(
            result.provider.account_status,
            AccountSelectionStatus::Ready
        );
        assert_ne!(result.provider.error.as_deref(), Some("Login required"));
    }

    #[tokio::test]
    async fn copilot_rate_limit_records_backoff_and_preserves_snapshot() {
        let previous = ProviderRuntimeState::empty(ProviderId::Copilot);
        let mut previous_account =
            ProviderAccountRuntimeState::empty(ProviderId::Copilot, "copilot-1", "octocat");
        previous_account.snapshot = Some(snapshot());
        previous_account.last_success_at = Some(Utc::now());

        let result = refresh_provider_account(
            ProviderId::Copilot,
            true,
            Some(&previous),
            Some(&previous_account),
            "copilot-1".to_string(),
            "octocat".to_string(),
            async {
                Err(AppError::from(crate::error::CopilotError::RateLimited {
                    retry_after_secs: Some(42),
                }))
            },
        )
        .await;
        let account = result.accounts.first().unwrap();

        assert_eq!(account.health, ProviderHealth::Error);
        assert_eq!(account.auth_state, AuthState::Error);
        assert_eq!(account.consecutive_failures, 1);
        assert!(account.retry_after.is_some());
        assert!(account.snapshot.is_some());
    }

    #[tokio::test]
    async fn generic_failure_records_backoff_and_preserves_snapshot() {
        let previous = ProviderRuntimeState::empty(ProviderId::Copilot);
        let mut previous_account =
            ProviderAccountRuntimeState::empty(ProviderId::Copilot, "copilot-1", "octocat");
        previous_account.snapshot = Some(snapshot());
        previous_account.last_success_at = Some(Utc::now());

        let result = refresh_provider_account(
            ProviderId::Copilot,
            true,
            Some(&previous),
            Some(&previous_account),
            "copilot-1".to_string(),
            "octocat".to_string(),
            async {
                Err(AppError::from(
                    crate::error::CopilotError::UnrecognizedResponse {
                        detail: "access_type_sku=free_limited_copilot, login=octocat, token_based_billing=absent".to_string(),
                    },
                ))
            },
        )
        .await;
        let account = result.accounts.first().unwrap();

        assert_eq!(account.health, ProviderHealth::Error);
        assert_eq!(account.consecutive_failures, 1);
        assert!(account.is_backing_off());
        assert!(account.snapshot.is_some());
    }

    #[tokio::test]
    async fn refresh_provider_disabled_returns_disabled_state() {
        let result = refresh_provider_account(
            ProviderId::Codex,
            false,
            None,
            None,
            "codex-1".to_string(),
            "Codex".to_string(),
            async { Ok(("x".to_string(), snapshot())) },
        )
        .await;

        assert!(!result.provider.enabled);
    }

    #[tokio::test]
    async fn cursor_unauthorized_sets_login_required_state() {
        let result = refresh_provider_account(
            ProviderId::Cursor,
            true,
            None,
            None,
            "cursor-managed:user@example.com".to_string(),
            "user@example.com".to_string(),
            async { Err(AppError::from(crate::error::CursorError::Unauthorized)) },
        )
        .await;

        let account = result.accounts.first().unwrap();
        assert_eq!(
            result.provider.account_status,
            AccountSelectionStatus::LoginRequired
        );
        assert_eq!(
            result.provider.selected_account_ids.as_slice(),
            ["cursor-managed:user@example.com"]
        );
        assert_eq!(result.provider.error.as_deref(), Some("Login required"));
        assert_eq!(account.auth_state, AuthState::ActionRequired);
        assert_eq!(account.health, ProviderHealth::Error);
    }
}
