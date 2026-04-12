use crate::error::{ConfigError, Result};
use crate::model::ProviderId;
use dirs::{cache_dir, config_dir, state_dir};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub refresh_interval_seconds: u64,
    pub codex_enabled: bool,
    pub claude_enabled: bool,
    pub cursor_enabled: bool,
    pub cursor_browser: CursorBrowser,
    pub log_level: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            refresh_interval_seconds: 60,
            codex_enabled: true,
            claude_enabled: true,
            cursor_enabled: true,
            cursor_browser: CursorBrowser::Brave,
            log_level: "info".to_string(),
        }
    }
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let path = paths().config_file;
        if !path.exists() {
            let config = Self::default();
            config.save()?;
            return Ok(config);
        }
        let raw = fs::read_to_string(&path).map_err(|source| ConfigError::ReadConfigFile {
            path: path.clone(),
            source,
        })?;
        Ok(toml::from_str(&raw).map_err(ConfigError::ParseConfig)?)
    }

    pub fn save(&self) -> Result<()> {
        let paths = paths();
        fs::create_dir_all(&paths.config_dir).map_err(|source| ConfigError::CreateConfigDir {
            path: paths.config_dir.clone(),
            source,
        })?;
        let content = toml::to_string_pretty(self).map_err(ConfigError::EncodeConfig)?;
        fs::write(&paths.config_file, content).map_err(|source| ConfigError::WriteConfig {
            path: paths.config_file.clone(),
            source,
        })?;
        Ok(())
    }

    pub fn provider_enabled(&self, provider: ProviderId) -> bool {
        match provider {
            ProviderId::Codex => self.codex_enabled,
            ProviderId::Claude => self.claude_enabled,
            ProviderId::Cursor => self.cursor_enabled,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorBrowser {
    Brave,
}

impl CursorBrowser {
    pub fn cookie_db_path(self) -> Result<PathBuf> {
        let home = dirs::home_dir().ok_or(ConfigError::MissingHomeDir)?;
        Ok(match self {
            Self::Brave => home.join(".config/BraveSoftware/Brave-Browser/Default/Cookies"),
        })
    }

    pub fn keyring_application(self) -> &'static str {
        match self {
            Self::Brave => "brave",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Brave => "Brave",
        }
    }
}

pub struct AppPaths {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub cache_dir: PathBuf,
    pub snapshot_file: PathBuf,
    pub state_dir: PathBuf,
    pub log_dir: PathBuf,
}

pub fn paths() -> AppPaths {
    let config_root = config_dir().unwrap_or_else(|| PathBuf::from("."));
    let cache_root = cache_dir().unwrap_or_else(|| PathBuf::from("."));
    let state_root = state_dir().unwrap_or_else(|| PathBuf::from("."));
    let config_dir = config_root.join("yapcap");
    let cache_dir = cache_root.join("yapcap");
    let state_dir = state_root.join("yapcap");
    let log_dir = state_dir.join("logs");
    AppPaths {
        config_file: config_dir.join("config.toml"),
        snapshot_file: cache_dir.join("snapshots.json"),
        config_dir,
        cache_dir,
        state_dir,
        log_dir,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_enables_all_providers() {
        let config = AppConfig::default();
        assert!(config.provider_enabled(ProviderId::Codex));
        assert!(config.provider_enabled(ProviderId::Claude));
        assert!(config.provider_enabled(ProviderId::Cursor));
        assert_eq!(config.refresh_interval_seconds, 60);
    }
}
