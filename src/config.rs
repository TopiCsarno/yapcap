// SPDX-License-Identifier: MPL-2.0

use crate::model::ProviderId;
use chrono::{DateTime, Utc};
use cosmic::cosmic_config::{
    self, ConfigGet, ConfigSet, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry,
};
use dirs::{cache_dir, state_dir};
use serde::{Deserialize, Deserializer, Serialize};
use std::path::PathBuf;

mod watch_update;

pub const APP_ID: &str = "io.github.TopiCsarno.YapCap";

#[derive(Debug, Clone, CosmicConfigEntry, Serialize, Deserialize, Eq, PartialEq)]
#[version = 600]
pub struct Config {
    pub refresh_interval_seconds: u64,
    pub reset_time_format: ResetTimeFormat,
    pub usage_amount_format: UsageAmountFormat,
    pub panel_icon_style: PanelIconStyle,
    #[serde(default = "default_selected_provider")]
    pub selected_provider: ProviderId,
    #[serde(default = "default_provider_visibility_mode")]
    pub provider_visibility_mode: ProviderVisibilityMode,
    #[serde(default)]
    pub codex_enablement: ProviderEnablement,
    #[serde(default)]
    pub claude_enablement: ProviderEnablement,
    #[serde(default)]
    pub cursor_enablement: ProviderEnablement,
    #[serde(default = "default_gemini_enablement")]
    pub gemini_enablement: ProviderEnablement,
    #[serde(default)]
    pub copilot_enablement: ProviderEnablement,
    #[serde(default)]
    pub minimax_enablement: ProviderEnablement,
    #[serde(default)]
    pub kimi_enablement: ProviderEnablement,
    #[serde(default)]
    pub antigravity_enablement: ProviderEnablement,
    #[serde(default)]
    pub opencode_go_enablement: ProviderEnablement,
    #[serde(default)]
    pub grok_enablement: ProviderEnablement,
    pub selected_codex_account_ids: Vec<String>,
    pub codex_managed_accounts: Vec<ManagedCodexAccountConfig>,
    pub selected_claude_account_ids: Vec<String>,
    pub claude_managed_accounts: Vec<ManagedClaudeAccountConfig>,
    pub selected_cursor_account_ids: Vec<String>,
    pub cursor_managed_accounts: Vec<ManagedCursorAccountConfig>,
    #[serde(default)]
    pub selected_gemini_account_ids: Vec<String>,
    #[serde(default)]
    pub gemini_managed_accounts: Vec<ManagedGeminiAccountConfig>,
    #[serde(default)]
    pub selected_copilot_account_ids: Vec<String>,
    #[serde(default)]
    pub copilot_managed_accounts: Vec<ManagedCopilotAccountConfig>,
    #[serde(default)]
    pub selected_minimax_account_ids: Vec<String>,
    #[serde(default)]
    pub minimax_managed_accounts: Vec<ManagedMinimaxAccountConfig>,
    #[serde(default)]
    pub selected_kimi_account_ids: Vec<String>,
    #[serde(default)]
    pub kimi_managed_accounts: Vec<ManagedKimiAccountConfig>,
    #[serde(default)]
    pub selected_antigravity_account_ids: Vec<String>,
    #[serde(default)]
    pub antigravity_managed_accounts: Vec<ManagedAntigravityAccountConfig>,
    #[serde(default)]
    pub selected_opencode_go_account_ids: Vec<String>,
    #[serde(default)]
    pub opencode_go_managed_accounts: Vec<ManagedOpenCodeGoAccountConfig>,
    #[serde(default)]
    pub selected_grok_account_ids: Vec<String>,
    #[serde(default)]
    pub grok_managed_accounts: Vec<ManagedGrokAccountConfig>,
    pub log_level: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            refresh_interval_seconds: 300,
            reset_time_format: ResetTimeFormat::Relative,
            usage_amount_format: UsageAmountFormat::Used,
            panel_icon_style: PanelIconStyle::LogoAndBars,
            selected_provider: ProviderId::Codex,
            provider_visibility_mode: ProviderVisibilityMode::UserManaged,
            codex_enablement: ProviderEnablement::Auto,
            claude_enablement: ProviderEnablement::Auto,
            cursor_enablement: ProviderEnablement::Auto,
            gemini_enablement: default_gemini_enablement(),
            copilot_enablement: ProviderEnablement::Auto,
            minimax_enablement: ProviderEnablement::Auto,
            kimi_enablement: ProviderEnablement::Auto,
            antigravity_enablement: ProviderEnablement::Auto,
            opencode_go_enablement: ProviderEnablement::Auto,
            grok_enablement: ProviderEnablement::Auto,
            selected_codex_account_ids: Vec::new(),
            codex_managed_accounts: Vec::new(),
            selected_claude_account_ids: Vec::new(),
            claude_managed_accounts: Vec::new(),
            selected_cursor_account_ids: Vec::new(),
            cursor_managed_accounts: Vec::new(),
            selected_gemini_account_ids: Vec::new(),
            gemini_managed_accounts: Vec::new(),
            selected_copilot_account_ids: Vec::new(),
            copilot_managed_accounts: Vec::new(),
            selected_minimax_account_ids: Vec::new(),
            minimax_managed_accounts: Vec::new(),
            selected_kimi_account_ids: Vec::new(),
            kimi_managed_accounts: Vec::new(),
            selected_antigravity_account_ids: Vec::new(),
            antigravity_managed_accounts: Vec::new(),
            selected_opencode_go_account_ids: Vec::new(),
            opencode_go_managed_accounts: Vec::new(),
            selected_grok_account_ids: Vec::new(),
            grok_managed_accounts: Vec::new(),
            log_level: "info".to_string(),
        }
    }
}

