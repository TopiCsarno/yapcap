// SPDX-License-Identifier: MPL-2.0

use crate::model::{AccountSelectionStatus, AppState, ProviderId};
use chrono::{DateTime, Utc};
use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};

pub const RUNTIME_DOCUMENT_VERSION: u16 = 1;
pub const CONTROL_DOCUMENT_VERSION: u16 = 1;

#[derive(Debug, Clone, CosmicConfigEntry, Serialize, Deserialize, PartialEq)]
#[version = 1]
pub struct SharedRuntimeState {
    #[serde(default = "runtime_document_version")]
    pub document_version: u16,
    #[serde(default)]
    pub generation: u64,
    #[serde(default = "Utc::now")]
    pub written_at: DateTime<Utc>,
    pub app_state: AppState,
}

#[derive(Debug, Clone, CosmicConfigEntry, Serialize, Deserialize, PartialEq)]
#[version = 2]
pub struct SharedControlState {
    #[serde(default = "control_document_version")]
    pub document_version: u16,
    #[serde(default)]
    pub generation: u64,
    #[serde(default = "Utc::now")]
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub requests: Vec<ProviderRefreshRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderRefreshRequest {
    pub provider: ProviderId,
    pub reason: RefreshRequestReason,
    pub requested_at: DateTime<Utc>,
    pub requesting_process_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reauthenticated_account_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RefreshRequestReason {
    User,
    ProviderSelected,
    AccountAction,
}

#[derive(Clone, Copy)]
pub struct SharedStateWriter<'a> {
    pub process_id: &'a str,
    pub owner_status: &'a str,
}

impl Default for SharedRuntimeState {
    fn default() -> Self {
        Self::new(AppState::empty(), 0)
    }
}

impl SharedRuntimeState {
    #[must_use]
    pub fn new(app_state: AppState, generation: u64) -> Self {
        Self {
            document_version: RUNTIME_DOCUMENT_VERSION,
            generation,
            written_at: Utc::now(),
            app_state,
        }
    }

    #[cfg(test)]
    #[must_use]
    pub fn from_json(raw: &str) -> Option<Self> {
        serde_json::from_str(raw).ok()
    }
}

impl Default for SharedControlState {
    fn default() -> Self {
        Self {
            document_version: CONTROL_DOCUMENT_VERSION,
            generation: 0,
            updated_at: Utc::now(),
            requests: Vec::new(),
        }
    }
}

impl SharedControlState {
    #[must_use]
    pub fn with_cleared_requests(&self) -> Self {
        let mut next = self.clone();
        if next.requests.is_empty() {
            return next;
        }
        next.requests.clear();
        next.generation += 1;
        next.updated_at = Utc::now();
        next
    }

    pub fn upsert_request(&mut self, mut request: ProviderRefreshRequest) {
        if let Some(existing) = self
            .requests
            .iter_mut()
            .find(|existing| existing.provider == request.provider)
        {
            for account_id in existing.reauthenticated_account_ids.drain(..) {
                if !request.reauthenticated_account_ids.contains(&account_id) {
                    request.reauthenticated_account_ids.push(account_id);
                }
            }
            *existing = request;
        } else {
            self.requests.push(request);
        }
        self.generation += 1;
        self.updated_at = Utc::now();
    }

    #[must_use]
    pub fn with_provider_requests_removed_many(&self, providers: &[ProviderId]) -> Self {
        let mut next = self.clone();
        let before = next.requests.len();
        next.requests
            .retain(|request| !providers.contains(&request.provider));
        if next.requests.len() == before {
            return next;
        }
        next.generation += 1;
        next.updated_at = Utc::now();
        next
    }

