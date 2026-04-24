// SPDX-License-Identifier: MPL-2.0

use crate::config::{Browser, Config, CursorCredentialSource, ManagedCursorAccountConfig, paths};
use crate::providers::cursor::identity::normalized_email;
use crate::providers::cursor::refresh::fetch_with_cookie_header;
use crate::providers::cursor::shared::new_account_id;
use crate::providers::cursor::storage::{
    create_private_dir, managed_account_dir, write_imported_account,
};
use chrono::Utc;
use std::collections::HashSet;

pub fn discover_accounts(config: &Config) -> Vec<ManagedCursorAccountConfig> {
    let mut accounts = config.cursor_managed_accounts.clone();
    accounts.sort_by(|left, right| left.email.cmp(&right.email));
    accounts
}

pub async fn discover_browser_accounts(
    config: Config,
    client: reqwest::Client,
) -> Vec<ManagedCursorAccountConfig> {
    let root = paths().cursor_accounts_dir;
    if create_private_dir(&root).is_err() {
        return Vec::new();
    }

    let existing = config.cursor_managed_accounts;
    let mut seen_emails = HashSet::new();
    let mut discovered = Vec::new();

    for &browser in &Browser::ALL {
        let Ok(profiles) = browser.cookie_profiles() else {
            continue;
        };
        for profile in profiles {
            if !profile.cookie_db_path.exists() {
                continue;
            }
            let Ok(cookie_header) = crate::providers::cursor::refresh::cookie_header_from_db(
                browser,
                &profile.cookie_db_path,
            )
            .await
            else {
                continue;
            };
            let Ok(snapshot) = fetch_with_cookie_header(&client, &cookie_header).await else {
                continue;
            };
            let Some(email) = snapshot.identity.email.as_deref() else {
                continue;
            };
            let email = normalized_email(email);
            if !seen_emails.insert(email.clone()) {
                continue;
            }
            let existing_account = existing
                .iter()
                .find(|account| normalized_email(&account.email) == email)
                .cloned();
            let now = Utc::now();
            let label = email.clone();
            let display_name = snapshot.identity.display_name.clone();
            let plan = snapshot.identity.plan.clone();
            let browser_metadata = existing_account
                .as_ref()
                .and_then(|account| account.browser)
                .or(Some(browser));
            let created_at = existing_account
                .as_ref()
                .map_or(now, |account| account.created_at);
            let id = existing_account
                .as_ref()
                .map_or_else(new_account_id, |account| account.id.clone());
            let account = ManagedCursorAccountConfig {
                id: id.clone(),
                email: email.clone(),
                label,
                account_root: managed_account_dir(&id),
                credential_source: CursorCredentialSource::ImportedBrowserProfile,
                browser: browser_metadata,
                display_name,
                plan,
                created_at,
                updated_at: now,
                last_authenticated_at: Some(now),
            };
            if write_imported_account(&account, &cookie_header).is_err() {
                continue;
            }
            discovered.push(account);
        }
    }

    discovered
}