fn default_provider_visibility_mode() -> ProviderVisibilityMode {
    ProviderVisibilityMode::UserManaged
}

fn default_selected_provider() -> ProviderId {
    ProviderId::Codex
}

fn default_gemini_enablement() -> ProviderEnablement {
    ProviderEnablement::Disabled
}

impl Config {
    #[must_use]
    pub fn provider_enablement(&self, provider: ProviderId) -> ProviderEnablement {
        match provider {
            ProviderId::Codex => self.codex_enablement,
            ProviderId::Claude => self.claude_enablement,
            ProviderId::Cursor => self.cursor_enablement,
            ProviderId::Gemini => self.gemini_enablement,
            ProviderId::Copilot => self.copilot_enablement,
            ProviderId::Minimax => self.minimax_enablement,
            ProviderId::Kimi => self.kimi_enablement,
            ProviderId::Antigravity => self.antigravity_enablement,
            ProviderId::OpenCodeGo => self.opencode_go_enablement,
            ProviderId::Grok => self.grok_enablement,
        }
    }

    #[must_use]
    pub fn selected_account_ids(&self, provider: ProviderId) -> &[String] {
        match provider {
            ProviderId::Codex => &self.selected_codex_account_ids,
            ProviderId::Claude => &self.selected_claude_account_ids,
            ProviderId::Cursor => &self.selected_cursor_account_ids,
            ProviderId::Gemini => &self.selected_gemini_account_ids,
            ProviderId::Copilot => &self.selected_copilot_account_ids,
            ProviderId::Minimax => &self.selected_minimax_account_ids,
            ProviderId::Kimi => &self.selected_kimi_account_ids,
            ProviderId::Antigravity => &self.selected_antigravity_account_ids,
            ProviderId::OpenCodeGo => &self.selected_opencode_go_account_ids,
            ProviderId::Grok => &self.selected_grok_account_ids,
        }
    }

