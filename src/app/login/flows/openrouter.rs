// SPDX-License-Identifier: MPL-2.0

use super::super::{LoginEventKind, LoginFlow, apply_login_success};
use crate::account_selection::select_account_after_login;
use crate::app::{AppModel, Config, Handle, Message, ProviderId, Task, openrouter};
use crate::providers::openrouter::login::{
    OpenRouterLoginEvent, OpenRouterLoginState, OpenRouterLoginStatus,
};

pub(crate) struct OpenRouterLoginFlow;

impl LoginFlow for OpenRouterLoginFlow {
    type State = OpenRouterLoginState;
    type Event = OpenRouterLoginEvent;
    const PROVIDER: ProviderId = ProviderId::OpenRouter;

    fn state(app: &AppModel) -> &Option<Self::State> {
        &app.openrouter_login
    }

    fn state_mut(app: &mut AppModel) -> &mut Option<Self::State> {
        &mut app.openrouter_login
    }

    fn handle_mut(app: &mut AppModel) -> &mut Option<Handle> {
        &mut app.openrouter_login_handle
    }

    fn is_running(state: &Self::State) -> bool {
        state.status == OpenRouterLoginStatus::Editing
    }

    fn log_id(state: &Self::State) -> &str {
        &state.account_id
    }

    fn status_debug(state: &Self::State) -> String {
        format!("{:?}", state.status)
    }

    fn account_exists(config: &Config, account_id: &str) -> bool {
        config
            .openrouter_managed_accounts
            .iter()
            .any(|account| account.id == account_id)
    }

    fn failed_state(error: String) -> Self::State {
        OpenRouterLoginState::failed(error)
    }

    fn prepare(config: Config) -> Result<(Self::State, cosmic::iced::Task<Self::Event>), String> {
        let _ = config;
        Ok((openrouter::login::prepare(), cosmic::iced::Task::none()))
    }

    fn prepare_for_reauth(
        config: Config,
        account_id: &str,
    ) -> Result<(Self::State, cosmic::iced::Task<Self::Event>), String> {
        Ok((
            openrouter::login::prepare_for_reauth(config, account_id)?,
            cosmic::iced::Task::none(),
        ))
    }

    fn wrap_event(event: Self::Event) -> Message {
        Message::LoginEvent(
            ProviderId::OpenRouter,
            Box::new(LoginEventKind::OpenRouter(event)),
        )
    }

    fn on_event(app: &mut AppModel, event: Self::Event) -> Task<Message> {
        match event {
            OpenRouterLoginEvent::ApiKeyChanged(api_key) => {
                if let Some(login) = app.openrouter_login.as_mut() {
                    login.update_api_key(api_key);
                }
                Task::none()
            }
            OpenRouterLoginEvent::ApiKeyVisibilityToggled => {
                if let Some(login) = app.openrouter_login.as_mut() {
                    login.toggle_api_key_visibility();
                }
                Task::none()
            }
            OpenRouterLoginEvent::LabelChanged(label) => {
                if let Some(login) = app.openrouter_login.as_mut() {
                    login.update_label(label);
                }
                Task::none()
            }
            OpenRouterLoginEvent::Saved => {
                let Some(login) = app.openrouter_login.as_mut() else {
                    return Task::none();
                };
                let flow_id = login.account_id.clone();
                match openrouter::login::save(&app.config, login) {
                    Ok(managed_account) => {
                        let account_id = managed_account.id.clone();
                        let selected_account_id = account_id.clone();
                        let task = apply_login_success(
                            app,
                            ProviderId::OpenRouter,
                            &flow_id,
                            account_id,
                            move |config| {
                                openrouter::account::apply_login_account(config, managed_account);
                                select_account_after_login(
                                    config,
                                    ProviderId::OpenRouter,
                                    selected_account_id,
                                );
                            },
                        );
                        app.openrouter_login_handle = None;
                        app.openrouter_login = None;
                        task
                    }
                    Err(_) => Task::none(),
                }
            }
        }
    }
}
