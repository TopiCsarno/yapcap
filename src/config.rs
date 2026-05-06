// SPDX-License-Identifier: MPL-2.0

use crate::model::ProviderId;
use chrono::{DateTime, Utc};
use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use dirs::{cache_dir, state_dir};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Clone, CosmicConfigEntry, Serialize, Deserialize, Eq, PartialEq)]
#[version = 400]
pub struct Config {
    pub refresh_interval_seconds: u64,
    pub reset_time_format: ResetTimeFormat,
    pub usage_amount_format: UsageAmountFormat,
    pub panel_icon_style: PanelIconStyle,
    #[serde(default = "default_provider_visibility_mode")]
    pub provider_visibility_mode: ProviderVisibilityMode,
    pub codex_enabled: bool,
    pub claude_enabled: bool,
    pub cursor_enabled: bool,
    #[serde(default)]
    pub show_all_accounts: HashSet<ProviderId>,
    pub selected_codex_account_ids: Vec<String>,
    pub codex_managed_accounts: Vec<ManagedCodexAccountConfig>,
    pub selected_claude_account_ids: Vec<String>,
    pub claude_managed_accounts: Vec<ManagedClaudeAccountConfig>,
    pub selected_cursor_account_ids: Vec<String>,
    pub cursor_managed_accounts: Vec<ManagedCursorAccountConfig>,
    pub log_level: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            refresh_interval_seconds: 300,
            reset_time_format: ResetTimeFormat::Relative,
            usage_amount_format: UsageAmountFormat::Used,
            panel_icon_style: PanelIconStyle::LogoAndBars,
            provider_visibility_mode: ProviderVisibilityMode::AutoInitPending,
            codex_enabled: true,
            claude_enabled: true,
            cursor_enabled: true,
            show_all_accounts: HashSet::new(),
            selected_codex_account_ids: Vec::new(),
            codex_managed_accounts: Vec::new(),
            selected_claude_account_ids: Vec::new(),
            claude_managed_accounts: Vec::new(),
            selected_cursor_account_ids: Vec::new(),
            cursor_managed_accounts: Vec::new(),
            log_level: "info".to_string(),
        }
    }
}

fn default_provider_visibility_mode() -> ProviderVisibilityMode {
    ProviderVisibilityMode::UserManaged
}

impl Config {
    #[must_use]
    pub fn provider_enabled(&self, provider: ProviderId) -> bool {
        match provider {
            ProviderId::Codex => self.codex_enabled,
            ProviderId::Claude => self.claude_enabled,
            ProviderId::Cursor => self.cursor_enabled,
        }
    }

    #[must_use]
    pub fn selected_account_ids(&self, provider: ProviderId) -> &[String] {
        match provider {
            ProviderId::Codex => &self.selected_codex_account_ids,
            ProviderId::Claude => &self.selected_claude_account_ids,
            ProviderId::Cursor => &self.selected_cursor_account_ids,
        }
    }

    pub fn selected_account_ids_mut(&mut self, provider: ProviderId) -> &mut Vec<String> {
        match provider {
            ProviderId::Codex => &mut self.selected_codex_account_ids,
            ProviderId::Claude => &mut self.selected_claude_account_ids,
            ProviderId::Cursor => &mut self.selected_cursor_account_ids,
        }
    }

    #[must_use]
    pub fn show_all_accounts(&self, provider: ProviderId) -> bool {
        self.show_all_accounts.contains(&provider)
    }

    pub fn set_provider_show_all(&mut self, provider: ProviderId, show_all: bool) {
        if show_all {
            self.show_all_accounts.insert(provider);
        } else {
            self.show_all_accounts.remove(&provider);
        }
    }

