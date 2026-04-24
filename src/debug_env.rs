// SPDX-License-Identifier: MPL-2.0

use crate::model::{AccountSelectionStatus, AppState, ProviderId, ProviderRuntimeState};

const DEBUG_ENV: &str = "YAPCAP_DEBUG";

pub fn apply(state: &mut AppState) {
    let Ok(raw) = std::env::var(DEBUG_ENV) else {
        return;
    };

    for token in raw.split(',').map(str::trim) {
        let provider = match token {
            "no_codex" => Some(ProviderId::Codex),
            "no_claude" => Some(ProviderId::Claude),
            "no_cursor" => Some(ProviderId::Cursor),
            _ => None,
        };

        if let Some(provider) = provider {
            let mut simulated = ProviderRuntimeState::empty(provider);
            simulated.account_status = AccountSelectionStatus::LoginRequired;
            simulated.error = None;
            state.upsert_provider(simulated);
            state.provider_accounts.retain(|a| a.provider != provider);
        }
    }
}
