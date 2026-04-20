// SPDX-License-Identifier: MPL-2.0

use crate::error::ConfigError;
use crate::model::ProviderId;
use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use dirs::{cache_dir, state_dir};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const CURSOR_BROWSER_ENV: &str = "YAPCAP_CURSOR_BROWSER";

#[derive(Debug, Clone, CosmicConfigEntry, Eq, PartialEq)]
#[version = 1]
pub struct Config {
    pub refresh_interval_seconds: u64,
    pub codex_enabled: bool,
    pub claude_enabled: bool,
    pub cursor_enabled: bool,
    pub cursor_browser: Browser,
    pub cursor_profile_id: Option<String>,
    pub log_level: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            refresh_interval_seconds: 60,
            codex_enabled: true,
            claude_enabled: true,
            cursor_enabled: true,
            cursor_browser: Browser::Brave,
            cursor_profile_id: None,
            log_level: "info".to_string(),
        }
    }
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
    pub fn with_env_overrides(mut self) -> Self {
        if let Some(browser) = Browser::from_env(CURSOR_BROWSER_ENV) {
            self.cursor_browser = browser;
        }
        self
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Browser {
    #[default]
    Brave,
    Chrome,
    Chromium,
    Edge,
    Firefox,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserProfile {
    pub browser: Browser,
    pub id: String,
    pub label: String,
    pub cookie_db_path: PathBuf,
}

impl Browser {
    fn from_env(name: &str) -> Option<Self> {
        let raw = std::env::var(name).ok()?;
        Self::parse(&raw)
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "brave" => Some(Self::Brave),
            "chrome" => Some(Self::Chrome),
            "chromium" => Some(Self::Chromium),
            "edge" | "microsoft-edge" => Some(Self::Edge),
            "firefox" => Some(Self::Firefox),
            _ => None,
        }
    }

    pub fn cookie_profiles(self) -> crate::error::Result<Vec<BrowserProfile>> {
        let home = dirs::home_dir().ok_or(ConfigError::MissingHomeDir)?;
        match self {
            Self::Brave => Ok(discover_chromium_profiles(
                self,
                &home.join(".config/BraveSoftware/Brave-Browser"),
            )),
            Self::Chrome => Ok(discover_chromium_profiles(
                self,
                &home.join(".config/google-chrome"),
            )),
            Self::Chromium => Ok(discover_chromium_profiles(
                self,
                &home.join(".config/chromium"),
            )),
            Self::Edge => Ok(discover_chromium_profiles(
                self,
                &home.join(".config/microsoft-edge"),
            )),
            Self::Firefox => discover_firefox_profiles(self, &home),
        }
    }

    #[must_use]
    pub fn profile_by_id(self, profiles: &[BrowserProfile], id: &str) -> Option<BrowserProfile> {
        profiles
            .iter()
            .find(|profile| profile.browser == self && profile.id == id)
            .cloned()
    }

    #[must_use]
    pub fn keyring_application(self) -> Option<&'static str> {
        match self {
            Self::Brave => Some("brave"),
            Self::Chrome => Some("chrome"),
            Self::Chromium => Some("chromium"),
            Self::Edge => Some("Microsoft Edge"),
            Self::Firefox => None,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Brave => "Brave",
            Self::Chrome => "Chrome",
            Self::Chromium => "Chromium",
            Self::Edge => "Edge",
            Self::Firefox => "Firefox",
        }
    }

    #[must_use]
    fn chromium_root_name(self) -> Option<&'static str> {
        match self {
            Self::Brave => Some("Brave"),
            Self::Chrome => Some("Chrome"),
            Self::Chromium => Some("Chromium"),
            Self::Edge => Some("Edge"),
            Self::Firefox => None,
        }
    }

    #[must_use]
    fn profile(
        self,
        id: impl Into<String>,
        label: impl Into<String>,
        cookie_db_path: PathBuf,
    ) -> BrowserProfile {
        BrowserProfile {
            browser: self,
            id: id.into(),
            label: label.into(),
            cookie_db_path,
        }
    }
}

