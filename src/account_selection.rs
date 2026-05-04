// SPDX-License-Identifier: MPL-2.0

use crate::config::Config;
use crate::model::ProviderId;
use crate::providers::cursor;

pub const MAX_MULTI_ACCOUNT_SELECTION: usize = 4;

#[must_use]
pub fn provider_show_all_account_selection(config: &Config, provider: ProviderId) -> Vec<String> {
    let available_ids: Vec<String> = match provider {
        ProviderId::Codex => config
            .codex_managed_accounts
            .iter()
            .map(|a| a.id.clone())
            .collect(),
        ProviderId::Claude => config
            .claude_managed_accounts
            .iter()
            .map(|a| a.id.clone())
            .collect(),
        ProviderId::Cursor => config
            .cursor_managed_accounts
            .iter()
            .map(|a| cursor::managed_account_id(&a.id))
            .collect(),
    };
    let active_id = config
        .selected_account_ids(provider)
        .first()
        .map(String::as_str);

    show_all_account_selection(&available_ids, active_id)
}

#[must_use]
fn show_all_account_selection(available_ids: &[String], active_id: Option<&str>) -> Vec<String> {
    let active_id = active_id.filter(|id| available_ids.iter().any(|available| available == id));
    let mut selected = Vec::with_capacity(available_ids.len().min(MAX_MULTI_ACCOUNT_SELECTION));

    if let Some(id) = active_id {
        selected.push(id.to_string());
    }

    for id in available_ids {
        if selected.len() >= MAX_MULTI_ACCOUNT_SELECTION {
            break;
        }
        if active_id == Some(id.as_str()) {
            continue;
        }
        selected.push(id.clone());
    }

    selected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ManagedCodexAccountConfig;
    use chrono::Utc;
    use std::path::PathBuf;

    #[test]
    fn show_all_selection_keeps_active_account_then_fills_to_cap() {
        let available_ids = ["one", "two", "three", "four", "five"]
            .map(str::to_string)
            .to_vec();

        let selected = show_all_account_selection(&available_ids, Some("five"));

        assert_eq!(selected, ["five", "one", "two", "three"]);
    }

    #[test]
    fn show_all_selection_keeps_all_accounts_when_at_cap() {
        let available_ids = ["one", "two", "three", "four"].map(str::to_string).to_vec();

        let selected = show_all_account_selection(&available_ids, Some("two"));

        assert_eq!(selected, ["two", "one", "three", "four"]);
    }

    #[test]
    fn show_all_selection_uses_stable_order_without_available_active_account() {
        let available_ids = ["one", "two", "three", "four", "five"]
            .map(str::to_string)
            .to_vec();

        let selected = show_all_account_selection(&available_ids, Some("missing"));

        assert_eq!(selected, ["one", "two", "three", "four"]);
    }

    #[test]
    fn codex_show_all_selection_keeps_active_account_then_caps_to_four() {
        let mut config = Config {
            codex_managed_accounts: (1..=5).map(codex_account).collect(),
            selected_codex_account_ids: vec!["codex-5".to_string()],
            ..Config::default()
        };
        config.set_provider_show_all(ProviderId::Codex, true);

        let selected = provider_show_all_account_selection(&config, ProviderId::Codex);

        assert_eq!(selected, ["codex-5", "codex-1", "codex-2", "codex-3"]);
        assert_eq!(config.codex_managed_accounts.len(), 5);
    }

    fn codex_account(index: usize) -> ManagedCodexAccountConfig {
        let id = format!("codex-{index}");
        ManagedCodexAccountConfig {
            id: id.clone(),
            label: id,
            codex_home: PathBuf::from(format!("/tmp/yapcap/codex-{index}")),
            email: Some(format!("user{index}@example.com")),
            provider_account_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_authenticated_at: None,
        }
    }
}
