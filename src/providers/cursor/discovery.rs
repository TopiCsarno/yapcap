// SPDX-License-Identifier: MPL-2.0

use crate::config::{Config, ManagedCursorAccountConfig, paths};
use crate::providers::cursor::identity::normalized_email;
use crate::providers::cursor::storage::managed_account_dir;

pub fn discover_accounts(config: &Config) -> Vec<ManagedCursorAccountConfig> {
    let storage = crate::account_storage::ProviderAccountStorage::new(paths().cursor_accounts_dir);
    let mut accounts = config
        .cursor_managed_accounts
        .iter()
        .filter_map(|account| {
            let metadata = storage.load_metadata(&account.id).ok()?;
            storage.load_tokens(&account.id).ok()?;
            let mut account = account.clone();
            account.email = normalized_email(&metadata.email);
            account.label.clone_from(&account.email);
            account.account_root = managed_account_dir(&account.id);
            (!account.email.is_empty()).then_some(account)
        })
        .collect::<Vec<_>>();
    accounts.sort_by(|left, right| left.email.cmp(&right.email));
    accounts
}
