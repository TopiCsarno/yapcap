// SPDX-License-Identifier: MPL-2.0

use crate::account_storage::{ProviderAccountStorage, validated_account_dir};
use crate::config::{Config, ManagedCommandCodeAccountConfig, host_user_home_dir, paths};
use chrono::Utc;
use serde::Deserialize;
use std::io::Read;
use std::path::{Path, PathBuf};

const API_KEY_FILE: &str = "api_key.txt";

pub(crate) fn storage() -> ProviderAccountStorage {
    ProviderAccountStorage::new(paths().command_code_accounts_dir)
}

pub(crate) fn load_key(account_id: &str) -> Result<String, String> {
    storage()
        .read_text_file(account_id, API_KEY_FILE)
        .map_err(|_| "Command Code credentials could not be read".to_string())
}

pub(crate) fn write_key(account_id: &str, key: &str) -> Result<(), String> {
    storage()
        .write_text_file(account_id, API_KEY_FILE, key)
        .map_err(|_| "Command Code credentials could not be saved".to_string())
}

pub(crate) fn host_auth_path() -> Option<PathBuf> {
    Some(host_user_home_dir()?.join(".commandcode/auth.json"))
}

pub(crate) fn import_key() -> Result<String, String> {
    let path =
        host_auth_path().ok_or_else(|| "Command Code home directory is unavailable".to_string())?;
    import_key_at(&path)
}

pub(crate) fn import_key_at(path: &Path) -> Result<String, String> {
    #[derive(Deserialize)]
    struct HostCredentials {
        #[serde(rename = "apiKey")]
        api_key: String,
    }
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| "No Command Code CLI login found; run command-code login first".to_string())?;
    if !metadata.is_file() || metadata.len() > 65_536 {
        return Err("Command Code CLI credentials could not be read".to_string());
    }
    let mut contents = String::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(65_537).read_to_string(&mut contents))
        .map_err(|_| "Command Code CLI credentials could not be read".to_string())?;
    let credentials: HostCredentials = serde_json::from_str(&contents)
        .map_err(|_| "Command Code CLI credentials are invalid".to_string())?;
    normalize_key(&credentials.api_key)
}

pub(crate) fn normalize_key(value: &str) -> Result<String, String> {
    let key = value.trim();
    if key.is_empty()
        || key.len() > 8192
        || key.chars().any(char::is_whitespace)
        || key.chars().any(char::is_control)
    {
        return Err("Enter a valid Command Code API key".to_string());
    }
    Ok(key.to_string())
}

pub(crate) fn descriptors(config: &Config) -> Vec<ManagedCommandCodeAccountConfig> {
    config
        .command_code_managed_accounts
        .iter()
        .filter(|account| {
            validated_account_dir(&paths().command_code_accounts_dir, &account.id).is_ok()
        })
        .cloned()
        .collect()
}

pub(crate) fn new_account_id() -> String {
    format!(
        "command-code-{}",
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    )
}

pub(crate) fn apply(config: &mut Config, account: ManagedCommandCodeAccountConfig) {
    config
        .command_code_managed_accounts
        .retain(|existing| existing.id != account.id);
    config.command_code_managed_accounts.push(account);
}
