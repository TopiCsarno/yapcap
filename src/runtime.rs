use crate::cache::{load_cached_state, save_cached_state};
use crate::config::AppConfig;
use crate::error::AppError;
use crate::model::{
    AppState, AuthState, ProviderHealth, ProviderId, ProviderRuntimeState, UsageSnapshot,
};
use crate::providers::{claude, codex, cursor};
use chrono::Utc;
use tracing::{error, info, warn};

pub async fn load_initial_state(config: &AppConfig) -> AppState {
    let mut state = load_cached_state()
        .ok()
        .flatten()
        .unwrap_or_else(AppState::empty);
    for provider in &mut state.providers {
        provider.enabled = config.provider_enabled(provider.provider);
    }
    state
}

pub async fn refresh_one(
    config: AppConfig,
    provider: ProviderId,
    previous: Option<ProviderRuntimeState>,
) -> ProviderRuntimeState {
    let client = reqwest::Client::new();
    match provider {
        ProviderId::Codex => {
            refresh_provider(
                provider,
                config.provider_enabled(provider),
                previous.as_ref(),
                async {
                    codex::fetch(&client)
                        .await
                        .map(|snapshot| (snapshot.source.clone(), snapshot))
                },
            )
            .await
        }
        ProviderId::Claude => {
            refresh_provider(
                provider,
                config.provider_enabled(provider),
                previous.as_ref(),
                async {
                    claude::fetch_with_browser(&client, config.claude_browser)
                        .await
                        .map(|snapshot| (snapshot.source.clone(), snapshot))
                },
            )
            .await
        }
        ProviderId::Cursor => {
            refresh_provider(
                provider,
                config.provider_enabled(provider),
                previous.as_ref(),
                async {
                    cursor::fetch(&client, config.cursor_browser)
                        .await
                        .map(|snapshot| (config.cursor_browser.label().to_string(), snapshot))
                },
            )
            .await
        }
    }
}

pub fn persist_state(state: &AppState) {
    if let Err(error_value) = save_cached_state(state) {
        error!(error = %error_value, "failed to save snapshot cache");
    }
}

async fn refresh_provider<F>(
    provider: ProviderId,
    enabled: bool,
    previous: Option<&ProviderRuntimeState>,
    fetch: F,
) -> ProviderRuntimeState
where
    F: std::future::Future<Output = crate::error::Result<(String, UsageSnapshot)>>,
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
            info!(
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
        Err(error_value) => {
            if error_value.is_transient() {
                warn!(provider = provider.label(), error = %error_value, "provider refresh skipped (transient)");
            } else {
                error!(provider = provider.label(), error = %error_value, "provider refresh failed");
            }
            state.is_refreshing = false;
            state.health = ProviderHealth::Error;
            state.auth_state = classify_auth_state(&error_value);
            state.error = Some(format!("{:#}", error_value));
        }
    }

    state
}

fn classify_auth_state(error: &AppError) -> AuthState {
    if error.requires_user_action() {
        AuthState::ActionRequired
    } else {
        AuthState::Error
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CodexError;
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

    #[tokio::test]
    async fn keeps_previous_snapshot_when_refresh_fails() {
        let mut previous = ProviderRuntimeState::empty(ProviderId::Codex);
        previous.snapshot = Some(snapshot());
        previous.last_success_at = Some(Utc::now());
        previous.source_label = Some("OAuth".to_string());
        previous.error = None;

        let state = refresh_provider(ProviderId::Codex, true, Some(&previous), async {
            Err(CodexError::NoUsageData.into())
        })
        .await;

        assert!(state.snapshot.is_some());
        assert!(state.last_success_at.is_some());
        assert_eq!(state.source_label.as_deref(), Some("OAuth"));
        assert_eq!(state.auth_state, AuthState::Error);
    }

    #[test]
    fn classifies_unauthorized_as_action_required() {
        let state = classify_auth_state(&CodexError::Unauthorized.into());
        assert_eq!(state, AuthState::ActionRequired);
    }
}
