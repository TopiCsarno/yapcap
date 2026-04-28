// SPDX-License-Identifier: MPL-2.0

use crate::auth::{email_from_id_token, load_codex_auth_from_home};
use crate::config::{Config, ManagedCodexAccountConfig};
use crate::model::UsageSnapshot;
use crate::providers::codex::account::{
    commit_pending_home, create_private_dir, find_matching_account, managed_home, new_account_id,
};
use crate::providers::codex::fetch;
use chrono::Utc;
use cosmic::iced::Task;
use cosmic::iced::futures::SinkExt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct CodexLoginState {
    pub flow_id: String,
    pub status: CodexLoginStatus,
    pub login_url: Option<String>,
    pub output: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexLoginStatus {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone)]
pub enum CodexLoginEvent {
    Output {
        flow_id: String,
        line: String,
        login_url: Option<String>,
    },
    Finished {
        flow_id: String,
        result: Box<Result<CodexLoginSuccess, String>>,
    },
}

#[derive(Debug, Clone)]
pub struct CodexLoginSuccess {
    pub account: ManagedCodexAccountConfig,
    pub snapshot: Option<UsageSnapshot>,
}

pub fn prepare(config: Config) -> Result<(CodexLoginState, Task<CodexLoginEvent>), String> {
    let flow_id = new_account_id();
    let account_root = crate::config::paths().codex_accounts_dir;
    let pending_home = account_root.join(format!("pending-{flow_id}"));
    let stable_home = managed_home(&flow_id);

    create_private_dir(&account_root)?;
    create_private_dir(&pending_home)?;
    fs::write(
        pending_home.join("config.toml"),
        "cli_auth_credentials_store = \"file\"\n",
    )
    .map_err(|error| format!("failed to write Codex config: {error}"))?;

    let state = CodexLoginState {
        flow_id: flow_id.clone(),
        status: CodexLoginStatus::Running,
        login_url: None,
        output: Vec::new(),
        error: None,
    };
    let stream = cosmic::iced::stream::channel(100, move |mut output| async move {
        run_login(flow_id, config, pending_home, stable_home, &mut output).await;
    });

    Ok((state, Task::stream(stream)))
}

async fn run_login(
    flow_id: String,
    config: Config,
    pending_home: PathBuf,
    stable_home: PathBuf,
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<CodexLoginEvent>,
) {
    let guard = PendingDirGuard {
        path: pending_home.clone(),
        keep: false,
    };
    let result = run_login_inner(&flow_id, &config, &pending_home, &stable_home, output).await;
    if result.is_ok() {
        std::mem::forget(guard);
    }
    let _ = output
        .send(CodexLoginEvent::Finished {
            flow_id,
            result: Box::new(result),
        })
        .await;
}

async fn run_login_inner(
    flow_id: &str,
    config: &Config,
    pending_home: &Path,
    stable_home: &Path,
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<CodexLoginEvent>,
) -> Result<CodexLoginSuccess, String> {
    run_codex_login_process(flow_id, pending_home, &["login"], output).await?;

    let auth = load_codex_auth_from_home(pending_home)
        .map_err(|error| format!("Codex login did not create a usable auth.json: {error}"))?;
    let snapshot = fetch(&crate::runtime::http_client(), pending_home.to_path_buf())
        .await
        .map_err(|error| tracing::warn!("Codex usage validation failed after login: {error}"))
        .ok();

    let provider_account_id = snapshot
        .as_ref()
        .and_then(|s| s.identity.account_id.clone())
        .or_else(|| auth.account_id.clone());
    let login_email = snapshot
        .as_ref()
        .and_then(|s| s.identity.email.clone())
        .or_else(|| auth.id_token.as_deref().and_then(email_from_id_token));

    let existing = find_matching_account(config, login_email.as_deref());

    let target_home = existing.map_or_else(
        || stable_home.to_path_buf(),
        |account| account.codex_home.clone(),
    );
    commit_pending_home(pending_home, &target_home)?;

    let now = Utc::now();
    let account = ManagedCodexAccountConfig {
        id: existing.map_or_else(
            || {
                stable_home
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("codex-account")
                    .to_string()
            },
            |account| account.id.clone(),
        ),
        label: login_email
            .clone()
            .or_else(|| existing.map(|account| account.label.clone()))
            .unwrap_or_else(|| "Codex account".to_string()),
        codex_home: target_home,
        email: login_email,
        provider_account_id,
        created_at: existing.map_or(now, |account| account.created_at),
        updated_at: now,
        last_authenticated_at: Some(now),
    };

    Ok(CodexLoginSuccess { account, snapshot })
}

