// SPDX-License-Identifier: MPL-2.0

use crate::account_storage::validated_account_dir;
use crate::config::{Config, ManagedOpenRouterAccountConfig, paths};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRouterAccount {
    pub id: String,
    pub label: String,
    pub config_dir: PathBuf,
}

pub fn discover_accounts(config: &Config) -> Vec<OpenRouterAccount> {
    config
        .openrouter_managed_accounts
        .iter()
        .filter_map(|managed| {
            validated_account_dir(&paths().openrouter_accounts_dir, &managed.id)
                .ok()
                .map(|config_dir| OpenRouterAccount {
                    id: managed.id.clone(),
                    label: managed.label.clone(),
                    config_dir,
                })
        })
        .collect()
}

pub fn apply_login_account(config: &mut Config, account: ManagedOpenRouterAccountConfig) {
    let account_id = account.id.clone();
    config
        .openrouter_managed_accounts
        .retain(|existing| existing.id != account_id);
    config.openrouter_managed_accounts.push(account);
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn apply_login_account_replaces_matching_account() {
        let mut config = Config {
            openrouter_managed_accounts: vec![account("openrouter-1", "First")],
            ..Config::default()
        };

        apply_login_account(&mut config, account("openrouter-1", "Updated"));

        assert_eq!(config.openrouter_managed_accounts.len(), 1);
        assert_eq!(config.openrouter_managed_accounts[0].label, "Updated");
    }

    fn account(id: &str, label: &str) -> ManagedOpenRouterAccountConfig {
        let now = Utc::now();
        ManagedOpenRouterAccountConfig {
            id: id.to_string(),
            label: label.to_string(),
            api_key_source: "stored".to_string(),
            created_at: now,
            updated_at: now,
            last_authenticated_at: Some(now),
        }
    }
}