    pub fn selected_account_ids_mut(&mut self, provider: ProviderId) -> &mut Vec<String> {
        match provider {
            ProviderId::Codex => &mut self.selected_codex_account_ids,
            ProviderId::Claude => &mut self.selected_claude_account_ids,
            ProviderId::Cursor => &mut self.selected_cursor_account_ids,
            ProviderId::Gemini => &mut self.selected_gemini_account_ids,
            ProviderId::Copilot => &mut self.selected_copilot_account_ids,
            ProviderId::Minimax => &mut self.selected_minimax_account_ids,
            ProviderId::Kimi => &mut self.selected_kimi_account_ids,
            ProviderId::Antigravity => &mut self.selected_antigravity_account_ids,
            ProviderId::OpenCodeGo => &mut self.selected_opencode_go_account_ids,
            ProviderId::Grok => &mut self.selected_grok_account_ids,
        }
    }

    pub fn set_provider_enabled(&mut self, provider: ProviderId, enabled: bool) -> bool {
        let enablement = provider_enablement_mut(self, provider);
        let explicit = if enabled {
            ProviderEnablement::Enabled
        } else {
            ProviderEnablement::Disabled
        };
        let changed = *enablement != explicit;
        *enablement = explicit;
        changed
    }
}

pub fn migrate_provider_enablement(context: &cosmic_config::Config, config: &mut Config) -> bool {
    let mut migrated = false;
    for provider in ProviderId::ALL {
        let enablement_key = provider_enablement_key(provider);
        if !matches!(
            context.get::<ProviderEnablement>(enablement_key),
            Err(cosmic_config::Error::NotFound | cosmic_config::Error::NoConfigDirectory)
        ) {
            continue;
        }
        let Ok(enabled) = context.get::<bool>(provider_enabled_key(provider)) else {
            continue;
        };
        let enablement = if enabled {
            ProviderEnablement::Enabled
        } else {
            ProviderEnablement::Disabled
        };
        if context.set(enablement_key, enablement).is_ok() {
            *provider_enablement_mut(config, provider) = enablement;
            migrated = true;
        }
    }
    migrated
}

fn provider_enabled_key(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Codex => "codex_enabled",
        ProviderId::Claude => "claude_enabled",
        ProviderId::Cursor => "cursor_enabled",
        ProviderId::Gemini => "gemini_enabled",
        ProviderId::Copilot => "copilot_enabled",
        ProviderId::Minimax => "minimax_enabled",
        ProviderId::Kimi => "kimi_enabled",
        ProviderId::Antigravity => "antigravity_enabled",
        ProviderId::OpenCodeGo => "opencode_go_enabled",
        ProviderId::Grok => "grok_enabled",
    }
}

fn provider_enablement_key(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Codex => "codex_enablement",
        ProviderId::Claude => "claude_enablement",
        ProviderId::Cursor => "cursor_enablement",
        ProviderId::Gemini => "gemini_enablement",
        ProviderId::Copilot => "copilot_enablement",
        ProviderId::Minimax => "minimax_enablement",
        ProviderId::Kimi => "kimi_enablement",
        ProviderId::Antigravity => "antigravity_enablement",
        ProviderId::OpenCodeGo => "opencode_go_enablement",
        ProviderId::Grok => "grok_enablement",
    }
}

