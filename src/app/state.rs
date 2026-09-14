// SPDX-License-Identifier: MPL-2.0

use crate::model::{AppState, ProviderAccountRuntimeState, ProviderId, ProviderRuntimeState};
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
        self.account(provider, first_id)
    }

    #[must_use]
    pub(super) fn account(
        &self,
        provider: ProviderId,
        account_id: &str,
    ) -> Option<&ProviderAccountRuntimeState> {
        self.provider_accounts
            .iter()
            .find(|entry| entry.provider == provider && entry.account_id == account_id)
    }

    #[must_use]
    pub fn selected_account_index(&self, provider: ProviderId) -> usize {
        let Some(selected_id) = self
            .provider(provider)
            .and_then(|entry| entry.selected_account_ids.first())
        else {
            return 0;
        };
        self.accounts_for(provider)
            .iter()
            .position(|account| account.account_id == *selected_id)
            .unwrap_or(0)
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
            if *existing == provider_state {
                return;
            }
            *existing = provider_state;
        } else {
            self.providers.push(provider_state);
        }
        self.updated_at = Utc::now();
    }

    pub fn begin_provider_refresh(
        &mut self,
        provider: ProviderId,
    ) -> Option<chrono::DateTime<Utc>> {
        let state = self.provider_mut(provider)?;
        if !state.enabled || state.is_refreshing {
            return None;
        }
        state.is_refreshing = true;
        state.refresh_started_at = Some(Utc::now());
        state.refresh_started_at
    }

    pub fn finish_provider_refresh(&mut self, provider: ProviderId) {
        if let Some(state) = self.provider_mut(provider) {
            state.is_refreshing = false;
            state.refresh_started_at = None;
        }
    }

    pub fn upsert_account(&mut self, account_state: ProviderAccountRuntimeState) {
        if let Some(existing) = self.provider_accounts.iter_mut().find(|entry| {
            entry.provider == account_state.provider && entry.account_id == account_state.account_id
        }) {
            if *existing == account_state {
                return;
            }
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
    fn upsert_provider_does_not_touch_updated_at_when_unchanged() {
        let mut state = AppState::empty();
        let provider = state.provider(ProviderId::Codex).unwrap().clone();
        let updated_at = state.updated_at;

        state.upsert_provider(provider);

        assert_eq!(state.updated_at, updated_at);
    }

    #[test]
    fn upsert_account_does_not_touch_updated_at_when_unchanged() {
        let mut state = AppState::empty();
        let account = ProviderAccountRuntimeState::empty(ProviderId::Codex, "codex-1", "Codex");
        state.upsert_account(account.clone());
        let updated_at = state.updated_at;

        state.upsert_account(account);

        assert_eq!(state.updated_at, updated_at);
    }
}
