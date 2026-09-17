use super::super::{LoginEventKind, LoginFlow, apply_login_success};
use crate::account_selection::select_account_after_login;
use crate::app::{AppModel, Config, Handle, Message, ProviderId, Task, synthetic};
use crate::providers::synthetic::login::{
    SyntheticLoginEvent, SyntheticLoginState, SyntheticLoginStatus,
};

pub(crate) struct SyntheticLoginFlow;

impl LoginFlow for SyntheticLoginFlow {
    type State = SyntheticLoginState;
    type Event = SyntheticLoginEvent;
    const PROVIDER: ProviderId = ProviderId::Synthetic;

    fn state(app: &AppModel) -> &Option<Self::State> {
        &app.synthetic_login
    }

    fn state_mut(app: &mut AppModel) -> &mut Option<Self::State> {
        &mut app.synthetic_login
    }

    fn handle_mut(app: &mut AppModel) -> &mut Option<Handle> {
        &mut app.synthetic_login_handle
    }

    fn is_running(state: &Self::State) -> bool {
        state.status == SyntheticLoginStatus::Editing
    }

    fn log_id(state: &Self::State) -> &str {
        &state.account_id
    }

    fn status_debug(state: &Self::State) -> String {
        format!("{:?}", state.status)
    }

    fn account_exists(config: &Config, account_id: &str) -> bool {
        config
            .synthetic_managed_accounts
            .iter()
            .any(|account| account.id == account_id)
    }

    fn failed_state(error: String) -> Self::State {
        SyntheticLoginState::failed(error)
    }

    fn prepare(config: Config) -> Result<(Self::State, cosmic::iced::Task<Self::Event>), String> {
        let _ = config;
        Ok((synthetic::login::prepare(), cosmic::iced::Task::none()))
    }

    fn prepare_for_reauth(
        config: Config,
        account_id: &str,
    ) -> Result<(Self::State, cosmic::iced::Task<Self::Event>), String> {
        Ok((
            synthetic::login::prepare_for_reauth(config, account_id)?,
            cosmic::iced::Task::none(),
        ))
    }

    fn wrap_event(event: Self::Event) -> Message {
        Message::LoginEvent(
            ProviderId::Synthetic,
            Box::new(LoginEventKind::Synthetic(event)),
        )
    }

    fn on_event(app: &mut AppModel, event: Self::Event) -> Task<Message> {
        match event {
            SyntheticLoginEvent::ApiKeyChanged(api_key) => {
                if let Some(login) = app.synthetic_login.as_mut() {
                    login.update_api_key(api_key);
                }
                Task::none()
            }
            SyntheticLoginEvent::ApiKeyVisibilityToggled => {
                if let Some(login) = app.synthetic_login.as_mut() {
                    login.toggle_api_key_visibility();
                }
                Task::none()
            }
            SyntheticLoginEvent::LabelChanged(label) => {
                if let Some(login) = app.synthetic_login.as_mut() {
                    login.update_label(label);
                }
                Task::none()
            }
            SyntheticLoginEvent::Saved => {
                let Some(login) = app.synthetic_login.as_mut() else {
                    return Task::none();
                };
                let flow_id = login.account_id.clone();
                match synthetic::login::save(&app.config, login) {
                    Ok(managed_account) => {
                        let account_id = managed_account.id.clone();
                        let selected_account_id = account_id.clone();
                        let task = apply_login_success(
                            app,
                            ProviderId::Synthetic,
                            &flow_id,
                            account_id,
                            move |config| {
                                synthetic::account::apply_login_account(config, managed_account);
                                select_account_after_login(
                                    config,
                                    ProviderId::Synthetic,
                                    selected_account_id,
                                );
                            },
                        );
                        app.synthetic_login_handle = None;
                        app.synthetic_login = None;
                        task
                    }
                    Err(_) => Task::none(),
                }
            }
        }
    }
}
