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
    Chrome,
    Edge,
    Firefox,
}

impl CursorBrowser {
    pub fn cookie_db_path(self) -> Result<PathBuf> {
        let home = dirs::home_dir().ok_or(ConfigError::MissingHomeDir)?;
        Ok(match self {
            Self::Brave => home.join(".config/BraveSoftware/Brave-Browser/Default/Cookies"),
            Self::Chrome => home.join(".config/google-chrome/Default/Cookies"),
            Self::Edge => home.join(".config/microsoft-edge/Default/Cookies"),
            Self::Firefox => find_firefox_cookie_db(&home)?,
        })
    }

    /// Keyring application name used to look up the Safe Storage secret.
    /// Returns `None` for browsers that don't use a keyring (Firefox).
    pub fn keyring_application(self) -> Option<&'static str> {
        match self {
            Self::Brave => Some("brave"),
            Self::Chrome => Some("chrome"),
            Self::Edge => Some("Microsoft Edge"),
            Self::Firefox => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Brave => "Brave",
            Self::Chrome => "Chrome",
            Self::Edge => "Edge",
            Self::Firefox => "Firefox",
        }
    }
}

fn find_firefox_cookie_db(home: &std::path::Path) -> Result<PathBuf> {
    // Firefox uses ~/.mozilla/firefox/ traditionally, but XDG-compliant installs
    // (e.g. on newer distros or Flatpak) place it under ~/.config/mozilla/firefox/.
    let candidates = [
        home.join(".mozilla/firefox"),
        home.join(".config/mozilla/firefox"),
    ];
    for firefox_dir in &candidates {
        let profiles_ini = firefox_dir.join("profiles.ini");
        if !profiles_ini.exists() {
            continue;
        }
        let content =
            fs::read_to_string(&profiles_ini).map_err(|source| ConfigError::ReadConfigFile {
                path: profiles_ini.clone(),
                source,
            })?;
        if let Some(path) = parse_firefox_profile_cookie_db(&content, firefox_dir) {
            return Ok(path);
        }
    }
    Err(ConfigError::FirefoxProfileNotFound.into())
}

fn parse_firefox_profile_cookie_db(ini: &str, firefox_dir: &std::path::Path) -> Option<PathBuf> {
    // Modern Firefox writes an [Install<hash>] section whose Default= key is
    // the relative path of the last-used profile. That takes precedence over the
    // legacy Default=1 flag in [Profile...] sections.
    let mut in_install_section = false;
    let mut install_default: Option<PathBuf> = None;
    let mut first_path: Option<PathBuf> = None;
    let mut current_path: Option<PathBuf> = None;
    let mut is_profile_default = false;
    let mut profile_default: Option<PathBuf> = None;

    for line in ini.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            // Flush previous section state.
            if is_profile_default {
                if let Some(dir) = current_path.take() {
                    profile_default = Some(dir.join("cookies.sqlite"));
                }
            }
            current_path = None;
            is_profile_default = false;
            in_install_section = line.starts_with("[Install");
        } else if in_install_section {
            if let Some(rel) = line.strip_prefix("Default=") {
                install_default = Some(firefox_dir.join(rel).join("cookies.sqlite"));
            }
        } else {
            if let Some(rel) = line.strip_prefix("Path=") {
                let profile_dir = firefox_dir.join(rel);
                if first_path.is_none() {
                    first_path = Some(profile_dir.join("cookies.sqlite"));
                }
                current_path = Some(profile_dir);
            } else if line == "Default=1" {
                is_profile_default = true;
            }
        }
    }
    // Flush the last section.
    if is_profile_default {
        if let Some(dir) = current_path {
            profile_default = Some(dir.join("cookies.sqlite"));
        }
    }

    install_default.or(profile_default).or(first_path)
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
