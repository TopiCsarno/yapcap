// SPDX-License-Identifier: MPL-2.0

use crate::auth::{ClaudeAuth, load_claude_auth_from_path};
use crate::error::ClaudeError;
use chrono::{DateTime, TimeZone, Utc};
use serde::Deserialize;
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const REFRESH_BEFORE: chrono::Duration = chrono::Duration::minutes(5);
const CLAUDE_AUTH_TIMEOUT: Duration = Duration::from_secs(8);
const CLAUDE_SPAWN_RETRY_DELAY: Duration = Duration::from_millis(50);
const CLAUDE_SPAWN_RETRIES: u8 = 3;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ClaudeAccountStatus {
    #[serde(default)]
    pub email: Option<String>,
    #[serde(rename = "orgName", default)]
    pub organization: Option<String>,
    #[serde(rename = "subscriptionType", default)]
    pub subscription_type: Option<String>,
}

pub fn load_fresh_auth(path: &Path, now: DateTime<Utc>) -> Result<(ClaudeAuth, bool), ClaudeError> {
    let auth = load_claude_auth_from_path(path)?;
    if should_refresh_auth(&auth, now) {
        refresh_claude_credentials(path)?;
        let refreshed = load_claude_auth_from_path(path)?;
        let was_refreshed = !should_refresh_auth(&refreshed, now);
        return Ok((refreshed, was_refreshed));
    }
    Ok((auth, false))
}

pub fn load_account_status(config_dir: &Path) -> Result<ClaudeAccountStatus, ClaudeError> {
    let stdout = run_claude_status_command(config_dir, true)?.unwrap_or_default();
    serde_json::from_slice(&stdout).map_err(ClaudeError::DecodeCliStatus)
}

pub fn refresh_claude_credentials(credentials_path: &Path) -> Result<(), ClaudeError> {
    let config_dir = credentials_path
        .parent()
        .ok_or(ClaudeError::CliUnavailable)?;
    let _ = run_claude_status_command(config_dir, false)?;
    Ok(())
}

fn should_refresh_auth(auth: &ClaudeAuth, now: DateTime<Utc>) -> bool {
    let Some(expires_at_ms) = auth.expires_at_ms else {
        return false;
    };
    let Some(expires_at) = Utc.timestamp_millis_opt(expires_at_ms).single() else {
        return true;
    };
    expires_at <= now + REFRESH_BEFORE
}

