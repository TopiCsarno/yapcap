// SPDX-License-Identifier: MPL-2.0

use serde::Deserialize;

const DEFAULT_URL: &str = "https://api.github.com/repos/TopiCsarno/yapcap/releases/latest";
#[cfg(debug_assertions)]
const DEBUG_UPDATE_AVAILABLE_ENV: &str = "YAPCAP_DEBUG_UPDATE_AVAILABLE";
const USER_AGENT: &str = concat!("yapcap/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    Unchecked,
    NoUpdate,
    UpdateAvailable { version: String, url: String },
    Error(String),
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
}

pub async fn check(client: &reqwest::Client) -> UpdateStatus {
    #[cfg(debug_assertions)]
    if let Ok(value) = std::env::var(DEBUG_UPDATE_AVAILABLE_ENV) {
        return debug_update_available_status(&value);
    }

    match fetch_release(client, DEFAULT_URL).await {
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

#[cfg(any(debug_assertions, test))]
fn debug_update_available_status(value: &str) -> UpdateStatus {
    let version = match value.trim() {
        "" | "1" | "true" | "yes" => "9.9.9",
        version => strip_v(version),
    };

    UpdateStatus::UpdateAvailable {
        version: version.to_string(),
        url: format!("https://github.com/TopiCsarno/yapcap/releases/tag/v{version}"),
    }
}

async fn fetch_release(client: &reqwest::Client, url: &str) -> Result<GithubRelease, String> {
    let response = client
        .get(url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| request_failure_message(&e))?;

    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err("no releases".to_string());
    }
    let body = response
        .text()
        .await
        .map_err(|e| format!("read failed after http {}: {e}", status.as_u16()))?;
    if !status.is_success() {
        return Err(http_failure_message(status, &body));
    }

    serde_json::from_str::<GithubRelease>(&body)
        .map_err(|e| format!("decode failed: {e}; body: {}", compact_body(&body)))
}

fn request_failure_message(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        return format!("request timed out after 20s: {error}");
    }
    if error.is_connect() {
        return format!("connection failed: {error}");
    }
    format!("request failed: {error}")
}

fn http_failure_message(status: reqwest::StatusCode, body: &str) -> String {
    let reason = status.canonical_reason().unwrap_or("unknown status");
    let body = compact_body(body);
    if body.is_empty() {
        return format!("http {} {reason}", status.as_u16());
    }
    format!("http {} {reason}: {body}", status.as_u16())
}

fn compact_body(body: &str) -> String {
    let one_line = body.split_whitespace().collect::<Vec<_>>().join(" ");
    one_line.chars().take(240).collect()
}

fn strip_v(tag: &str) -> &str {
    tag.strip_prefix('v').unwrap_or(tag)
}

fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let mut parts = strip_v(s.trim()).split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch_raw = parts.next()?;
    let patch = patch_raw.split(['-', '+']).next()?.parse().ok()?;
    Some((major, minor, patch))
}

#[must_use]
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
    fn debug_update_available_status_defaults_to_future_release() {
        assert_eq!(
            debug_update_available_status("1"),
            UpdateStatus::UpdateAvailable {
                version: "9.9.9".to_string(),
                url: "https://github.com/TopiCsarno/yapcap/releases/tag/v9.9.9".to_string(),
            }
        );
    }

    #[test]
    fn debug_update_available_status_accepts_version_value() {
        assert_eq!(
            debug_update_available_status("v0.1.0"),
            UpdateStatus::UpdateAvailable {
                version: "0.1.0".to_string(),
                url: "https://github.com/TopiCsarno/yapcap/releases/tag/v0.1.0".to_string(),
            }
        );
    }
}
