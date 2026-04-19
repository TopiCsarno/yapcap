use crate::auth::{ClaudeAuth, load_claude_auth_from_path};
use crate::error::{ClaudeError, Result};
use chrono::{DateTime, TimeZone, Utc};
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const REFRESH_BEFORE: chrono::Duration = chrono::Duration::minutes(5);
const CLAUDE_AUTH_TIMEOUT: Duration = Duration::from_secs(8);

pub fn load_fresh_auth(path: &Path, now: DateTime<Utc>) -> Result<ClaudeAuth> {
    let auth = load_claude_auth_from_path(path)?;
    if should_refresh_auth(&auth, now) {
        refresh_claude_credentials(path)?;
        return load_claude_auth_from_path(path);
    }
    Ok(auth)
}

pub fn refresh_claude_credentials(credentials_path: &Path) -> Result<()> {
    let binary = find_claude_binary().ok_or(ClaudeError::CliUnavailable)?;
    refresh_claude_credentials_with_binary(credentials_path, &binary)
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

fn refresh_claude_credentials_with_binary(credentials_path: &Path, binary: &Path) -> Result<()> {
    let config_dir = credentials_path
        .parent()
        .ok_or(ClaudeError::CliUnavailable)?;
    let mut child = Command::new(binary)
        .args(["auth", "status", "--json"])
        .env("CLAUDE_CONFIG_DIR", config_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(ClaudeError::CliCommand)?;

    let started_at = Instant::now();
    loop {
        match child.try_wait().map_err(ClaudeError::CliIo)? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => {
                return Err(ClaudeError::CliStatusFailed {
                    status: status.to_string(),
                }
                .into());
            }
            None if started_at.elapsed() >= CLAUDE_AUTH_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ClaudeError::CliTimeout {
                    timeout: CLAUDE_AUTH_TIMEOUT,
                }
                .into());
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
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
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
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
) -> Result<ClaudeAuth> {
    let auth = load_claude_auth_from_path(path)?;
    if should_refresh_auth(&auth, now) {
        refresh_claude_credentials_with_binary(path, binary)?;
        return load_claude_auth_from_path(path);
    }
    Ok(auth)
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
        let auth = load_fresh_auth_with_binary(&credentials_path, now, &fake_claude).unwrap();

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
exit 7
"#,
        )
        .unwrap();
        #[cfg(unix)]
        fs::set_permissions(&fake_claude, fs::Permissions::from_mode(0o755)).unwrap();

        let now = Utc.timestamp_millis_opt(1_700_000_000_000).unwrap();
        let auth = load_fresh_auth_with_binary(&credentials_path, now, &fake_claude).unwrap();

        assert_eq!(auth.access_token, "current-token");
        assert_eq!(auth.expires_at_ms, Some(1_700_007_200_000));
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
