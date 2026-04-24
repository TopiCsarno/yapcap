// SPDX-License-Identifier: MPL-2.0

use super::account::{
    commit_pending_dir, create_private_dir, find_matching_account, managed_config_dir,
    new_account_id, prune_managed_claude_config,
};
use super::fetch;
use super::refresh::load_account_status;
use crate::auth::load_claude_auth_from_config_dir;
use crate::config::{Config, ManagedClaudeAccountConfig, paths};
use crate::model::UsageSnapshot;
use chrono::Utc;
use cosmic::iced::Task;
use cosmic::iced::futures::SinkExt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

const REQUIRED_SCOPE: &str = "user:profile";

#[derive(Debug, Clone)]
pub struct ClaudeLoginState {
    pub flow_id: String,
    pub status: ClaudeLoginStatus,
    pub login_url: Option<String>,
    pub output: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeLoginStatus {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone)]
pub enum ClaudeLoginEvent {
    Output {
        flow_id: String,
        line: String,
        login_url: Option<String>,
    },
    Finished {
        flow_id: String,
        result: Box<Result<ClaudeLoginSuccess, String>>,
    },
}

#[derive(Debug, Clone)]
pub struct ClaudeLoginSuccess {
    pub account: ManagedClaudeAccountConfig,
    pub snapshot: Option<UsageSnapshot>,
}

pub fn prepare(config: Config) -> Result<(ClaudeLoginState, Task<ClaudeLoginEvent>), String> {
    let flow_id = new_account_id();
    let account_root = paths().claude_accounts_dir;
    let pending_dir = account_root.join(format!("pending-{flow_id}"));
    let stable_dir = managed_config_dir(&flow_id);

    create_private_dir(&account_root)?;
    create_private_dir(&pending_dir)?;

    let state = ClaudeLoginState {
        flow_id: flow_id.clone(),
        status: ClaudeLoginStatus::Running,
        login_url: None,
        output: Vec::new(),
        error: None,
    };
    let stream = cosmic::iced::stream::channel(100, move |mut output| async move {
        run_login(flow_id, config, pending_dir, stable_dir, &mut output).await;
    });

    Ok((state, Task::stream(stream)))
}

async fn run_login(
    flow_id: String,
    config: Config,
    pending_dir: PathBuf,
    stable_dir: PathBuf,
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<ClaudeLoginEvent>,
) {
    let guard = PendingDirGuard {
        path: pending_dir.clone(),
        keep: false,
    };
    let result = run_login_inner(&flow_id, &config, &pending_dir, &stable_dir, output).await;
    if result.is_ok() {
        std::mem::forget(guard);
    }
    let _ = output
        .send(ClaudeLoginEvent::Finished {
            flow_id,
            result: Box::new(result),
        })
        .await;
}

async fn run_login_inner(
    flow_id: &str,
    config: &Config,
    pending_dir: &Path,
    stable_dir: &Path,
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<ClaudeLoginEvent>,
) -> Result<ClaudeLoginSuccess, String> {
    run_claude_login_process(flow_id, pending_dir, output).await?;

    let auth = load_claude_auth_from_config_dir(pending_dir)
        .map_err(|error| format!("Claude login did not create usable credentials: {error}"))?;
    if !auth.scopes.iter().any(|scope| scope == REQUIRED_SCOPE) {
        return Err("Claude token missing user:profile scope".to_string());
    }

    let status = load_account_status(pending_dir).map_err(|error| {
        format!("Claude login did not produce usable account metadata: {error}")
    })?;
    let snapshot = fetch(&crate::runtime::http_client(), pending_dir.to_path_buf())
        .await
        .map_err(|error| tracing::warn!("Claude usage validation failed after login: {error}"))
        .ok();

    let existing = find_matching_account(config, status.email.as_deref());

    let target_dir = existing.map_or_else(
        || stable_dir.to_path_buf(),
        |account| account.config_dir.clone(),
    );
    prune_managed_claude_config(pending_dir)?;
    commit_pending_dir(&paths().claude_accounts_dir, pending_dir, &target_dir)?;

    let now = Utc::now();
    let account = ManagedClaudeAccountConfig {
        id: existing.map_or_else(
            || {
                stable_dir
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("claude-account")
                    .to_string()
            },
            |account| account.id.clone(),
        ),
        label: status
            .email
            .clone()
            .or_else(|| existing.and_then(|account| account.email.clone()))
            .unwrap_or_else(|| "Claude account".to_string()),
        config_dir: target_dir,
        email: status
            .email
            .clone()
            .or_else(|| existing.and_then(|account| account.email.clone())),
        organization: status
            .organization
            .clone()
            .or_else(|| existing.and_then(|account| account.organization.clone())),
        subscription_type: snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.identity.plan.clone())
            .or(status.subscription_type.clone())
            .or_else(|| existing.and_then(|account| account.subscription_type.clone())),
        created_at: existing.map_or(now, |account| account.created_at),
        updated_at: now,
        last_authenticated_at: Some(now),
    };

    Ok(ClaudeLoginSuccess { account, snapshot })
}

async fn run_claude_login_process(
    flow_id: &str,
    pending_dir: &Path,
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<ClaudeLoginEvent>,
) -> Result<(), String> {
    let mut child = Command::new("claude")
        .env("CLAUDE_CONFIG_DIR", pending_dir)
        .args(["auth", "login", "--claudeai"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| login_spawn_error(&error))?;

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
        .map_err(|error| format!("failed to wait for Claude login: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("Claude login exited with {status}"))
    }
}

async fn send_output(
    flow_id: &str,
    line: String,
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<ClaudeLoginEvent>,
) {
    let clean = strip_ansi(&line);
    let login_url = find_url(&clean);
    let _ = output
        .send(ClaudeLoginEvent::Output {
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

fn login_spawn_error(error: &std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound {
        "Claude CLI not found".to_string()
    } else {
        format!("failed to start Claude login: {error}")
    }
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
        if name.starts_with("pending-claude-") {
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
    fn spawn_error_names_missing_cli() {
        let error = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");

        assert_eq!(login_spawn_error(&error), "Claude CLI not found");
    }
}