fn provider_enablement_mut(config: &mut Config, provider: ProviderId) -> &mut ProviderEnablement {
    match provider {
        ProviderId::Codex => &mut config.codex_enablement,
        ProviderId::Claude => &mut config.claude_enablement,
        ProviderId::Cursor => &mut config.cursor_enablement,
        ProviderId::Gemini => &mut config.gemini_enablement,
        ProviderId::Copilot => &mut config.copilot_enablement,
        ProviderId::Minimax => &mut config.minimax_enablement,
        ProviderId::Kimi => &mut config.kimi_enablement,
        ProviderId::Antigravity => &mut config.antigravity_enablement,
        ProviderId::OpenCodeGo => &mut config.opencode_go_enablement,
        ProviderId::Grok => &mut config.grok_enablement,
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
pub enum ProviderEnablement {
    #[default]
    Auto,
    Enabled,
    Disabled,
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
pub struct ManagedGeminiAccountConfig {
    pub id: String,
    pub label: String,
    pub account_root: PathBuf,
    pub email: String,
    pub sub: String,
    #[serde(default)]
    pub hd: Option<String>,
    #[serde(default)]
    pub last_tier_id: Option<String>,
    #[serde(default)]
    pub last_cloudaicompanion_project: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_authenticated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedAntigravityAccountConfig {
    pub id: String,
    pub label: String,
    pub account_root: PathBuf,
    pub email: String,
    pub sub: String,
    #[serde(default)]
    pub last_tier_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_authenticated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedCopilotAccountConfig {
    pub id: String,
    pub label: String,
    pub github_user_id: u64,
    pub login: String,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedMinimaxAccountConfig {
    pub id: String,
    pub label: String,
    pub api_key_source: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_authenticated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedKimiAccountConfig {
    pub id: String,
    pub label: String,
    pub api_key_source: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_authenticated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedOpenCodeGoAccountConfig {
    pub id: String,
    pub label: String,
    pub api_key_source: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_authenticated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedGrokAccountConfig {
    pub id: String,
    pub label: String,
    pub config_dir: PathBuf,
    pub email: Option<String>,
    pub provider_account_id: Option<String>,
    pub team_id: Option<String>,
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
    pub state_dir: PathBuf,
    pub codex_accounts_dir: PathBuf,
    pub claude_accounts_dir: PathBuf,
    pub cursor_accounts_dir: PathBuf,
    pub gemini_accounts_dir: PathBuf,
    pub copilot_accounts_dir: PathBuf,
    pub minimax_accounts_dir: PathBuf,
    pub kimi_accounts_dir: PathBuf,
    pub antigravity_accounts_dir: PathBuf,
    pub opencode_go_accounts_dir: PathBuf,
    #[allow(dead_code)]
    pub grok_accounts_dir: PathBuf,
    pub log_dir: PathBuf,
}

#[cfg(not(test))]
pub fn cosmic_config_context(
    app_id: &str,
    version: u64,
) -> Result<cosmic_config::Config, cosmic_config::Error> {
    cosmic_config::Config::new(app_id, version)
}

#[cfg(test)]
pub fn cosmic_config_context(
    app_id: &str,
    version: u64,
) -> Result<cosmic_config::Config, cosmic_config::Error> {
    cosmic_config::Config::with_custom_path(app_id, version, test_config_root())
}

pub fn write_changed_config_entries(
    context: &cosmic_config::Config,
    old: &Config,
    new: &Config,
) -> Result<(), cosmic_config::Error> {
    let tx = context.transaction();
    let Config {
        refresh_interval_seconds,
        reset_time_format,
        usage_amount_format,
        panel_icon_style,
        selected_provider,
        provider_visibility_mode,
        codex_enablement,
        claude_enablement,
        cursor_enablement,
        gemini_enablement,
        copilot_enablement,
        minimax_enablement,
        kimi_enablement,
        antigravity_enablement,
        opencode_go_enablement,
        selected_codex_account_ids,
        codex_managed_accounts,
        selected_claude_account_ids,
        claude_managed_accounts,
        selected_cursor_account_ids,
        cursor_managed_accounts,
        selected_gemini_account_ids,
        gemini_managed_accounts,
        selected_copilot_account_ids,
        copilot_managed_accounts,
        selected_minimax_account_ids,
        minimax_managed_accounts,
        selected_kimi_account_ids,
        kimi_managed_accounts,
        selected_antigravity_account_ids,
        antigravity_managed_accounts,
        selected_opencode_go_account_ids,
        opencode_go_managed_accounts,
        grok_enablement,
        selected_grok_account_ids,
        grok_managed_accounts,
        log_level,
    } = new;

    macro_rules! set_changed {
        ($field:ident) => {
            if old.$field != *$field {
                ConfigSet::set(&tx, stringify!($field), $field)?;
            }
        };
    }

    set_changed!(refresh_interval_seconds);
    set_changed!(reset_time_format);
    set_changed!(usage_amount_format);
    set_changed!(panel_icon_style);
    set_changed!(selected_provider);
    set_changed!(provider_visibility_mode);
    set_changed!(codex_enablement);
    set_changed!(claude_enablement);
    set_changed!(cursor_enablement);
    set_changed!(gemini_enablement);
    set_changed!(copilot_enablement);
    set_changed!(minimax_enablement);
    set_changed!(kimi_enablement);
    set_changed!(antigravity_enablement);
    set_changed!(opencode_go_enablement);
    set_changed!(grok_enablement);
    set_changed!(selected_codex_account_ids);
    set_changed!(codex_managed_accounts);
    set_changed!(selected_claude_account_ids);
    set_changed!(claude_managed_accounts);
    set_changed!(selected_cursor_account_ids);
    set_changed!(cursor_managed_accounts);
    set_changed!(selected_gemini_account_ids);
    set_changed!(gemini_managed_accounts);
    set_changed!(selected_copilot_account_ids);
    set_changed!(copilot_managed_accounts);
    set_changed!(selected_minimax_account_ids);
    set_changed!(minimax_managed_accounts);
    set_changed!(selected_kimi_account_ids);
    set_changed!(kimi_managed_accounts);
    set_changed!(selected_antigravity_account_ids);
    set_changed!(antigravity_managed_accounts);
    set_changed!(selected_opencode_go_account_ids);
    set_changed!(opencode_go_managed_accounts);
    set_changed!(selected_grok_account_ids);
    set_changed!(grok_managed_accounts);
    set_changed!(log_level);

    tx.commit()
}

#[cfg(test)]
fn test_config_root() -> PathBuf {
    thread_local! {
        static ROOT: tempfile::TempDir =
            tempfile::tempdir().expect("create per-test cosmic config root");
    }
    ROOT.with(|root| root.path().to_path_buf())
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
pub fn managed_gemini_account_dir(account_id: &str) -> PathBuf {
    paths().gemini_accounts_dir.join(account_id)
}

#[must_use]
pub fn managed_antigravity_account_dir(account_id: &str) -> PathBuf {
    paths().antigravity_accounts_dir.join(account_id)
}

#[must_use]
#[allow(dead_code)]
pub fn managed_grok_account_dir(account_id: &str) -> PathBuf {
    paths().grok_accounts_dir.join(account_id)
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
    let gemini_accounts_dir = state_dir.join("gemini-accounts");
    let copilot_accounts_dir = state_dir.join("copilot-accounts");
    let minimax_accounts_dir = state_dir.join("minimax-accounts");
    let kimi_accounts_dir = state_dir.join("kimi-accounts");
    let antigravity_accounts_dir = state_dir.join("antigravity-accounts");
    let opencode_go_accounts_dir = state_dir.join("opencode-go-accounts");
    let grok_accounts_dir = state_dir.join("grok-accounts");
    let log_dir = state_dir.join("logs");
    AppPaths {
        cache_dir,
        state_dir,
        codex_accounts_dir,
        claude_accounts_dir,
        cursor_accounts_dir,
        gemini_accounts_dir,
        copilot_accounts_dir,
        minimax_accounts_dir,
        kimi_accounts_dir,
        antigravity_accounts_dir,
        opencode_go_accounts_dir,
        grok_accounts_dir,
        log_dir,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic::cosmic_config::{ConfigGet, ConfigSet};

    #[test]
    fn provider_enablement_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&ProviderEnablement::Auto).unwrap(),
            "\"auto\""
        );
        assert_eq!(
            serde_json::to_string(&ProviderEnablement::Enabled).unwrap(),
            "\"enabled\""
        );
        assert_eq!(
            serde_json::from_str::<ProviderEnablement>("\"disabled\"").unwrap(),
            ProviderEnablement::Disabled
        );
    }

    #[test]
    fn provider_enablement_migrates_legacy_bools() {
        let ctx = cosmic_config_context(APP_ID, Config::VERSION).unwrap();
        ctx.set("codex_enabled", true).unwrap();
        ctx.set("claude_enabled", false).unwrap();
        let mut config = Config::default();

        assert!(migrate_provider_enablement(&ctx, &mut config));
        assert_eq!(config.codex_enablement, ProviderEnablement::Enabled);
        assert_eq!(config.claude_enablement, ProviderEnablement::Disabled);
        assert_eq!(
            ctx.get::<ProviderEnablement>("codex_enablement").unwrap(),
            ProviderEnablement::Enabled
        );
        assert!(ctx.get::<bool>("codex_enabled").unwrap());
    }

    #[test]
    fn provider_enablement_migration_leaves_fresh_config_on_auto() {
        let ctx = cosmic_config_context(APP_ID, Config::VERSION).unwrap();
        let mut config = Config::default();

        assert!(!migrate_provider_enablement(&ctx, &mut config));
        assert_eq!(config.codex_enablement, ProviderEnablement::Auto);
        assert!(ctx.get::<ProviderEnablement>("codex_enablement").is_err());
    }

    #[test]
    fn provider_enablement_migration_is_idempotent() {
        let ctx = cosmic_config_context(APP_ID, Config::VERSION).unwrap();
        ctx.set("cursor_enabled", false).unwrap();
        let mut config = Config::default();

        assert!(migrate_provider_enablement(&ctx, &mut config));
        config.cursor_enablement = ProviderEnablement::Auto;

        assert!(!migrate_provider_enablement(&ctx, &mut config));
        assert_eq!(config.cursor_enablement, ProviderEnablement::Auto);
    }

    #[test]
    fn write_changed_config_entries_persists_only_changed_fields() {
        let ctx = cosmic_config_context(APP_ID, Config::VERSION).unwrap();
        let old = Config::default();
        old.write_entry(&ctx).unwrap();

        let mut new = old.clone();
        new.panel_icon_style = match old.panel_icon_style {
            PanelIconStyle::PercentOnly => PanelIconStyle::LogoAndBars,
            _ => PanelIconStyle::PercentOnly,
        };
        new.refresh_interval_seconds = old.refresh_interval_seconds + 60;
        new.kimi_enablement = ProviderEnablement::Enabled;
        new.selected_kimi_account_ids = vec!["kimi-test".to_string()];

        write_changed_config_entries(&ctx, &old, &new).unwrap();

        let reloaded = Config::get_entry(&ctx).expect("reload config");
        assert_eq!(reloaded, new);
    }

    #[test]
    fn write_changed_config_entries_noop_when_equal() {
        let ctx = cosmic_config_context(APP_ID, Config::VERSION).unwrap();
        let config = Config::default();
        config.write_entry(&ctx).unwrap();

        write_changed_config_entries(&ctx, &config, &config).unwrap();

        let reloaded = Config::get_entry(&ctx).expect("reload config");
        assert_eq!(reloaded, config);
    }

    #[test]
    fn set_provider_enabled_writes_explicit_enablement() {
        let mut config = Config::default();

        assert!(config.set_provider_enabled(ProviderId::Gemini, true));
        assert_eq!(config.gemini_enablement, ProviderEnablement::Enabled);
        assert!(config.set_provider_enabled(ProviderId::Gemini, false));
        assert_eq!(config.gemini_enablement, ProviderEnablement::Disabled);
    }

    #[test]
    fn default_config_requires_manual_gemini_enablement() {
        let config = Config::default();
        assert_eq!(
            config.provider_visibility_mode,
            ProviderVisibilityMode::UserManaged
        );
        assert_eq!(config.refresh_interval_seconds, 300);
        assert_eq!(config.reset_time_format, ResetTimeFormat::Relative);
        assert_eq!(config.usage_amount_format, UsageAmountFormat::Used);
        assert_eq!(config.panel_icon_style, PanelIconStyle::LogoAndBars);
        assert_eq!(config.selected_provider, ProviderId::Codex);
        assert_eq!(config.codex_enablement, ProviderEnablement::Auto);
        assert_eq!(config.gemini_enablement, ProviderEnablement::Disabled);
    }

    #[test]
    fn missing_gemini_enablement_defaults_to_disabled() {
        let mut value = serde_json::to_value(Config::default()).unwrap();
        value.as_object_mut().unwrap().remove("gemini_enablement");

        let config: Config = serde_json::from_value(value).unwrap();

        assert_eq!(config.gemini_enablement, ProviderEnablement::Disabled);
    }

    #[test]
    fn config_schema_version_marks_fresh_patch_boundary() {
        let config = Config::default();
        assert_eq!(Config::VERSION, 600);
        assert!(config.codex_managed_accounts.is_empty());
        assert!(config.claude_managed_accounts.is_empty());
        assert!(config.cursor_managed_accounts.is_empty());
        assert!(config.gemini_managed_accounts.is_empty());
        assert!(config.copilot_managed_accounts.is_empty());
        assert!(config.minimax_managed_accounts.is_empty());
        assert!(config.kimi_managed_accounts.is_empty());
        assert!(config.antigravity_managed_accounts.is_empty());
        assert!(config.grok_managed_accounts.is_empty());
    }

    #[test]
    fn copilot_account_config_does_not_serialize_account_root() {
        let now = Utc::now();
        let account = ManagedCopilotAccountConfig {
            id: "copilot-42".to_string(),
            label: "octocat".to_string(),
            github_user_id: 42,
            login: "octocat".to_string(),
            created_at: now,
            updated_at: now,
            last_authenticated_at: Some(now),
        };

        let value = serde_json::to_value(account).unwrap();

        assert!(value.get("account_root").is_none());
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
    fn missing_selected_provider_defaults_to_codex() {
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
                "selected_codex_account_ids": [],
                "codex_managed_accounts": [],
                "selected_claude_account_ids": [],
                "claude_managed_accounts": [],
                "selected_cursor_account_ids": [],
                "cursor_managed_accounts": [],
                "log_level": "info"
            }"#,
        )
        .unwrap();

        assert_eq!(config.selected_provider, ProviderId::Codex);
    }

    #[test]
    fn selected_provider_serializes_as_snake_case() {
        let config = Config {
            selected_provider: ProviderId::Claude,
            ..Config::default()
        };

        let value = serde_json::to_value(&config).unwrap();
        assert_eq!(value.get("selected_provider").unwrap(), "claude");

        let parsed: Config = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.selected_provider, ProviderId::Claude);
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

    #[test]
    fn grok_accounts_dir_is_configured_under_state_root() {
        let p = paths();
        assert!(
            p.grok_accounts_dir
                .ends_with(std::path::Path::new("yapcap/grok-accounts")),
            "unexpected grok_accounts_dir: {}",
            p.grok_accounts_dir.display()
        );
    }

    #[test]
    fn grok_managed_account_config_roundtrips() {
        let id = "grok-test-1";
        let dir = managed_grok_account_dir(id);
        assert_eq!(dir, paths().grok_accounts_dir.join(id));

        let now = Utc::now();
        let account = ManagedGrokAccountConfig {
            id: id.to_string(),
            label: "xai-user".to_string(),
            config_dir: dir,
            email: Some("user@example.com".to_string()),
            provider_account_id: Some("user-123".to_string()),
            team_id: Some("team-456".to_string()),
            plan: Some("Premium+".to_string()),
            created_at: now,
            updated_at: now,
            last_authenticated_at: Some(now),
        };
        let serialized = serde_json::to_string(&account).unwrap();
        let deserialized: ManagedGrokAccountConfig = serde_json::from_str(&serialized).unwrap();
        assert_eq!(account, deserialized);
    }
}