fn find_codex_binary() -> Option<(std::path::PathBuf, Option<std::path::PathBuf>)> {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(':') {
            let candidate = std::path::PathBuf::from(dir).join("codex");
            if candidate.is_file() {
                return Some((candidate, None));
            }
        }
    }
    let node_dir = dirs::home_dir()?.join(".nvm/versions/node");
    let mut candidates: Vec<std::path::PathBuf> = std::fs::read_dir(&node_dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("bin/codex"))
        .filter(|p| p.is_file())
        .collect();
    candidates.sort_by_key(|p| p.metadata().and_then(|m| m.modified()).ok());
    let binary = candidates.pop()?;
    let bin_dir = binary.parent().map(Path::to_path_buf);
    Some((binary, bin_dir))
}

async fn run_codex_login_process(
    flow_id: &str,
    pending_home: &Path,
    args: &[&str],
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<CodexLoginEvent>,
) -> Result<(), String> {
    let (binary, extra_bin_dir) =
        find_codex_binary().ok_or_else(|| "Codex CLI not found".to_string())?;
    let path_env = match extra_bin_dir {
        Some(bin_dir) => {
            let current = std::env::var("PATH").unwrap_or_default();
            format!("{}:{current}", bin_dir.display())
        }
        None => std::env::var("PATH").unwrap_or_default(),
    };
    let mut child = Command::new(binary)
        .env("CODEX_HOME", pending_home)
        .env("PATH", path_env)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("failed to start Codex login: {error}"))?;

    if let Some(stderr) = child.stderr.take() {
        let flow_id = flow_id.to_string();
        let mut sender = output.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                send_output(&flow_id, line, &mut sender).await;
            }
        });
    }

    if let Some(stdout) = child.stdout.take() {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            send_output(flow_id, line, output).await;
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|error| format!("failed to wait for Codex login: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("Codex login exited with {status}"))
    }
}

async fn send_output(
    flow_id: &str,
    line: String,
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<CodexLoginEvent>,
) {
    let clean = strip_ansi(&line);
    let login_url = find_url(&clean);
    let _ = output
        .send(CodexLoginEvent::Output {
            flow_id: flow_id.to_string(),
            line: clean,
            login_url,
        })
        .await;
}

fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else if ch != '\x1b' {
            result.push(ch);
        }
    }
    result
}

fn find_url(line: &str) -> Option<String> {
    line.split_whitespace()
        .find(|word| word.starts_with("https://") || word.starts_with("http://"))
        .map(|word| {
            word.trim_end_matches(['.', ',', ')', ']', '}', '"', '\''])
                .to_string()
        })
}

struct PendingDirGuard {
    path: PathBuf,
    keep: bool,
}

impl Drop for PendingDirGuard {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        let Some(name) = self.path.file_name().and_then(|name| name.to_str()) else {
            return;
        };
        if name.starts_with("pending-codex-") {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_url_from_line() {
        assert_eq!(
            find_url("Open https://example.com/device and sign in."),
            Some("https://example.com/device".to_string())
        );
    }

    #[test]
    fn strips_ansi_codes() {
        assert_eq!(
            strip_ansi("\x1b[94mhttps://auth.openai.com/codex/device\x1b[0m"),
            "https://auth.openai.com/codex/device"
        );
        assert_eq!(
            strip_ansi("\x1b[90m(expires in 15 minutes)\x1b[0m"),
            "(expires in 15 minutes)"
        );
        assert_eq!(strip_ansi("plain text"), "plain text");
    }

    #[test]
    fn extracts_ansi_wrapped_url() {
        let url_line = strip_ansi("\x1b[94mhttps://auth.openai.com/codex/device\x1b[0m");
        assert_eq!(
            find_url(&url_line).as_deref(),
            Some("https://auth.openai.com/codex/device")
        );
    }

    #[test]
    fn add_flow_matches_existing_account_by_email() {
        let now = Utc::now();
        let account = ManagedCodexAccountConfig {
            id: "work".to_string(),
            label: "user@example.com".to_string(),
            codex_home: PathBuf::from("/tmp/work"),
            email: Some("user@example.com".to_string()),
            provider_account_id: Some("acct_123".to_string()),
            created_at: now,
            updated_at: now,
            last_authenticated_at: Some(now),
        };
        let config = Config {
            codex_managed_accounts: vec![account],
            ..Config::default()
        };

        let found = find_matching_account(&config, Some("USER@example.com"));

        assert_eq!(found.map(|account| account.id.as_str()), Some("work"));
    }
}
