// SPDX-License-Identifier: MPL-2.0

use super::super::{LoginEventKind, LoginFlow, apply_login_success, log_login_failed};
use crate::app::{
    AppModel, Config, GrokLoginEvent, GrokLoginState, GrokLoginStatus, Handle, Message, ProviderId,
    Task, grok,
};

pub(crate) struct GrokLoginFlow;

impl LoginFlow for GrokLoginFlow {
    type State = GrokLoginState;
    type Event = GrokLoginEvent;
    const PROVIDER: ProviderId = ProviderId::Grok;

    fn state(app: &AppModel) -> &Option<Self::State> {
        &app.grok_login
    }

    fn state_mut(app: &mut AppModel) -> &mut Option<Self::State> {
        &mut app.grok_login
    }

    fn handle_mut(app: &mut AppModel) -> &mut Option<Handle> {
        &mut app.grok_login_handle
    }

    fn is_running(state: &Self::State) -> bool {
        state.status == GrokLoginStatus::Running
    }

    fn log_id(state: &Self::State) -> &str {
        &state.flow_id
    }

    fn status_debug(state: &Self::State) -> String {
        format!("{:?}", state.status)
    }

    fn account_exists(config: &Config, account_id: &str) -> bool {
        config
            .grok_managed_accounts
            .iter()
            .any(|a| a.id == account_id)
    }

    fn failed_state(error: String) -> Self::State {
        GrokLoginState {
            flow_id: "failed".to_string(),
            status: GrokLoginStatus::Failed,
            login_url: None,
            error: Some(error),
            importing_from_host_cli: false,
        }
    }

    fn prepare(config: Config) -> Result<(Self::State, cosmic::iced::Task<Self::Event>), String> {
        grok::prepare(config)
    }

    fn prepare_for_reauth(
        config: Config,
        account_id: &str,
    ) -> Result<(Self::State, cosmic::iced::Task<Self::Event>), String> {
        grok::prepare_targeted(account_id.to_string(), config)
    }

    fn wrap_event(event: Self::Event) -> Message {
        Message::LoginEvent(ProviderId::Grok, Box::new(LoginEventKind::Grok(event)))
    }

    fn on_event(app: &mut AppModel, event: Self::Event) -> Task<Message> {
        match event {
            GrokLoginEvent::LoginUrl { flow_id, url } => {
                let Some(login) = app.grok_login.as_mut() else {
                    return Task::none();
                };
                if login.flow_id != flow_id {
                    return Task::none();
                }
                login.login_url = Some(url);
                Task::none()
            }
            GrokLoginEvent::Finished { flow_id, result } => {
                let Some(login) = app.grok_login.as_ref() else {
                    return Task::none();
                };
                if login.flow_id != flow_id {
                    return Task::none();
                }
                app.grok_login_handle = None;
                match *result {
                    Ok(success) => {
                        let account_id = success.account.id.clone();
                        app.grok_login = None;
                        apply_login_success(
                            app,
                            ProviderId::Grok,
                            &flow_id,
                            account_id,
                            move |cfg| grok::account::apply_login_account(cfg, success.account),
                        )
                    }
                    Err(error) => {
                        if let Some(login) = app.grok_login.as_mut() {
                            login.status = GrokLoginStatus::Failed;
                            login.error = Some(error.clone());
                        }
                        log_login_failed(&app.process_info.id, ProviderId::Grok, &flow_id, &error);
                        Task::none()
                    }
                }
            }
        }
    }
}
