// SPDX-License-Identifier: MPL-2.0

use crate::account_storage::ProviderAccountStorage;
use crate::config::{Config, ManagedClaudeAccountConfig, paths};
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeAccount {
    pub id: String,
    pub label: String,
    pub email: Option<String>,
    pub organization: Option<String>,
    pub subscription_type: Option<String>,
    pub config_dir: PathBuf,
}

pub fn discover_accounts(config: &Config) -> Vec<ClaudeAccount> {
    let storage = ProviderAccountStorage::new(paths().claude_accounts_dir);
    let mut accounts = Vec::new();
    for managed in &config.claude_managed_accounts {
        let metadata = storage.load_metadata(&managed.id).ok();
        let email = metadata
            .as_ref()
            .map(|m| m.email.clone())
            .filter(|e| !e.is_empty())
            .or_else(|| managed.email.clone());
        let organization = metadata
            .as_ref()
            .and_then(|m| m.organization_name.clone())
            .or_else(|| managed.organization.clone());
        let discovered = ClaudeAccount {
            id: managed.id.clone(),
            label: email.clone().unwrap_or_else(|| managed.label.clone()),
            email,
            organization,
            subscription_type: managed.subscription_type.clone(),
            config_dir: managed.config_dir.clone(),
        };
        if let Some(email_key) = discovered.email.as_deref().map(normalized_email) {
            if let Some(index) = accounts.iter().position(|existing: &ClaudeAccount| {
                existing.email.as_deref().map(normalized_email) == Some(email_key.clone())
            }) {
                let existing = &accounts[index];
                if prefer_account(existing, &discovered, &config.selected_claude_account_ids) {
                    continue;
                }
                accounts[index] = discovered;
            } else {
                accounts.push(discovered);
            }
        } else {
            accounts.push(discovered);
        }
    }
    accounts
}

pub fn apply_login_account(config: &mut Config, account: ManagedClaudeAccountConfig) {
    let account_id = account.id.clone();
    if !config.selected_claude_account_ids.contains(&account_id) {
        config.selected_claude_account_ids.push(account_id.clone());
    }
    config
        .claude_managed_accounts
        .retain(|existing| existing.id != account_id);
    config.claude_managed_accounts.push(account);
    dedupe_managed_accounts(config);
}

pub fn remove_managed_config_dir(config_dir: &Path) {
    let root = paths().claude_accounts_dir;
    let Ok(root) = root.canonicalize() else {
        return;
    };
    let Ok(metadata) = fs::symlink_metadata(config_dir) else {
        return;
    };
    if metadata.file_type().is_symlink() {
        tracing::warn!(path = %config_dir.display(), "refusing to delete symlinked claude account config dir");
        return;
    }
    let Ok(config_dir) = config_dir.canonicalize() else {
        return;
    };
    if !config_dir.starts_with(&root) {
        tracing::warn!(path = %config_dir.display(), root = %root.display(), "refusing to delete claude account outside managed root");
        return;
    }
    if let Err(error) = fs::remove_dir_all(&config_dir) {
        tracing::warn!(path = %config_dir.display(), error = %error, "failed to delete claude account config dir");
    }
}

pub fn dedupe_managed_accounts(config: &mut Config) -> bool {
    let original_selected = config.selected_claude_account_ids.clone();
    let mut selected_ids = original_selected.clone();
    let original_len = config.claude_managed_accounts.len();
    let mut deduped = Vec::new();

    for account in config.claude_managed_accounts.drain(..) {
        let Some(email_key) = account.email.as_deref().map(normalized_email) else {
            deduped.push(account);
            continue;
        };

        if let Some(index) = deduped
            .iter()
            .position(|existing: &ManagedClaudeAccountConfig| {
                existing.email.as_deref().map(normalized_email) == Some(email_key.clone())
            })
        {
            let existing = deduped.remove(index);
            let keep_existing = prefer_managed_account(&existing, &account, &original_selected);
            let (mut winner, loser) = if keep_existing {
                (existing, account)
            } else {
                (account, existing)
            };
            let loser_id = loser.id.clone();
            let winner_id = winner.id.clone();
            merge_account_metadata(&mut winner, &loser);
            for id in &mut selected_ids {
                if id == loser_id.as_str() {
                    id.clone_from(&winner_id);
                }
            }
            deduped.push(winner);
            continue;
        }

        deduped.push(account);
    }

    let changed = deduped.len() != original_len || selected_ids != original_selected;
    config.claude_managed_accounts = deduped;
    config.selected_claude_account_ids = selected_ids;
    changed
}

pub(crate) fn find_matching_account<'a>(
    config: &'a Config,
    email: Option<&str>,
) -> Option<&'a ManagedClaudeAccountConfig> {
    let email = email?;
    let email = normalized_email(email);
    config
        .claude_managed_accounts
        .iter()
        .find(|account| account.email.as_deref().map(normalized_email) == Some(email.clone()))
}

pub(crate) fn normalized_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

fn prefer_account(
    existing: &ClaudeAccount,
    candidate: &ClaudeAccount,
    selected_ids: &[String],
) -> bool {
    if selected_ids.iter().any(|id| id == existing.id.as_str()) {
        return true;
    }
    if selected_ids.iter().any(|id| id == candidate.id.as_str()) {
        return false;
    }
    existing.id >= candidate.id
}

fn prefer_managed_account(
    existing: &ManagedClaudeAccountConfig,
    candidate: &ManagedClaudeAccountConfig,
    selected_ids: &[String],
) -> bool {
    if selected_ids.iter().any(|id| id == existing.id.as_str()) {
        return true;
    }
    if selected_ids.iter().any(|id| id == candidate.id.as_str()) {
        return false;
    }
    let existing_auth = existing.last_authenticated_at.or(Some(existing.updated_at));
    let candidate_auth = candidate
        .last_authenticated_at
        .or(Some(candidate.updated_at));
    existing_auth >= candidate_auth
}

fn merge_account_metadata(
    target: &mut ManagedClaudeAccountConfig,
    source: &ManagedClaudeAccountConfig,
) {
    if target.email.is_none() {
        target.email.clone_from(&source.email);
    }
    if target.organization.is_none() {
        target.organization.clone_from(&source.organization);
    }
    if target.subscription_type.is_none() {
        target
            .subscription_type
            .clone_from(&source.subscription_type);
    }
    if target.email.is_none() && source.email.is_some() {
        target.label.clone_from(&source.label);
    }
    if source.created_at < target.created_at {
        target.created_at = source.created_at;
    }
    if source.updated_at > target.updated_at {
        target.updated_at = source.updated_at;
    }
    if source.last_authenticated_at > target.last_authenticated_at {
        target.last_authenticated_at = source.last_authenticated_at;
    }
}
