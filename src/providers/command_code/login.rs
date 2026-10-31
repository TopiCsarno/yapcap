// SPDX-License-Identifier: MPL-2.0

use super::{account, browser::BrowserLogin};
use crate::config::{Config, ManagedCommandCodeAccountConfig};
use chrono::{DateTime, Utc};
use cosmic::iced::{Task, futures::SinkExt};
use std::fmt;

#[derive(Clone)]
pub struct CommandCodeLoginState {
    pub flow_id: String,
    pub account_id: String,
    pub label: String,
    pub api_key: String,
    pub api_key_visible: bool,
    pub status: CommandCodeLoginStatus,
    pub login_url: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub reauth: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandCodeLoginStatus {
    Editing,
    Running,
}

#[derive(Clone)]
pub enum CommandCodeLoginEvent {
    ApiKeyChanged(String),
    ApiKeyVisibilityToggled,
    LabelChanged(String),
    BrowserRequested,
    ImportCliRequested,
    Saved,
    LoginUrl {
        flow_id: String,
        url: String,
    },
    Finished {
        flow_id: String,
        result: Box<Result<CommandCodeLoginSuccess, String>>,
    },
}

#[derive(Clone)]
pub struct CommandCodeLoginSuccess {
    api_key: String,
    label: String,
}

pub(crate) enum LoginSource {
    Browser,
    Cli,
    Key(String),
}

impl fmt::Debug for CommandCodeLoginState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandCodeLoginState")
            .field("flow_id", &self.flow_id)
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for CommandCodeLoginEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CommandCodeLoginEvent")
    }
}

impl CommandCodeLoginState {
    pub fn failed(error: String) -> Self {
        let mut state = new_state(None);
        state.error = Some(error);
        state
    }
}

pub(crate) fn prepare(
    config: &Config,
    target: Option<&str>,
) -> Result<CommandCodeLoginState, String> {
    let account = target
        .map(|id| {
            config
                .command_code_managed_accounts
                .iter()
                .find(|account| account.id == id)
                .ok_or_else(|| "Command Code account no longer exists".to_string())
        })
        .transpose()?;
    Ok(new_state(account))
}

fn new_state(account: Option<&ManagedCommandCodeAccountConfig>) -> CommandCodeLoginState {
    CommandCodeLoginState {
        flow_id: account::new_account_id(),
        account_id: account.map_or_else(account::new_account_id, |account| account.id.clone()),
        label: account.map_or_else(String::new, |account| account.label.clone()),
        api_key: String::new(),
        api_key_visible: false,
        status: CommandCodeLoginStatus::Editing,
        login_url: None,
        error: None,
        created_at: account.map_or_else(Utc::now, |account| account.created_at),
        reauth: account.is_some(),
    }
}

pub(crate) fn authenticate(
    state: &mut CommandCodeLoginState,
    source: LoginSource,
) -> Task<CommandCodeLoginEvent> {
    state.flow_id = account::new_account_id();
    state.status = CommandCodeLoginStatus::Running;
    state.login_url = None;
    state.error = None;
    let flow_id = state.flow_id.clone();
    Task::stream(cosmic::iced::stream::channel(
        4,
        move |mut output: cosmic::iced::futures::channel::mpsc::Sender<CommandCodeLoginEvent>| async move {
            let key = match source {
                LoginSource::Key(key) => account::normalize_key(&key),
                LoginSource::Cli => account::import_key(),
                LoginSource::Browser => match BrowserLogin::start().await {
                    Ok(browser) => {
                        if output
                            .send(CommandCodeLoginEvent::LoginUrl {
                                flow_id: flow_id.clone(),
                                url: browser.url.clone(),
                            })
                            .await
                            .is_err()
                        {
                            return;
                        }
                        browser.wait().await
                    }
                    Err(error) => Err(error),
                },
            };
            let result = match key {
                Ok(key) => validate(key).await,
                Err(error) => Err(error),
            };
            let _ = output
                .send(CommandCodeLoginEvent::Finished {
                    flow_id,
                    result: Box::new(result),
                })
                .await;
        },
    ))
}

async fn validate(api_key: String) -> Result<CommandCodeLoginSuccess, String> {
    let snapshot = super::fetch_key(&api_key)
        .await
        .map_err(|error| error.to_string())?;
    let label = snapshot
        .identity
        .display_name
        .or(snapshot.identity.email)
        .unwrap_or_else(|| "Command Code".to_string());
    Ok(CommandCodeLoginSuccess { api_key, label })
}

pub(crate) fn commit(
    config: &Config,
    state: &CommandCodeLoginState,
    success: CommandCodeLoginSuccess,
) -> Result<ManagedCommandCodeAccountConfig, String> {
    if state.reauth
        && !config
            .command_code_managed_accounts
            .iter()
            .any(|account| account.id == state.account_id)
    {
        return Err("Command Code account no longer exists".to_string());
    }
    if config.command_code_managed_accounts.iter().any(|existing| {
        existing.id != state.account_id
            && account::load_key(&existing.id)
                .ok()
                .is_some_and(|key| key == success.api_key)
    }) {
        return Err("This Command Code API key is already saved".to_string());
    }
    account::write_key(&state.account_id, &success.api_key)?;
    let now = Utc::now();
    Ok(ManagedCommandCodeAccountConfig {
        id: state.account_id.clone(),
        label: if state.label.trim().is_empty() {
            success.label
        } else {
            state.label.trim().to_string()
        },
        created_at: state.created_at,
        updated_at: now,
        last_authenticated_at: Some(now),
    })
}

#[cfg(test)]
mod tests;
