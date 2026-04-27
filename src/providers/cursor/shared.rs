// SPDX-License-Identifier: MPL-2.0

use crate::config::Browser;
use chrono::Utc;
use std::ffi::OsString;

pub fn chromium_browser(preferred: Browser) -> Browser {
    match preferred {
        Browser::Firefox => Browser::Brave,
        other => other,
    }
}

pub fn browser_command(browser: Browser) -> OsString {
    let candidates = match browser {
        Browser::Brave => &["brave", "brave-browser"][..],
        Browser::Chrome => &["google-chrome", "google-chrome-stable", "chrome"][..],
        Browser::Chromium => &["chromium", "chromium-browser"][..],
        Browser::Edge => &["microsoft-edge", "microsoft-edge-stable"][..],
        Browser::Firefox => &["firefox"][..],
    };
    resolve_executable(candidates).unwrap_or_else(|| OsString::from(candidates[0]))
}

fn resolve_executable(candidates: &[&str]) -> Option<OsString> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for candidate in candidates {
            let executable = dir.join(candidate);
            if executable.is_file() {
                return Some(executable.into_os_string());
            }
        }
    }
    None
}

pub fn browser_spawn_error(browser: Browser, error: &std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound {
        format!("{} executable not found", browser.label())
    } else {
        format!("failed to start {}: {error}", browser.label())
    }
}

pub fn new_account_id() -> String {
    let millis = Utc::now().timestamp_millis();
    format!("cursor-{millis}-{}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("yapcap-{name}-{nanos}"))
    }

    #[test]
    fn browser_command_uses_installed_brave_variant() {
        let _guard = test_support::env_lock();
        let bin_dir = test_dir("brave-bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(bin_dir.join("brave-browser"), "").unwrap();
        unsafe {
            std::env::set_var("PATH", &bin_dir);
        }

        let command = browser_command(Browser::Brave);

        unsafe {
            std::env::remove_var("PATH");
        }

        assert_eq!(command, bin_dir.join("brave-browser").into_os_string());
    }

    #[test]
    fn browser_command_falls_back_to_default_name_when_missing() {
        let _guard = test_support::env_lock();
        unsafe {
            std::env::set_var("PATH", "");
        }

        let command = browser_command(Browser::Brave);

        unsafe {
            std::env::remove_var("PATH");
        }

        assert_eq!(command, OsString::from("brave"));
    }
}
