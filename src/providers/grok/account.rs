// SPDX-License-Identifier: MPL-2.0

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::account_selection::select_account_after_login;
use crate::account_storage::ProviderAccountStorage;
use crate::config::{
    Config, ManagedGrokAccountConfig, host_user_home_dir, managed_grok_account_dir, paths,
};
use crate::model::ProviderId;

pub const GROK_AUTH_KEY: &str = "https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostGrokCredentials {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub email: Option<String>,
    pub user_id: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub team_id: Option<String>,
}

pub fn host_auth_file_path() -> Option<PathBuf> {
    host_user_home_dir().map(|home| home.join(".grok").join("auth.json"))
}

fn parse_trimmed_str(entry: &serde_json::Value, key: &str) -> Option<String> {
    entry
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn parse_expires_at(entry: &serde_json::Value) -> Option<DateTime<Utc>> {
    let exp = entry.get("expires_at")?;
    if let Some(ts) = exp.as_i64() {
        DateTime::from_timestamp(ts, 0)
    } else if let Some(s) = exp.as_str() {
        s.parse::<i64>()
            .ok()
            .and_then(|ts| DateTime::from_timestamp(ts, 0))
            .or_else(|| {
                chrono::DateTime::parse_from_rfc3339(s)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            })
    } else {
        None
    }
}

pub fn read_host_credentials(path: &Path) -> Option<HostGrokCredentials> {
    let content = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    let entry = json.get(GROK_AUTH_KEY).or_else(|| {
        json.as_object().and_then(|obj| {
            obj.iter()
                .find(|(k, _)| k.starts_with("https://auth.x.ai::"))
                .map(|(_, v)| v)
        })
    })?;

    let access_token =
        parse_trimmed_str(entry, "key").or_else(|| parse_trimmed_str(entry, "access_token"))?;

    Some(HostGrokCredentials {
        access_token,
        refresh_token: parse_trimmed_str(entry, "refresh_token"),
        expires_at: parse_expires_at(entry),
        email: parse_trimmed_str(entry, "email"),
        user_id: parse_trimmed_str(entry, "user_id").or_else(|| parse_trimmed_str(entry, "sub")),
        first_name: parse_trimmed_str(entry, "first_name"),
        last_name: parse_trimmed_str(entry, "last_name"),
        team_id: parse_trimmed_str(entry, "team_id"),
    })
}

pub fn system_active_account_id(
    managed_accounts: &[ManagedGrokAccountConfig],
    grok_auth_path: &Path,
) -> Option<String> {
    let creds = read_host_credentials(grok_auth_path)?;
    if let Some(ref uid) = creds.user_id
        && let Some(account) = managed_accounts
            .iter()
            .find(|account| account.provider_account_id.as_deref() == Some(uid.as_str()))
    {
        return Some(account.id.clone());
    }
    if let Some(ref email) = creds.email {
        let norm_email = normalized_email(email);
        if let Some(account) = managed_accounts.iter().find(|account| {
            if let (Some(host_uid), Some(acct_uid)) = (&creds.user_id, &account.provider_account_id)
                && host_uid != acct_uid
            {
                return false;
            }
            account.email.as_deref().map(normalized_email).as_deref() == Some(norm_email.as_str())
        }) {
            return Some(account.id.clone());
        }
    }
    None
}

pub fn discover_accounts(config: &Config) -> Vec<ManagedGrokAccountConfig> {
    let storage = ProviderAccountStorage::new(paths().grok_accounts_dir);
    let mut accounts = Vec::new();
    for managed in &config.grok_managed_accounts {
        let metadata = storage.load_metadata(&managed.id).ok();
        let email = metadata
            .as_ref()
            .map(|m| m.email.clone())
            .filter(|e| !e.is_empty())
            .or_else(|| managed.email.clone());
        let provider_account_id = metadata
            .as_ref()
            .and_then(|m| m.provider_account_id.clone())
            .or_else(|| managed.provider_account_id.clone());
        let label = if managed.label.trim().is_empty() {
            email.clone().unwrap_or_else(|| "Grok account".to_string())
        } else {
            managed.label.clone()
        };
        let mut discovered = managed.clone();
        discovered.config_dir = managed_grok_account_dir(&managed.id);
        discovered.email = email;
        discovered.provider_account_id = provider_account_id;
        discovered.label = label;

        if let Some(index) = accounts
            .iter()
            .position(|existing: &ManagedGrokAccountConfig| same_identity(existing, &discovered))
        {
            let existing = &accounts[index];
            if prefer_managed_account(existing, &discovered, &config.selected_grok_account_ids) {
                continue;
            }
            accounts[index] = discovered;
        } else {
            accounts.push(discovered);
        }
    }
    accounts
}

pub fn apply_login_account(config: &mut Config, account: ManagedGrokAccountConfig) {
    let incoming = account.clone();
    config
        .grok_managed_accounts
        .retain(|existing| existing.id != account.id);
    config.grok_managed_accounts.push(account);
    dedupe_managed_accounts(config);
    let selected_id = config
        .grok_managed_accounts
        .iter()
        .find(|existing| same_identity(existing, &incoming))
        .map(|survivor| survivor.id.clone())
        .unwrap_or(incoming.id);
    select_account_after_login(config, ProviderId::Grok, selected_id);
}

pub fn sync_managed_account_dirs(config: &mut Config) -> bool {
    let deduped = dedupe_managed_accounts(config);
    let mut dirs_changed = false;
    for account in &mut config.grok_managed_accounts {
        let canonical = managed_grok_account_dir(&account.id);
        if account.config_dir != canonical {
            account.config_dir = canonical;
            dirs_changed = true;
        }
    }
    deduped || dirs_changed
}

pub fn dedupe_managed_accounts(config: &mut Config) -> bool {
    let original_selected = config.selected_grok_account_ids.clone();
    let mut selected_ids = original_selected.clone();
    let original_len = config.grok_managed_accounts.len();
    let mut deduped = Vec::new();

    for account in config.grok_managed_accounts.drain(..) {
        if let Some(index) = deduped
            .iter()
            .position(|existing: &ManagedGrokAccountConfig| same_identity(existing, &account))
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
    config.grok_managed_accounts = deduped;
    config.selected_grok_account_ids = selected_ids;
    changed
}

pub fn normalized_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

pub fn new_account_id() -> String {
    let millis = Utc::now().timestamp_millis();
    format!("grok-{millis}-{}", std::process::id())
}

pub(crate) fn find_matching_account<'a>(
    config: &'a Config,
    email: Option<&str>,
    provider_account_id: Option<&str>,
) -> Option<&'a ManagedGrokAccountConfig> {
    config.grok_managed_accounts.iter().find(|account| {
        match (account.provider_account_id.as_deref(), provider_account_id) {
            (Some(acc_uid), Some(uid)) => acc_uid == uid,
            _ => match (account.email.as_deref(), email) {
                (Some(acc_email), Some(email)) => {
                    normalized_email(acc_email) == normalized_email(email)
                }
                _ => false,
            },
        }
    })
}

fn same_identity(a: &ManagedGrokAccountConfig, b: &ManagedGrokAccountConfig) -> bool {
    if a.id == b.id {
        return true;
    }
    match (&a.provider_account_id, &b.provider_account_id) {
        (Some(id_a), Some(id_b)) => id_a == id_b,
        _ => match (&a.email, &b.email) {
            (Some(email_a), Some(email_b)) => {
                normalized_email(email_a) == normalized_email(email_b)
            }
            _ => false,
        },
    }
}

fn prefer_managed_account(
    existing: &ManagedGrokAccountConfig,
    candidate: &ManagedGrokAccountConfig,
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
    target: &mut ManagedGrokAccountConfig,
    source: &ManagedGrokAccountConfig,
) {
    if target.email.is_none() {
        target.email.clone_from(&source.email);
    }
    if target.provider_account_id.is_none() {
        target
            .provider_account_id
            .clone_from(&source.provider_account_id);
    }
    if target.team_id.is_none() {
        target.team_id.clone_from(&source.team_id);
    }
    if target.plan.is_none() {
        target.plan.clone_from(&source.plan);
    }
    if target.label == "Grok account" && source.label != "Grok account" {
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
