//! GitHub release-based update check.
//!
//! Hits `api.github.com/repos/{repo}/releases/latest`, compares `tag_name`
//! against `CARGO_PKG_VERSION`. Override the URL with `YAPCAP_UPDATE_URL` for
//! testing against a local mock.

use serde::Deserialize;

const DEFAULT_URL: &str = "https://api.github.com/repos/TopiCsarno/yapcap/releases/latest";
const URL_OVERRIDE_ENV: &str = "YAPCAP_UPDATE_URL";
const USER_AGENT: &str = concat!("yapcap/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    Unchecked,
    NoUpdate,
    UpdateAvailable { version: String, url: String },
    Error(String),
}

/// Pure view-model for the About section's update line. Kept free of any UI
/// types so it's unit-testable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateDisplay {
    pub text: String,
    pub link: Option<String>,
}

impl UpdateStatus {
    pub fn describe(&self) -> UpdateDisplay {
        match self {
            UpdateStatus::Unchecked => UpdateDisplay {
                text: "Checking for updates...".to_string(),
                link: None,
            },
            UpdateStatus::NoUpdate => UpdateDisplay {
                text: "Up to date".to_string(),
                link: None,
            },
            UpdateStatus::UpdateAvailable { version, url } => UpdateDisplay {
                text: format!("Update available: v{version}"),
                link: Some(url.clone()),
            },
            UpdateStatus::Error(reason) => UpdateDisplay {
                text: format!("Update check failed: {reason}"),
                link: None,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
}

pub async fn check(client: &reqwest::Client) -> UpdateStatus {
    let url = std::env::var(URL_OVERRIDE_ENV).unwrap_or_else(|_| DEFAULT_URL.to_string());
    match fetch_release(client, &url).await {
        Ok(release) => {
            let current = env!("CARGO_PKG_VERSION");
            if is_newer(&release.tag_name, current) {
                UpdateStatus::UpdateAvailable {
                    version: strip_v(&release.tag_name).to_string(),
                    url: release.html_url,
                }
            } else {
                UpdateStatus::NoUpdate
            }
        }
        Err(error) if error == "no releases" => UpdateStatus::NoUpdate,
        Err(error) => UpdateStatus::Error(error),
    }
}

async fn fetch_release(client: &reqwest::Client, url: &str) -> Result<GithubRelease, String> {
    let response = client
        .get(url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|error| format!("request failed: {error}"))?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        // Repo has no releases yet — treat as "up to date" rather than error.
        return Err("no releases".to_string());
    }
    if !response.status().is_success() {
        return Err(format!("http {}", response.status().as_u16()));
    }

    response
        .json::<GithubRelease>()
        .await
        .map_err(|error| format!("decode failed: {error}"))
}

fn strip_v(tag: &str) -> &str {
    tag.strip_prefix('v').unwrap_or(tag)
}

fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let mut parts = strip_v(s.trim()).split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch_raw = parts.next()?;
    // Drop any pre-release/build suffix after the patch: 0.2.0-rc1 -> 0.2.0
    let patch = patch_raw.split(['-', '+']).next()?.parse().ok()?;
    Some((major, minor, patch))
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_leading_v() {
        assert_eq!(strip_v("v0.1.0"), "0.1.0");
        assert_eq!(strip_v("0.1.0"), "0.1.0");
    }

    #[test]
    fn parses_simple_versions() {
        assert_eq!(parse_version("0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.2.0-rc1"), Some((0, 2, 0)));
    }

    #[test]
    fn is_newer_respects_semver_order() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("v0.1.1", "0.1.0"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.1.0", "0.2.0"));
    }

    #[test]
    fn is_newer_false_on_garbage() {
        assert!(!is_newer("nope", "0.1.0"));
        assert!(!is_newer("v0.1.0", "nope"));
    }

    #[test]
    fn describe_unchecked_shows_checking_text_and_no_link() {
        let display = UpdateStatus::Unchecked.describe();
        assert_eq!(display.text, "Checking for updates...");
        assert!(display.link.is_none());
    }

    #[test]
    fn describe_no_update_shows_up_to_date_and_no_link() {
        let display = UpdateStatus::NoUpdate.describe();
        assert_eq!(display.text, "Up to date");
        assert!(display.link.is_none());
    }

    #[test]
    fn describe_update_available_shows_version_and_link() {
        let display = UpdateStatus::UpdateAvailable {
            version: "0.2.0".to_string(),
            url: "https://example.test/releases/v0.2.0".to_string(),
        }
        .describe();
        assert_eq!(display.text, "Update available: v0.2.0");
        assert_eq!(
            display.link.as_deref(),
            Some("https://example.test/releases/v0.2.0")
        );
    }

    #[test]
    fn describe_error_shows_reason_and_no_link() {
        let display = UpdateStatus::Error("http 500".to_string()).describe();
        assert_eq!(display.text, "Update check failed: http 500");
        assert!(display.link.is_none());
    }
}
