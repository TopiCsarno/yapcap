// SPDX-License-Identifier: MPL-2.0

use crate::account_storage::validated_account_dir;
use crate::config::{Config, ManagedSyntheticAccountConfig, paths};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntheticAccount {
    pub id: String,
    pub label: String,
    pub config_dir: PathBuf,
}

pub fn discover_accounts(config: &Config) -> Vec<SyntheticAccount> {
    config
        .synthetic_managed_accounts
        .iter()
        .filter_map(|managed| {
            validated_account_dir(&paths().synthetic_accounts_dir, &managed.id)
                .ok()
                .map(|config_dir| SyntheticAccount {
                    id: managed.id.clone(),
                    label: managed.label.clone(),
                    config_dir,
                })
        })
        .collect()
}

pub fn apply_login_account(config: &mut Config, account: ManagedSyntheticAccountConfig) {
    let account_id = account.id.clone();
    config
        .synthetic_managed_accounts
        .retain(|existing| existing.id != account_id);
    config.synthetic_managed_accounts.push(account);
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn apply_login_account_replaces_matching_account() {
        let mut config = Config {
            synthetic_managed_accounts: vec![account("synthetic-1", "First")],
            ..Config::default()
        };

        apply_login_account(&mut config, account("synthetic-1", "Updated"));

        assert_eq!(config.synthetic_managed_accounts.len(), 1);
        assert_eq!(config.synthetic_managed_accounts[0].label, "Updated");
    }

    #[test]
    fn apply_login_account_keeps_distinct_account() {
        let mut config = Config {
            synthetic_managed_accounts: vec![account("synthetic-1", "First")],
            ..Config::default()
        };

        apply_login_account(&mut config, account("synthetic-2", "Second"));

        assert_eq!(config.synthetic_managed_accounts.len(), 2);
    }

    fn account(id: &str, label: &str) -> ManagedSyntheticAccountConfig {
        let now = Utc::now();
        ManagedSyntheticAccountConfig {
            id: id.to_string(),
            label: label.to_string(),
            api_key_source: "stored".to_string(),
            created_at: now,
            updated_at: now,
            last_authenticated_at: Some(now),
        }
    }
}
