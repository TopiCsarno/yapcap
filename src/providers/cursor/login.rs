// SPDX-License-Identifier: MPL-2.0

use crate::config::{Browser, CursorCredentialSource, ManagedCursorAccountConfig, paths};
use crate::model::UsageSnapshot;
use crate::providers::cursor;
use crate::providers::cursor::identity::normalized_email;
use crate::providers::cursor::refresh::cookie_header_from_db;
use crate::providers::cursor::shared::{
    browser_command, browser_spawn_error, chromium_browser, new_account_id,
};
use crate::providers::cursor::storage::{
    PendingDirGuard, create_private_dir, managed_account_dir, profile_dir, remove_dir_if_exists,
    session_dir, write_imported_account,
};
use chrono::Utc;
use cosmic::iced::Task;
use cosmic::iced::futures::SinkExt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{Duration, Instant, sleep};

pub const LOGIN_URL: &str = "https://cursor.com/dashboard?tab=usage";

const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
const DETECT_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct CursorLoginState {
    pub flow_id: String,
    pub status: CursorLoginStatus,
    pub browser: Browser,
    pub login_url: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorLoginStatus {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone)]
pub enum CursorLoginEvent {
    Finished {
        flow_id: String,
        result: Box<Result<CursorLoginSuccess, String>>,
    },
}

#[derive(Debug, Clone)]
pub struct CursorLoginSuccess {
    pub account: ManagedCursorAccountConfig,
    pub snapshot: Option<UsageSnapshot>,
}

pub fn prepare(preferred: Browser) -> Result<(CursorLoginState, Task<CursorLoginEvent>), String> {
    let browser = chromium_browser(preferred);
    let flow_id = new_account_id();
    let account_root = paths().cursor_accounts_dir;
    let pending_root = account_root.join(format!("pending-{flow_id}"));

    create_private_dir(&account_root)?;
    create_private_dir(&pending_root)?;
    create_private_dir(&session_dir(&pending_root))?;
    create_private_dir(&profile_dir(&pending_root))?;

    let state = CursorLoginState {
        flow_id: flow_id.clone(),
        status: CursorLoginStatus::Running,
        browser,
        login_url: LOGIN_URL.to_string(),
        error: None,
    };
    let stream = cosmic::iced::stream::channel(10, move |mut output| async move {
        run_login(flow_id.clone(), browser, pending_root, &mut output).await;
    });

    Ok((state, Task::stream(stream)))
}

async fn run_login(
    flow_id: String,
    browser: Browser,
    pending_root: PathBuf,
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<CursorLoginEvent>,
) {
    let guard = PendingDirGuard {
        path: pending_root.clone(),
    };
    let result = run_login_inner(browser, &pending_root, &flow_id).await;
    if result.is_ok() {
        std::mem::forget(guard);
    }
    let _ = output
        .send(CursorLoginEvent::Finished {
            flow_id,
            result: Box::new(result),
        })
        .await;
}

async fn run_login_inner(
    browser: Browser,
    pending_root: &Path,
    flow_id: &str,
) -> Result<CursorLoginSuccess, String> {
    let profile_root = profile_dir(pending_root);
    let mut child = Command::new(browser_command(browser))
        .arg(format!("--user-data-dir={}", profile_root.display()))
        .arg("--no-first-run")
        .arg(LOGIN_URL)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| browser_spawn_error(browser, &error))?;

    let snapshot = wait_for_snapshot_at(&profile_root, browser).await?;
    let cookie_header = cookie_header_from_profile(&profile_root, browser).await?;
    let _ = child.start_kill();
    let _ = child.wait().await;

    let email = snapshot
        .identity
        .email
        .as_deref()
        .map(normalized_email)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Cursor login did not return an email address".to_string())?;
    let stable_root = managed_account_dir(flow_id);

    remove_dir_if_exists(&stable_root)?;
    fs::rename(pending_root, &stable_root)
        .map_err(|error| format!("failed to commit Cursor account: {error}"))?;

    let now = Utc::now();
    let account = ManagedCursorAccountConfig {
        id: flow_id.to_string(),
        email: email.clone(),
        label: email,
        account_root: stable_root,
        credential_source: CursorCredentialSource::ImportedBrowserProfile,
        browser: Some(browser),
        display_name: snapshot.identity.display_name.clone(),
        plan: snapshot.identity.plan.clone(),
        created_at: now,
        updated_at: now,
        last_authenticated_at: Some(now),
    };
    write_imported_account(&account, &cookie_header)?;

    Ok(CursorLoginSuccess {
        account,
        snapshot: Some(snapshot),
    })
}

async fn wait_for_snapshot_at(
    profile_root: &Path,
    browser: Browser,
) -> Result<UsageSnapshot, String> {
    let cookie_db_path = profile_root.join("Default").join("Cookies");
    let deadline = Instant::now() + LOGIN_TIMEOUT;
    let mut last_error = "waiting for Cursor sign-in".to_string();
    while Instant::now() < deadline {
        match cursor::fetch_at(&crate::runtime::http_client(), browser, &cookie_db_path).await {
            Ok(snapshot) => return Ok(snapshot),
            Err(error) => {
                last_error = error.to_string();
                sleep(DETECT_INTERVAL).await;
            }
        }
    }
    Err(format!("Cursor login timed out: {last_error}"))
}

async fn cookie_header_from_profile(
    profile_root: &Path,
    browser: Browser,
) -> Result<String, String> {
    let cookie_db_path = profile_root.join("Default").join("Cookies");
    cookie_header_from_db(browser, &cookie_db_path)
        .await
        .map_err(|error| format!("failed to persist Cursor session cookie: {error}"))
}