    pub fn set_provider_enabled(&mut self, provider: ProviderId, enabled: bool) -> bool {
        let target = match provider {
            ProviderId::Codex => &mut self.codex_enabled,
            ProviderId::Claude => &mut self.claude_enabled,
            ProviderId::Cursor => &mut self.cursor_enabled,
        };
        let changed = *target != enabled;
        *target = enabled;
        changed
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PanelIconStyle {
    #[default]
    LogoAndBars,
    BarsOnly,
    LogoAndPercent,
    PercentOnly,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResetTimeFormat {
    #[default]
    Relative,
    Absolute,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UsageAmountFormat {
    #[default]
    Used,
    Left,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderVisibilityMode {
    AutoInitPending,
    #[default]
    UserManaged,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedCodexAccountConfig {
    pub id: String,
    pub label: String,
    pub codex_home: PathBuf,
    pub email: Option<String>,
    pub provider_account_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_authenticated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedClaudeAccountConfig {
    pub id: String,
    pub label: String,
    pub config_dir: PathBuf,
    pub email: Option<String>,
    pub organization: Option<String>,
    pub subscription_type: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_authenticated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedCursorAccountConfig {
    #[serde(default)]
    pub id: String,
    #[serde(default, deserialize_with = "deserialize_cursor_email")]
    pub email: String,
    pub label: String,
    #[serde(alias = "profile_root")]
    pub account_root: PathBuf,
    pub display_name: Option<String>,
    pub plan: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_authenticated_at: Option<DateTime<Utc>>,
}

fn deserialize_cursor_email<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum EmailValue {
        Text(String),
        Maybe(Option<String>),
    }

    Ok(match EmailValue::deserialize(deserializer)? {
        EmailValue::Text(value) => value,
        EmailValue::Maybe(value) => value.unwrap_or_default(),
    })
}

pub struct AppPaths {
    pub cache_dir: PathBuf,
    pub snapshot_file: PathBuf,
    pub codex_accounts_dir: PathBuf,
    pub claude_accounts_dir: PathBuf,
    pub cursor_accounts_dir: PathBuf,
    pub log_dir: PathBuf,
}

fn flatpak_var_app_subdir(segments: &[&str]) -> Option<PathBuf> {
    let app_id = std::env::var_os("FLATPAK_ID")?;
    let mut path = host_user_home_dir()?;
    path.push(".var");
    path.push("app");
    path.push(app_id);
    for seg in segments {
        path.push(seg);
    }
    Some(path)
}

fn cache_root_dir() -> PathBuf {
    if std::env::var_os("FLATPAK_ID").is_some() {
        flatpak_var_app_subdir(&["cache"])
            .or_else(cache_dir)
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        cache_dir().unwrap_or_else(|| PathBuf::from("."))
    }
}

fn state_parent_dir() -> PathBuf {
    if std::env::var_os("FLATPAK_ID").is_some() {
        flatpak_var_app_subdir(&["data"])
            .or_else(state_dir)
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        state_dir().unwrap_or_else(|| PathBuf::from("."))
    }
}

#[must_use]
pub fn host_user_home_dir() -> Option<PathBuf> {
    if std::env::var_os("FLATPAK_ID").is_none() {
        return dirs::home_dir();
    }
    passwd_home_dir().or_else(dirs::home_dir)
}

#[cfg(unix)]
fn passwd_home_dir() -> Option<PathBuf> {
    use libc::{c_char, c_int, getpwuid_r, getuid, passwd};
    use std::ffi::CStr;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStringExt;
    use std::ptr;

    let uid = unsafe { getuid() };
    let mut pwd: MaybeUninit<passwd> = MaybeUninit::uninit();
    let mut result: *mut passwd = ptr::null_mut();
    let mut buf = vec![0u8; 16 * 1024];
    let err: c_int = unsafe {
        getpwuid_r(
            uid,
            pwd.as_mut_ptr(),
            buf.as_mut_ptr().cast::<c_char>(),
            buf.len(),
            &raw mut result,
        )
    };
    if err != 0 || result.is_null() {
        return None;
    }
    let pwd = unsafe { pwd.assume_init() };
    if pwd.pw_dir.is_null() {
        return None;
    }
    let bytes = unsafe { CStr::from_ptr(pwd.pw_dir) }.to_bytes();
    if bytes.is_empty() {
        return None;
    }
    Some(PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec())))
}

#[cfg(not(unix))]
fn passwd_home_dir() -> Option<PathBuf> {
    None
}

#[must_use]
pub fn managed_codex_account_dir(account_id: &str) -> PathBuf {
    paths().codex_accounts_dir.join(account_id)
}

#[must_use]
pub fn managed_claude_account_dir(account_id: &str) -> PathBuf {
    paths().claude_accounts_dir.join(account_id)
}

#[must_use]
pub fn paths() -> AppPaths {
    let cache_root = cache_root_dir();
    let state_root = state_parent_dir();
    let cache_dir = cache_root.join("yapcap");
    let state_dir = state_root.join("yapcap");
    let codex_accounts_dir = state_dir.join("codex-accounts");
    let claude_accounts_dir = state_dir.join("claude-accounts");
    let cursor_accounts_dir = state_dir.join("cursor-accounts");
    let log_dir = state_dir.join("logs");
    AppPaths {
        snapshot_file: cache_dir.join("snapshots.json"),
        cache_dir,
        codex_accounts_dir,
        claude_accounts_dir,
        cursor_accounts_dir,
        log_dir,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_enables_all_providers() {
        let config = Config::default();
        assert!(config.provider_enabled(ProviderId::Codex));
        assert!(config.provider_enabled(ProviderId::Claude));
        assert!(config.provider_enabled(ProviderId::Cursor));
        assert_eq!(
            config.provider_visibility_mode,
            ProviderVisibilityMode::AutoInitPending
        );
        assert_eq!(config.refresh_interval_seconds, 300);
        assert_eq!(config.reset_time_format, ResetTimeFormat::Relative);
        assert_eq!(config.usage_amount_format, UsageAmountFormat::Used);
        assert_eq!(config.panel_icon_style, PanelIconStyle::LogoAndBars);
    }

    #[test]
    fn config_schema_version_marks_fresh_patch_boundary() {
        let config = Config::default();
        assert_eq!(Config::VERSION, 400);
        assert!(config.codex_managed_accounts.is_empty());
        assert!(config.claude_managed_accounts.is_empty());
        assert!(config.cursor_managed_accounts.is_empty());
    }

    #[test]
    fn missing_provider_visibility_mode_defaults_to_user_managed() {
        let config: Config = serde_json::from_str(
            r#"{
                "refresh_interval_seconds": 60,
                "reset_time_format": "relative",
                "usage_amount_format": "used",
                "panel_icon_style": "logo_and_bars",
                "codex_enabled": true,
                "claude_enabled": true,
                "cursor_enabled": true,
                "selected_codex_account_ids": [],
                "codex_managed_accounts": [],
                "selected_claude_account_ids": [],
                "claude_managed_accounts": [],
                "selected_cursor_account_ids": [],
                "cursor_managed_accounts": [],
                "cursor_browser": "brave",
                "log_level": "info"
            }"#,
        )
        .unwrap();

        assert_eq!(
            config.provider_visibility_mode,
            ProviderVisibilityMode::UserManaged
        );
    }

    #[test]
    fn legacy_cursor_discovery_fields_are_ignored() {
        let config: Config = serde_json::from_str(
            r#"{
                "refresh_interval_seconds": 60,
                "reset_time_format": "relative",
                "usage_amount_format": "used",
                "panel_icon_style": "logo_and_bars",
                "provider_visibility_mode": "user_managed",
                "codex_enabled": true,
                "claude_enabled": true,
                "cursor_enabled": true,
                "show_all_accounts": [],
                "selected_codex_account_ids": [],
                "codex_managed_accounts": [],
                "selected_claude_account_ids": [],
                "claude_managed_accounts": [],
                "selected_cursor_account_ids": [],
                "cursor_managed_accounts": [{
                    "id": "cursor-test",
                    "email": "user@example.com",
                    "label": "user@example.com",
                    "account_root": "/tmp/yapcap/cursor-test",
                    "credential_source": "imported_browser_profile",
                    "browser": "brave",
                    "display_name": null,
                    "plan": null,
                    "created_at": "2026-04-30T00:00:00Z",
                    "updated_at": "2026-04-30T00:00:00Z",
                    "last_authenticated_at": null
                }],
                "cursor_browser": "brave",
                "cursor_profile_id": "Default",
                "log_level": "info"
            }"#,
        )
        .unwrap();

        assert_eq!(config.cursor_managed_accounts.len(), 1);
        assert_eq!(config.cursor_managed_accounts[0].id, "cursor-test");
    }

    #[test]
    fn panel_icon_style_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&PanelIconStyle::LogoAndBars).unwrap(),
            "\"logo_and_bars\""
        );
        assert_eq!(
            serde_json::from_str::<PanelIconStyle>("\"bars_only\"").unwrap(),
            PanelIconStyle::BarsOnly
        );
        assert_eq!(
            serde_json::from_str::<PanelIconStyle>("\"logo_and_percent\"").unwrap(),
            PanelIconStyle::LogoAndPercent
        );
        assert_eq!(
            serde_json::from_str::<PanelIconStyle>("\"percent_only\"").unwrap(),
            PanelIconStyle::PercentOnly
        );
    }

    #[test]
    fn usage_amount_format_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&UsageAmountFormat::Used).unwrap(),
            "\"used\""
        );
        assert_eq!(
            serde_json::from_str::<UsageAmountFormat>("\"left\"").unwrap(),
            UsageAmountFormat::Left
        );
    }

    #[test]
    fn flatpak_paths_use_dot_var_layout() {
        let mut env = crate::test_support::test_env();
        env.set("FLATPAK_ID", "com.example.YapCapTest");
        let p = paths();

        use std::path::Path;
        assert!(
            p.cache_dir
                .ends_with(Path::new("com.example.YapCapTest/cache/yapcap")),
            "unexpected cache_dir: {}",
            p.cache_dir.display()
        );
        assert!(
            p.claude_accounts_dir.ends_with(Path::new(
                "com.example.YapCapTest/data/yapcap/claude-accounts"
            )),
            "unexpected claude_accounts_dir: {}",
            p.claude_accounts_dir.display()
        );
        assert!(
            p.log_dir
                .ends_with(Path::new("com.example.YapCapTest/data/yapcap/logs")),
            "unexpected log_dir: {}",
            p.log_dir.display()
        );
    }

    #[test]
    fn host_user_home_dir_matches_dirs_home_without_flatpak() {
        let mut env = crate::test_support::test_env();
        env.remove("FLATPAK_ID");
        assert_eq!(host_user_home_dir(), dirs::home_dir());
    }
}
