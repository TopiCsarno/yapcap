// SPDX-License-Identifier: MPL-2.0

use crate::cache::{load_cached_state, save_cached_state};
use crate::config::Config;
use crate::demo_env;
use crate::error::AppError;
use crate::model::{
    AccountSelectionStatus, AppState, AuthState, ProviderAccountRuntimeState, ProviderHealth,
    ProviderId, ProviderRuntimeState, UsageSnapshot,
};
use crate::providers;
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

pub fn load_initial_state(config: &Config) -> AppState {
    let mut state = load_cached_state()
        .ok()
        .flatten()
        .unwrap_or_else(AppState::empty);
    reconcile_state(config, &mut state);
    state
}

pub fn persist_state(state: &AppState) {
    if demo_env::is_active() {
        return;
    }
    if let Err(error) = save_cached_state(state) {
        tracing::error!(error = %error, "failed to save snapshot cache");
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

pub(crate) fn rate_limit_backoff_secs(consecutive: u32) -> u64 {
    const BASE: u64 = 300;
    const CAP: u64 = 3600;
    let shift = consecutive.saturating_sub(1).min(12);
    BASE.saturating_mul(1u64 << shift).min(CAP)
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
    account_id: String,
    previous: Option<ProviderRuntimeState>,
    previous_accounts: Vec<ProviderAccountRuntimeState>,
) -> ProviderRefreshResult {
    let enabled = config.provider_enabled(provider);
    let client = http_client();
    let accounts = providers::registry::discover_accounts(provider, &config);

    let Some(account) = accounts
        .iter()
        .find(|a| a.account_id == account_id)
        .or_else(|| accounts.first())
    else {
        return no_provider_accounts(provider, enabled, previous.as_ref());
    };

    let account_id = account.account_id.clone();
    let label = account.label.clone();
    let prev = previous_accounts
        .iter()
        .find(|a| a.account_id == account_id)
        .cloned();
    let handle = account.handle.clone();

    refresh_provider_account(
        provider,
        enabled,
        previous.as_ref(),
        prev.as_ref(),
        account_id,
        label,
        async move {
            providers::registry::fetch_handle(&handle, &client)
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
                source = source_label.as_str(),
                "provider refresh succeeded"
            );
            state.is_refreshing = false;
            state.error = None;
            account.health = ProviderHealth::Ok;
            account.auth_state = AuthState::Ready;
            account.source_label = Some(source_label);
            account.last_success_at = Some(Utc::now());
            account.snapshot = Some(snapshot);
            account.error = None;
            account.rate_limit_until = None;
            account.consecutive_rate_limits = 0;
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
                tracing::warn!(provider = provider.label(), error = %error, "provider refresh skipped (transient)");
            } else {
                tracing::error!(provider = provider.label(), error = %error, "provider refresh failed");
            }
            let user_message = error.user_message();
            state.is_refreshing = false;
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
            if error.is_rate_limited() {
                account.consecutive_rate_limits = account.consecutive_rate_limits.saturating_add(1);
                let backoff_secs = error
                    .rate_limit_retry_after_secs()
                    .unwrap_or_else(|| rate_limit_backoff_secs(account.consecutive_rate_limits));
                account.rate_limit_until =
                    Some(Utc::now() + chrono::Duration::seconds(backoff_secs.cast_signed()));
            }
        }
    }

    ProviderRefreshResult {
        provider: state,
        accounts: vec![account],
    }
}

pub fn reconcile_state(config: &Config, state: &mut AppState) {
    ensure_provider_states(state);
    for provider in ProviderId::ALL {
        providers::registry::reconcile_provider_accounts(provider, config, state);
    }
    for provider in &mut state.providers {
        provider.enabled = config.provider_enabled(provider.provider);
        provider.is_refreshing = false;
        if !provider.enabled {
            provider.account_status = AccountSelectionStatus::Unavailable;
            provider.selected_account_ids = Vec::new();
        }
    }
}

pub fn reconcile_provider(config: &Config, state: &mut AppState, provider: ProviderId) {
    ensure_provider_states(state);
    providers::registry::reconcile_provider_accounts(provider, config, state);
    if let Some(entry) = state.provider_mut(provider) {
        entry.enabled = config.provider_enabled(provider);
        entry.is_refreshing = false;
        if !entry.enabled {
            entry.account_status = AccountSelectionStatus::Unavailable;
            entry.selected_account_ids = Vec::new();
        }
    }
}

fn ensure_provider_states(state: &mut AppState) {
    for provider in ProviderId::ALL {
        if state.provider(provider).is_none() {
            state.providers.push(ProviderRuntimeState::empty(provider));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{ClaudeError, CodexError};
    use crate::model::{ProviderIdentity, UsageHeadline};

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
    fn load_initial_state_returns_empty_when_no_cache() {
        let config = Config::default();
        let state = load_initial_state(&config);
        assert_eq!(state.providers.len(), ProviderId::ALL.len());
        for provider in &state.providers {
            assert!(!provider.is_refreshing);
        }
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
