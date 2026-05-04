// SPDX-License-Identifier: MPL-2.0

use crate::account_selection::MAX_MULTI_ACCOUNT_SELECTION;
use crate::model::{
    AccountSelectionStatus, AppState, ProviderAccountRuntimeState, ProviderId, ProviderRuntimeState,
};
use chrono::Utc;

impl AppState {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            providers: ProviderId::ALL
                .into_iter()
                .map(ProviderRuntimeState::empty)
                .collect(),
            provider_accounts: Vec::new(),
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

    #[must_use]
    pub fn active_account(&self, provider: ProviderId) -> Option<&ProviderAccountRuntimeState> {
        let first_id = self.provider(provider)?.selected_account_ids.first()?;
        self.provider_accounts
            .iter()
            .find(|entry| entry.provider == provider && &entry.account_id == first_id)
    }

    #[must_use]
    pub fn selected_accounts(&self, provider: ProviderId) -> Vec<&ProviderAccountRuntimeState> {
        let selected_ids = self
            .provider(provider)
            .map(|p| p.selected_account_ids.as_slice())
            .unwrap_or_default();
        selected_ids
            .iter()
            .filter_map(|id| {
                self.provider_accounts
                    .iter()
                    .find(|a| a.provider == provider && &a.account_id == id)
            })
            .collect()
    }

    #[must_use]
    pub fn display_selected_accounts(
        &self,
        provider: ProviderId,
    ) -> Vec<&ProviderAccountRuntimeState> {
        self.selected_accounts(provider)
            .into_iter()
            .take(MAX_MULTI_ACCOUNT_SELECTION)
            .collect()
    }

    #[must_use]
    pub fn display_selected_account_count(&self, provider: ProviderId) -> usize {
        self.display_selected_accounts(provider).len().max(1)
    }

    #[must_use]
    pub fn accounts_for(&self, provider: ProviderId) -> Vec<&ProviderAccountRuntimeState> {
        self.provider_accounts
            .iter()
            .filter(|entry| entry.provider == provider)
            .collect()
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
            state.is_refreshing = state.account_status == AccountSelectionStatus::Ready;
        } else {
            state = ProviderRuntimeState::disabled(provider);
        }
        self.upsert_provider(state);
    }

    pub fn upsert_account(&mut self, account_state: ProviderAccountRuntimeState) {
        if let Some(existing) = self.provider_accounts.iter_mut().find(|entry| {
            entry.provider == account_state.provider && entry.account_id == account_state.account_id
        }) {
            *existing = account_state;
        } else {
            self.provider_accounts.push(account_state);
        }
        self.updated_at = Utc::now();
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
            headline: UsageHeadline(0),
            windows: Vec::new(),
            provider_cost: None,
            extra_usage: None,
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
        codex.legacy_display_snapshot = Some(snapshot(ProviderId::Codex));
        codex.account_status = AccountSelectionStatus::Ready;
        state.upsert_provider(codex);

        state.mark_provider_refreshing(ProviderId::Codex, true);

        let codex = state.provider(ProviderId::Codex).unwrap();
        assert!(codex.is_refreshing);
        assert!(codex.legacy_display_snapshot.is_some());
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
