// SPDX-License-Identifier: MPL-2.0

use crate::model::{AppState, ProviderId, ProviderRuntimeState};
use chrono::Utc;

impl AppState {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            providers: ProviderId::ALL
                .into_iter()
                .map(ProviderRuntimeState::empty)
                .collect(),
            updated_at: Utc::now(),
        }
    }

    #[must_use]
    pub fn provider(&self, provider: ProviderId) -> Option<&ProviderRuntimeState> {
        self.providers
            .iter()
            .find(|entry| entry.provider == provider)
    }

    pub fn provider_mut(&mut self, provider: ProviderId) -> Option<&mut ProviderRuntimeState> {
        self.providers
            .iter_mut()
            .find(|entry| entry.provider == provider)
    }

    pub fn upsert_provider(&mut self, provider_state: ProviderRuntimeState) {
        if let Some(existing) = self
            .providers
            .iter_mut()
            .find(|entry| entry.provider == provider_state.provider)
        {
            *existing = provider_state;
        } else {
            self.providers.push(provider_state);
        }
        self.updated_at = Utc::now();
    }

    pub fn mark_provider_refreshing(&mut self, provider: ProviderId, enabled: bool) {
        let mut state = self
            .provider(provider)
            .cloned()
            .unwrap_or_else(|| ProviderRuntimeState::empty(provider));
        if enabled {
            state.provider = provider;
            state.enabled = true;
            state.is_refreshing = true;
        } else {
            state = ProviderRuntimeState::disabled(provider);
        }
        self.upsert_provider(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ProviderIdentity, UsageHeadline, UsageSnapshot};

    fn snapshot(provider: ProviderId) -> UsageSnapshot {
        UsageSnapshot {
            provider,
            source: "test".to_string(),
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
    fn upsert_provider_replaces_only_matching_provider() {
        let mut state = AppState::empty();
        let mut codex = ProviderRuntimeState::empty(ProviderId::Codex);
        codex.error = Some("codex done".to_string());

        state.upsert_provider(codex);

        assert_eq!(
            state
                .provider(ProviderId::Codex)
                .and_then(|provider| provider.error.as_deref()),
            Some("codex done")
        );
        assert_eq!(
            state
                .provider(ProviderId::Claude)
                .and_then(|provider| provider.error.as_deref()),
            Some("Not refreshed yet")
        );
    }

    #[test]
    fn mark_provider_refreshing_preserves_previous_snapshot() {
        let mut state = AppState::empty();
        let mut codex = ProviderRuntimeState::empty(ProviderId::Codex);
        codex.snapshot = Some(snapshot(ProviderId::Codex));
        state.upsert_provider(codex);

        state.mark_provider_refreshing(ProviderId::Codex, true);

        let codex = state.provider(ProviderId::Codex).unwrap();
        assert!(codex.is_refreshing);
        assert!(codex.snapshot.is_some());
    }

    #[test]
    fn mark_provider_refreshing_marks_disabled_provider() {
        let mut state = AppState::empty();

        state.mark_provider_refreshing(ProviderId::Cursor, false);

        let cursor = state.provider(ProviderId::Cursor).unwrap();
        assert!(!cursor.enabled);
        assert!(!cursor.is_refreshing);
        assert_eq!(cursor.error.as_deref(), Some("Disabled in config"));
    }
}