fn discover_chromium_profiles(browser: Browser, root: &Path) -> Vec<BrowserProfile> {
    let mut profiles = Vec::new();
    let root_name = browser
        .chromium_root_name()
        .unwrap_or_else(|| browser.label());
    let default = root.join("Default").join("Cookies");
    if default.exists() {
        profiles.push(browser.profile("Default", format!("{root_name} Default"), default));
    }

    let Ok(entries) = fs::read_dir(root) else {
        return profiles;
    };

    let mut discovered = entries
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            if !file_type.is_dir() {
                return None;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            if id == "Default" {
                return None;
            }
            let cookie_db_path = entry.path().join("Cookies");
            if !cookie_db_path.exists() {
                return None;
            }
            Some(browser.profile(id.clone(), format!("{root_name} {id}"), cookie_db_path))
        })
        .collect::<Vec<_>>();
    discovered.sort_by(|left, right| left.id.cmp(&right.id));
    profiles.extend(discovered);
    profiles
}

fn discover_firefox_profiles(
    browser: Browser,
    home: &Path,
) -> crate::error::Result<Vec<BrowserProfile>> {
    let candidates = [
        home.join(".mozilla/firefox"),
        home.join(".config/mozilla/firefox"),
    ];
    let root_names = ["Firefox", "Firefox (XDG)"];
    for (firefox_dir, root_name) in candidates.iter().zip(root_names) {
        let profiles_ini = firefox_dir.join("profiles.ini");
        if !profiles_ini.exists() {
            continue;
        }
        let content =
            fs::read_to_string(&profiles_ini).map_err(|source| ConfigError::ReadConfigFile {
                path: profiles_ini.clone(),
                source,
            })?;
        let profiles = parse_firefox_profiles(browser, &content, firefox_dir, root_name);
        if !profiles.is_empty() {
            return Ok(profiles);
        }
    }
    Err(ConfigError::FirefoxProfileNotFound)
}

fn parse_firefox_profiles(
    browser: Browser,
    ini: &str,
    firefox_dir: &Path,
    root_name: &str,
) -> Vec<BrowserProfile> {
    let mut in_install_section = false;
    let mut install_default: Option<String> = None;
    let mut parsed = Vec::new();
    let mut current_path: Option<String> = None;
    let mut is_profile_default = false;
    let mut profile_default: Option<String> = None;

    for line in ini.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            if let Some(path) = current_path.take() {
                if is_profile_default {
                    profile_default = Some(path.clone());
                }
                parsed.push(path);
            }
            is_profile_default = false;
            in_install_section = line.starts_with("[Install");
        } else if in_install_section {
            if let Some(rel) = line.strip_prefix("Default=") {
                install_default = Some(rel.to_string());
            }
        } else if let Some(rel) = line.strip_prefix("Path=") {
            current_path = Some(rel.to_string());
        } else if line == "Default=1" {
            is_profile_default = true;
        }
    }
    if let Some(path) = current_path {
        if is_profile_default {
            profile_default = Some(path.clone());
        }
        parsed.push(path);
    }

    let mut ordered = Vec::new();
    push_unique(&mut ordered, install_default);
    push_unique(&mut ordered, profile_default);
    for path in parsed {
        push_unique(&mut ordered, Some(path));
    }

    ordered
        .into_iter()
        .filter_map(|id| {
            let cookie_db_path = firefox_dir.join(&id).join("cookies.sqlite");
            if !cookie_db_path.exists() {
                return None;
            }
            Some(browser.profile(id.clone(), format!("{root_name} {id}"), cookie_db_path))
        })
        .collect()
}

fn push_unique(values: &mut Vec<String>, value: Option<String>) {
    if let Some(value) = value
        && !values.contains(&value)
    {
        values.push(value);
    }
}

pub struct AppPaths {
    pub cache_dir: PathBuf,
    pub snapshot_file: PathBuf,
    pub log_dir: PathBuf,
}