    #[cfg(test)]
    #[must_use]
    pub fn from_json(raw: &str) -> Option<Self> {
        serde_json::from_str(raw).ok()
    }
}

#[must_use]
pub fn load_runtime(app_id: &str) -> Option<SharedRuntimeState> {
    let ctx = match crate::config::cosmic_config_context(app_id, SharedRuntimeState::VERSION) {
        Ok(ctx) => ctx,
        Err(error) => {
            tracing::warn!(
                error = ?error,
                document_version = RUNTIME_DOCUMENT_VERSION,
                "shared runtime missing; using empty runtime fallback"
            );
            return None;
        }
    };
    match SharedRuntimeState::get_entry(&ctx) {
        Ok(shared) => Some(shared),
        Err((errors, _fallback)) => {
            tracing::warn!(
                error_count = errors.len(),
                document_version = RUNTIME_DOCUMENT_VERSION,
                "shared runtime invalid; using empty runtime fallback"
            );
            None
        }
    }
}

#[must_use]
pub fn load_control(app_id: &str) -> SharedControlState {
    let ctx = match crate::config::cosmic_config_context(app_id, SharedControlState::VERSION) {
        Ok(ctx) => ctx,
        Err(error) => {
            tracing::warn!(
                error = ?error,
                document_version = CONTROL_DOCUMENT_VERSION,
                "shared control missing; using empty control fallback"
            );
            return SharedControlState::default();
        }
    };
    match SharedControlState::get_entry(&ctx) {
        Ok(shared) => shared,
        Err((errors, _fallback)) => {
            tracing::warn!(
                error_count = errors.len(),
                document_version = CONTROL_DOCUMENT_VERSION,
                "shared control invalid; using empty control fallback"
            );
            SharedControlState::default()
        }
    }
}

pub fn save_runtime_as(
    app_id: &str,
    state: &AppState,
    reason: &'static str,
    writer: Option<SharedStateWriter<'_>>,
) -> Result<(), cosmic_config::Error> {
    let ctx = crate::config::cosmic_config_context(app_id, SharedRuntimeState::VERSION)?;
    let generation = SharedRuntimeState::get_entry(&ctx)
        .ok()
        .map_or(1, |shared| shared.generation.saturating_add(1));
    let shared = SharedRuntimeState::new(state.clone(), generation);
    shared.write_entry(&ctx)?;
    let writer_process_id = writer.map(|writer| writer.process_id).unwrap_or("unknown");
    let writer_owner_status = writer
        .map(|writer| writer.owner_status)
        .unwrap_or("unknown");
    tracing::info!(
        reason,
        writer_process_id,
        writer_owner_status,
        generation = shared.generation,
        account_count = shared.app_state.provider_accounts.len(),
        provider_statuses = %runtime_provider_status_summary(&shared.app_state),
        refreshing_providers = %refreshing_provider_labels(&shared.app_state),
        "shared runtime written"
    );
    Ok(())
}

pub fn runtime_provider_status_summary(state: &AppState) -> String {
    let mut ready = 0;
    let mut login_required = 0;
    let mut selection_required = 0;
    let mut unavailable = 0;
    let mut refreshing = 0;
    for provider in &state.providers {
        if provider.is_refreshing {
            refreshing += 1;
        }
        match provider.account_status {
            AccountSelectionStatus::Ready => ready += 1,
            AccountSelectionStatus::LoginRequired => login_required += 1,
            AccountSelectionStatus::SelectionRequired => selection_required += 1,
            AccountSelectionStatus::Unavailable => unavailable += 1,
        }
    }
    format!(
        "ready:{ready},login_required:{login_required},selection_required:{selection_required},unavailable:{unavailable},refreshing:{refreshing}"
    )
}

pub fn refreshing_provider_labels(state: &AppState) -> String {
    let labels = state
        .providers
        .iter()
        .filter(|provider| provider.is_refreshing)
        .map(|provider| provider.provider.label())
        .collect::<Vec<_>>();
    if labels.is_empty() {
        "none".to_string()
    } else {
        labels.join(",")
    }
}

pub fn save_control(app_id: &str, state: &SharedControlState) -> Result<(), cosmic_config::Error> {
    let ctx = crate::config::cosmic_config_context(app_id, SharedControlState::VERSION)?;
    state.write_entry(&ctx)?;
    Ok(())
}

pub fn clear_control_requests(
    app_id: &str,
    state: &SharedControlState,
) -> Result<SharedControlState, cosmic_config::Error> {
    let cleared = state.with_cleared_requests();
    if cleared == *state {
        return Ok(cleared);
    }
    save_control(app_id, &cleared)?;
    Ok(cleared)
}

pub fn remove_control_requests_for_providers(
    app_id: &str,
    state: &SharedControlState,
    providers: &[ProviderId],
) -> Result<SharedControlState, cosmic_config::Error> {
    let next = state.with_provider_requests_removed_many(providers);
    if next == *state {
        return Ok(next);
    }
    save_control(app_id, &next)?;
    Ok(next)
}

fn runtime_document_version() -> u16 {
    RUNTIME_DOCUMENT_VERSION
}

fn control_document_version() -> u16 {
    CONTROL_DOCUMENT_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ProviderRuntimeState;

    #[test]
    fn invalid_shared_runtime_returns_none() {
        assert!(SharedRuntimeState::from_json("{").is_none());
    }

    #[test]
    fn shared_runtime_and_control_use_distinct_config_versions() {
        assert_ne!(SharedRuntimeState::VERSION, SharedControlState::VERSION);
    }

    #[test]
    fn shared_runtime_round_trips_app_state() {
        let mut app_state = AppState::empty();
        app_state.upsert_provider(ProviderRuntimeState::disabled(ProviderId::Claude));
        let shared = SharedRuntimeState::new(app_state.clone(), 7);
        let raw = serde_json::to_string(&shared).unwrap();
        let loaded = SharedRuntimeState::from_json(&raw).unwrap();

        assert_eq!(loaded.document_version, RUNTIME_DOCUMENT_VERSION);
        assert_eq!(loaded.generation, 7);
        assert_eq!(loaded.app_state.providers.len(), app_state.providers.len());
        assert!(
            !loaded
                .app_state
                .provider(ProviderId::Claude)
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn shared_control_round_trips_and_updates_provider_request() {
        let mut control = SharedControlState::default();
        control.upsert_request(ProviderRefreshRequest {
            provider: ProviderId::Codex,
            reason: RefreshRequestReason::User,
            requested_at: Utc::now(),
            requesting_process_id: "first".to_string(),
            reauthenticated_account_ids: Vec::new(),
        });
        control.upsert_request(ProviderRefreshRequest {
            provider: ProviderId::Codex,
            reason: RefreshRequestReason::ProviderSelected,
            requested_at: Utc::now(),
            requesting_process_id: "second".to_string(),
            reauthenticated_account_ids: Vec::new(),
        });

        let raw = serde_json::to_string(&control).unwrap();
        let loaded = SharedControlState::from_json(&raw).unwrap();

        assert_eq!(loaded.document_version, CONTROL_DOCUMENT_VERSION);
        assert_eq!(loaded.generation, 2);
        assert_eq!(loaded.requests.len(), 1);
        assert_eq!(
            loaded.requests[0].reason,
            RefreshRequestReason::ProviderSelected
        );
        assert_eq!(loaded.requests[0].requesting_process_id, "second");
    }

    #[test]
    fn shared_control_clear_removes_requests_and_advances_generation() {
        let mut control = SharedControlState::default();
        control.upsert_request(ProviderRefreshRequest {
            provider: ProviderId::Codex,
            reason: RefreshRequestReason::User,
            requested_at: Utc::now(),
            requesting_process_id: "process".to_string(),
            reauthenticated_account_ids: Vec::new(),
        });
        let generation = control.generation;

        let cleared = control.with_cleared_requests();

        assert!(cleared.requests.is_empty());
        assert_eq!(cleared.generation, generation + 1);
    }
}
