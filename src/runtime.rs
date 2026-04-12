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

pub async fn refresh_all(config: &AppConfig) -> AppState {
    let previous = load_cached_state()
        .ok()
        .flatten()
        .unwrap_or_else(AppState::empty);
    let client = reqwest::Client::new();

    let codex_future = refresh_provider(
        ProviderId::Codex,
        config.provider_enabled(ProviderId::Codex),
        previous_provider(&previous, ProviderId::Codex),
        async {
            codex::fetch(&client)
                .await
                .map(|snapshot| ("OAuth".to_string(), snapshot))
        },
    );
    let claude_future = refresh_provider(
        ProviderId::Claude,
        config.provider_enabled(ProviderId::Claude),
        previous_provider(&previous, ProviderId::Claude),
        async {
            claude::fetch_with_browser(&client, config.cursor_browser)
                .await
                .map(|snapshot| (snapshot.source.clone(), snapshot))
        },
    );
    let cursor_future = refresh_provider(
        ProviderId::Cursor,
        config.provider_enabled(ProviderId::Cursor),
        previous_provider(&previous, ProviderId::Cursor),
        async {
            cursor::fetch(&client, config.cursor_browser)
                .await
                .map(|snapshot| (config.cursor_browser.label().to_string(), snapshot))
        },
    );
    let (codex_state, claude_state, cursor_state) =
        tokio::join!(codex_future, claude_future, cursor_future);

    let app_state = AppState {
        providers: vec![codex_state, claude_state, cursor_state],
        updated_at: Utc::now(),
    };

    if let Err(error_value) = save_cached_state(&app_state) {
        error!(error = %error_value, "failed to save snapshot cache");
    }

    app_state
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

fn previous_provider(state: &AppState, provider: ProviderId) -> Option<&ProviderRuntimeState> {
    state
        .providers
        .iter()
        .find(|entry| entry.provider == provider)
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