fn run_claude_status_command(
    config_dir: &Path,
    capture_stdout: bool,
) -> Result<Option<Vec<u8>>, ClaudeError> {
    let binary = find_claude_binary().ok_or(ClaudeError::CliUnavailable)?;
    let mut child = spawn_claude_status_command(&binary, config_dir, capture_stdout)?;
    let started_at = Instant::now();
    loop {
        match child.try_wait().map_err(ClaudeError::CliIo)? {
            Some(status) if status.success() => {
                if capture_stdout {
                    return child
                        .wait_with_output()
                        .map(|output| Some(output.stdout))
                        .map_err(ClaudeError::CliIo);
                }
                return Ok(None);
            }
            Some(status) => {
                return Err(ClaudeError::CliStatusFailed {
                    status: status.to_string(),
                });
            }
            None if started_at.elapsed() >= CLAUDE_AUTH_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ClaudeError::CliTimeout {
                    timeout: CLAUDE_AUTH_TIMEOUT,
                });
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn spawn_claude_status_command(
    binary: &Path,
    config_dir: &Path,
    capture_stdout: bool,
) -> Result<std::process::Child, ClaudeError> {
    for attempt in 0..=CLAUDE_SPAWN_RETRIES {
        match Command::new(binary)
            .args(["auth", "status", "--json"])
            .env("CLAUDE_CONFIG_DIR", config_dir)
            .stdin(Stdio::null())
            .stdout(if capture_stdout {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => return Ok(child),
            Err(error)
                if error.kind() == std::io::ErrorKind::ExecutableFileBusy
                    && attempt < CLAUDE_SPAWN_RETRIES =>
            {
                std::thread::sleep(CLAUDE_SPAWN_RETRY_DELAY);
            }
            Err(error) => return Err(ClaudeError::CliCommand(error)),
        }
    }
    Err(ClaudeError::CliUnavailable)
}

fn find_claude_binary() -> Option<PathBuf> {
    env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| env::split_paths(&paths).collect::<Vec<_>>())
        .map(|dir| dir.join("claude"))
        .chain(
            dirs::home_dir()
                .into_iter()
                .map(|home| home.join(".local/bin/claude")),
        )
        .find(|path| is_executable(path))
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
fn load_fresh_auth_with_binary(
    path: &Path,
    now: DateTime<Utc>,
    binary: &Path,
) -> Result<(ClaudeAuth, bool), ClaudeError> {
    let auth = load_claude_auth_from_path(path)?;
    if should_refresh_auth(&auth, now) {
        refresh_claude_credentials_with_binary(path, binary)?;
        let refreshed = load_claude_auth_from_path(path)?;
        let was_refreshed = !should_refresh_auth(&refreshed, now);
        return Ok((refreshed, was_refreshed));
    }
    Ok((auth, false))
}

#[cfg(test)]
fn refresh_claude_credentials_with_binary(
    credentials_path: &Path,
    binary: &Path,
) -> Result<(), ClaudeError> {
    let config_dir = credentials_path
        .parent()
        .ok_or(ClaudeError::CliUnavailable)?;
    run_claude_status_with_binary(config_dir, binary, false).map(|_| ())
}

#[cfg(test)]
fn load_account_status_with_binary(
    config_dir: &Path,
    binary: &Path,
) -> Result<ClaudeAccountStatus, ClaudeError> {
    let stdout = run_claude_status_with_binary(config_dir, binary, true)?.unwrap_or_default();
    serde_json::from_slice(&stdout).map_err(ClaudeError::DecodeCliStatus)
}

#[cfg(test)]
fn run_claude_status_with_binary(
    config_dir: &Path,
    binary: &Path,
    capture_stdout: bool,
) -> Result<Option<Vec<u8>>, ClaudeError> {
    let mut child = spawn_claude_status_command(binary, config_dir, capture_stdout)?;

    let started_at = Instant::now();
    loop {
        match child.try_wait().map_err(ClaudeError::CliIo)? {
            Some(status) if status.success() => {
                if capture_stdout {
                    return child
                        .wait_with_output()
                        .map(|output| Some(output.stdout))
                        .map_err(ClaudeError::CliIo);
                }
                return Ok(None);
            }
            Some(status) => {
                return Err(ClaudeError::CliStatusFailed {
                    status: status.to_string(),
                });
            }
            None if started_at.elapsed() >= CLAUDE_AUTH_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ClaudeError::CliTimeout {
                    timeout: CLAUDE_AUTH_TIMEOUT,
                });
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    #[test]
    fn detects_expiring_claude_auth() {
        let auth = ClaudeAuth {
            access_token: "token".to_string(),
            id_token: None,
            scopes: vec!["user:profile".to_string()],
            subscription_type: None,
            expires_at_ms: Some(1_700_000_000_000),
        };
        let now = Utc.timestamp_millis_opt(1_699_999_760_001).unwrap();
        assert!(should_refresh_auth(&auth, now));

        let now = Utc.timestamp_millis_opt(1_699_999_600_000).unwrap();
        assert!(!should_refresh_auth(&auth, now));
    }

    #[test]
    fn refreshes_expired_credentials_with_claude_cli() {
        let dir = tempdir().unwrap();
        let credentials_path = dir.path().join(".credentials.json");
        write_credentials(&credentials_path, "old-token", 1_700_000_000_000);

        let fake_claude = dir.path().join("claude");
        fs::write(
            &fake_claude,
            r#"#!/bin/sh
cat > "$CLAUDE_CONFIG_DIR/.credentials.json" <<'JSON'
{
  "claudeAiOauth": {
    "accessToken": "new-token",
    "refreshToken": "refresh",
    "expiresAt": 1700007200000,
    "scopes": ["user:profile"],
    "subscriptionType": "pro"
  }
}
JSON
"#,
        )
        .unwrap();
        #[cfg(unix)]
        fs::set_permissions(&fake_claude, fs::Permissions::from_mode(0o755)).unwrap();

        let now = Utc.timestamp_millis_opt(1_700_000_000_000).unwrap();
        let (auth, was_refreshed) =
            load_fresh_auth_with_binary(&credentials_path, now, &fake_claude).unwrap();

        assert!(was_refreshed);
        assert_eq!(auth.access_token, "new-token");
        assert_eq!(auth.expires_at_ms, Some(1_700_007_200_000));
        let raw = fs::read_to_string(credentials_path).unwrap();
        assert!(raw.contains("new-token"));
    }

    #[test]
    fn leaves_fresh_credentials_untouched() {
        let dir = tempdir().unwrap();
        let credentials_path = dir.path().join(".credentials.json");
        write_credentials(&credentials_path, "current-token", 1_700_007_200_000);

        let fake_claude = dir.path().join("claude");
        fs::write(
            &fake_claude,
            r#"#!/bin/sh
printf '{"email":"user@example.com","orgName":"Org","subscriptionType":"pro"}'
"#,
        )
        .unwrap();
        #[cfg(unix)]
        fs::set_permissions(&fake_claude, fs::Permissions::from_mode(0o755)).unwrap();

        let now = Utc.timestamp_millis_opt(1_700_000_000_000).unwrap();
        let (auth, was_refreshed) =
            load_fresh_auth_with_binary(&credentials_path, now, &fake_claude).unwrap();

        assert!(!was_refreshed);
        assert_eq!(auth.access_token, "current-token");
        assert_eq!(auth.expires_at_ms, Some(1_700_007_200_000));
    }

    #[test]
    fn loads_account_status_from_cli_json() {
        let dir = tempdir().unwrap();
        let fake_claude = dir.path().join("claude");
        fs::write(
            &fake_claude,
            r#"#!/bin/sh
printf '{"email":"user@example.com","orgName":"Example Org","subscriptionType":"max"}'
"#,
        )
        .unwrap();
        #[cfg(unix)]
        fs::set_permissions(&fake_claude, fs::Permissions::from_mode(0o755)).unwrap();

        let status = load_account_status_with_binary(dir.path(), &fake_claude).unwrap();

        assert_eq!(status.email.as_deref(), Some("user@example.com"));
        assert_eq!(status.organization.as_deref(), Some("Example Org"));
        assert_eq!(status.subscription_type.as_deref(), Some("max"));
    }

    fn write_credentials(path: &Path, token: &str, expires_at_ms: i64) {
        fs::write(
            path,
            format!(
                r#"{{
  "claudeAiOauth": {{
    "accessToken": "{token}",
    "refreshToken": "refresh",
    "expiresAt": {expires_at_ms},
    "scopes": ["user:profile"],
    "subscriptionType": "pro"
  }}
}}"#
            ),
        )
        .unwrap();
    }
}