#[must_use]
pub fn paths() -> AppPaths {
    let cache_root = cache_dir().unwrap_or_else(|| PathBuf::from("."));
    let state_root = state_dir().unwrap_or_else(|| PathBuf::from("."));
    let cache_dir = cache_root.join("yapcap");
    let state_dir = state_root.join("yapcap");
    let log_dir = state_dir.join("logs");
    AppPaths {
        snapshot_file: cache_dir.join("snapshots.json"),
        cache_dir,
        log_dir,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("yapcap-{name}-{nanos}"))
    }

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }

    #[test]
    fn default_config_enables_all_providers() {
        let config = Config::default();
        assert!(config.provider_enabled(ProviderId::Codex));
        assert!(config.provider_enabled(ProviderId::Claude));
        assert!(config.provider_enabled(ProviderId::Cursor));
        assert_eq!(config.refresh_interval_seconds, 60);
        assert_eq!(config.cursor_profile_id, None);
    }

    #[test]
    fn cursor_browser_default_is_brave() {
        let config = Config::default();
        assert_eq!(config.cursor_browser.label(), "Brave");
    }

    #[test]
    fn cursor_browser_env_override() {
        unsafe {
            std::env::set_var(CURSOR_BROWSER_ENV, "chromium");
        }

        let config = Config::default().with_env_overrides();

        unsafe {
            std::env::remove_var(CURSOR_BROWSER_ENV);
        }

        assert_eq!(config.cursor_browser, Browser::Chromium);
        assert_eq!(config.cursor_browser.label(), "Chromium");
    }

    #[test]
    fn chromium_browser_metadata_matches_chromium_storage() {
        assert_eq!(Browser::Chromium.keyring_application(), Some("chromium"));
        assert_eq!(Browser::Chromium.label(), "Chromium");
    }

    #[test]
    fn chromium_profiles_prefer_default_then_sorted_profiles() {
        let root = test_dir("chromium-profiles");
        touch(&root.join("Profile 2/Cookies"));
        touch(&root.join("Default/Cookies"));
        touch(&root.join("Profile 1/Cookies"));

        let profiles = discover_chromium_profiles(Browser::Chromium, &root);

        assert_eq!(
            profiles
                .iter()
                .map(|profile| profile.id.as_str())
                .collect::<Vec<_>>(),
            ["Default", "Profile 1", "Profile 2"]
        );
        assert_eq!(
            Browser::Chromium
                .profile_by_id(&profiles, "Profile 1")
                .map(|profile| profile.cookie_db_path),
            Some(root.join("Profile 1/Cookies"))
        );
    }

    #[test]
    fn firefox_install_default_takes_precedence() {
        let firefox_dir = test_dir("firefox-install-default");
        touch(&firefox_dir.join("profile.default-release/cookies.sqlite"));
        touch(&firefox_dir.join("install.default-release/cookies.sqlite"));
        let ini = r"
[Profile0]
Name=default-release
Path=profile.default-release
Default=1

[Install4F96D1932A9F858E]
Default=install.default-release
Locked=1
";

        assert_eq!(
            parse_firefox_profiles(Browser::Firefox, ini, &firefox_dir, "Firefox")
                .into_iter()
                .map(|profile| profile.id)
                .collect::<Vec<_>>(),
            ["install.default-release", "profile.default-release"]
        );
    }

    #[test]
    fn firefox_profile_default_is_used_without_install_default() {
        let firefox_dir = test_dir("firefox-profile-default");
        touch(&firefox_dir.join("profile.first/cookies.sqlite"));
        touch(&firefox_dir.join("profile.default-release/cookies.sqlite"));
        let ini = r"
[Profile0]
Name=first
Path=profile.first

[Profile1]
Name=default-release
Path=profile.default-release
Default=1
";

        assert_eq!(
            parse_firefox_profiles(Browser::Firefox, ini, &firefox_dir, "Firefox")
                .into_iter()
                .map(|profile| profile.id)
                .collect::<Vec<_>>(),
            ["profile.default-release", "profile.first"]
        );
    }
}
