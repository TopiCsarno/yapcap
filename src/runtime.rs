// SPDX-License-Identifier: MPL-2.0

use crate::cache::{load_cached_state, save_cached_state};
use crate::config::Config;
use crate::error::AppError;
use crate::providers;
use crate::model::{
    AppState, AuthState, ProviderHealth, ProviderId, ProviderRuntimeState, UsageSnapshot,
};
use chrono::Utc;
use std::time::Duration;

pub(crate) const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
pub(crate) const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub async fn load_initial_state(config: &Config) -> AppState {
    let mut state = load_cached_state()
        .ok()
        .flatten()
        .unwrap_or_else(AppState::empty);
    for provider in &mut state.providers {
        provider.enabled = config.provider_enabled(provider.provider);
        provider.is_refreshing = false;
    }
    state
}

pub fn persist_state(state: &AppState) {
    if let Err(error) = save_cached_state(state) {
        tracing::error!(error = %error, "failed to save snapshot cache");
    }
}

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .connect_timeout(HTTP_CONNECT_TIMEOUT)
        .build()
        .unwrap_or_else(|error| {
            tracing::warn!(error = %error, "failed to build timed HTTP client; using reqwest default");
            reqwest::Client::new()
        })
}

pub(crate) fn classify_auth_state(error: &AppError) -> AuthState {
    if error.requires_user_action() {
        AuthState::ActionRequired
    } else {
        AuthState::Error
    }
}

pub async fn refresh_one(
    config: Config,
    provider: ProviderId,
    previous: Option<ProviderRuntimeState>,
) -> ProviderRuntimeState {
    let enabled = config.provider_enabled(provider);
    let client = http_client();
    match provider {
        ProviderId::Codex => {
            refresh_provider(provider, enabled, previous.as_ref(), async {
                providers::codex::fetch(&client)
                    .await
                    .map(|snap| (snap.source.clone(), snap))
                    .map_err(AppError::from)
            })
            .await
        }
        ProviderId::Claude => {
            refresh_provider(provider, enabled, previous.as_ref(), async {
                providers::claude::fetch(&client)
                    .await
                    .map(|snap| (snap.source.clone(), snap))
                    .map_err(AppError::from)
            })
            .await
        }
        ProviderId::Cursor => {
            let browser = config.cursor_browser;
            let profile_id = config.cursor_profile_id.clone();
            refresh_provider(provider, enabled, previous.as_ref(), async move {
                providers::cursor::fetch(&client, browser, profile_id.as_deref())
                    .await
                    .map(|snap| (snap.source.clone(), snap))
                    .map_err(AppError::from)
            })
            .await
        }
    }
}

pub(crate) async fn refresh_provider<F>(
    provider: ProviderId,
    enabled: bool,
    previous: Option<&ProviderRuntimeState>,
    fetch: F,
) -> ProviderRuntimeState
where
    F: std::future::Future<Output = crate::error::Result<(String, UsageSnapshot), AppError>>,
{
    if !enabled {
        return ProviderRuntimeState::disabled(provider);
    }

    let mut state = previous
        .cloned()
        .unwrap_or_else(|| ProviderRuntimeState::empty(provider));
    state.provider = provider;
    state.enabled = true;
    state.is_refreshing = true;

    match fetch.await {
        Ok((source_label, snapshot)) => {
            tracing::info!(
                provider = provider.label(),
                source = source_label.as_str(),
                "provider refresh succeeded"
            );
            state.is_refreshing = false;
            state.health = ProviderHealth::Ok;
            state.auth_state = AuthState::Ready;
            state.source_label = Some(source_label);
            state.last_success_at = Some(Utc::now());
            state.snapshot = Some(snapshot);
            state.error = None;
        }
        Err(error) => {
            if error.is_transient() {
                tracing::warn!(provider = provider.label(), error = %error, "provider refresh skipped (transient)");
            } else {
                tracing::error!(provider = provider.label(), error = %error, "provider refresh failed");
            }
            state.is_refreshing = false;
            state.health = ProviderHealth::Error;
            state.auth_state = classify_auth_state(&error);
            state.error = Some(format!("{error:#}"));
        }
    }

    state
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
            headline: UsageHeadline::Primary,
            primary: None,
            secondary: None,
            tertiary: None,
            provider_cost: None,
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

    #[tokio::test]
    async fn load_initial_state_returns_empty_when_no_cache() {
        let config = Config::default();
        let state = load_initial_state(&config).await;
        assert_eq!(state.providers.len(), ProviderId::ALL.len());
        for provider in &state.providers {
            assert!(!provider.is_refreshing);
        }
    }

    #[tokio::test]
    async fn refresh_provider_success_sets_ok_state() {
        let snap = snapshot();
        let state = refresh_provider(ProviderId::Codex, true, None, async {
            Ok(("OAuth".to_string(), snap))
        })
        .await;

        assert_eq!(state.health, ProviderHealth::Ok);
        assert_eq!(state.auth_state, AuthState::Ready);
        assert!(state.snapshot.is_some());
        assert!(state.last_success_at.is_some());
        assert!(!state.is_refreshing);
        assert!(state.error.is_none());
    }

    #[tokio::test]
    async fn refresh_provider_error_keeps_previous_snapshot() {
        let mut previous = ProviderRuntimeState::empty(ProviderId::Codex);
        previous.snapshot = Some(snapshot());
        previous.last_success_at = Some(Utc::now());
        previous.source_label = Some("OAuth".to_string());

        let state = refresh_provider(ProviderId::Codex, true, Some(&previous), async {
            Err(AppError::from(CodexError::NoUsageData))
        })
        .await;

        assert!(state.snapshot.is_some());
        assert!(state.last_success_at.is_some());
        assert_eq!(state.source_label.as_deref(), Some("OAuth"));
        assert_eq!(state.auth_state, AuthState::Error);
        assert!(!state.is_refreshing);
    }

    #[tokio::test]
    async fn refresh_provider_transient_error_sets_error_state() {
        let state = refresh_provider(ProviderId::Claude, true, None, async {
            Err(AppError::from(ClaudeError::RateLimited))
        })
        .await;

        assert_eq!(state.health, ProviderHealth::Error);
        assert!(!state.is_refreshing);
    }

    #[tokio::test]
    async fn refresh_provider_disabled_returns_disabled_state() {
        let state = refresh_provider(ProviderId::Codex, false, None, async {
            Ok(("x".to_string(), snapshot()))
        })
        .await;

        assert!(!state.enabled);
    }
}
