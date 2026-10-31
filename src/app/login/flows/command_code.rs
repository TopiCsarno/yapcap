// SPDX-License-Identifier: MPL-2.0

use super::super::{LoginEventKind, LoginFlow, apply_login_success_checked};
use crate::account_selection::select_account_after_login;
use crate::app::{AppModel, Config, Handle, Message, ProviderId, Task};
use crate::providers::command_code::{account, login};
use login::{
    CommandCodeLoginEvent as Event, CommandCodeLoginState as State,
    CommandCodeLoginStatus as Status, LoginSource,
};

pub(crate) struct CommandCodeLoginFlow;

impl LoginFlow for CommandCodeLoginFlow {
    type State = State;
    type Event = Event;
    const PROVIDER: ProviderId = ProviderId::CommandCode;

    fn state(app: &AppModel) -> &Option<State> {
        &app.command_code_login
    }

    fn state_mut(app: &mut AppModel) -> &mut Option<State> {
        &mut app.command_code_login
    }

    fn handle_mut(app: &mut AppModel) -> &mut Option<Handle> {
        &mut app.command_code_login_handle
    }

    fn is_running(state: &State) -> bool {
        state.status == Status::Running
    }

    fn log_id(state: &State) -> &str {
        &state.flow_id
    }

    fn status_debug(state: &State) -> String {
        format!("{:?}", state.status)
    }

    fn account_exists(config: &Config, account_id: &str) -> bool {
        config
            .command_code_managed_accounts
            .iter()
            .any(|account| account.id == account_id)
    }

    fn failed_state(error: String) -> State {
        State::failed(error)
    }

    fn prepare(config: Config) -> Result<(State, cosmic::iced::Task<Event>), String> {
        Ok((login::prepare(&config, None)?, cosmic::iced::Task::none()))
    }

    fn prepare_for_reauth(
        config: Config,
        account_id: &str,
    ) -> Result<(State, cosmic::iced::Task<Event>), String> {
        Ok((
            login::prepare(&config, Some(account_id))?,
            cosmic::iced::Task::none(),
        ))
    }

    fn wrap_event(event: Event) -> Message {
        Message::LoginEvent(
            ProviderId::CommandCode,
            Box::new(LoginEventKind::CommandCode(event)),
        )
    }

    fn on_event(app: &mut AppModel, event: Event) -> Task<Message> {
        let Some(state) = app.command_code_login.as_mut() else {
            return Task::none();
        };
        match event {
            Event::LoginUrl { flow_id, url }
                if flow_id == state.flow_id && state.status == Status::Running =>
            {
                state.login_url = Some(url.clone());
                crate::app::open_url(&url);
            }
            Event::Finished { flow_id, result }
                if flow_id == state.flow_id && state.status == Status::Running =>
            {
                app.command_code_login_handle = None;
                return finish(app, *result);
            }
            Event::ApiKeyChanged(key) if state.status == Status::Editing => state.api_key = key,
            Event::ApiKeyVisibilityToggled => state.api_key_visible = !state.api_key_visible,
            Event::LabelChanged(label) if state.status == Status::Editing => state.label = label,
            Event::BrowserRequested if state.status == Status::Editing => {
                return start(app, LoginSource::Browser);
            }
            Event::ImportCliRequested if state.status == Status::Editing => {
                return start(app, LoginSource::Cli);
            }
            Event::Saved if state.status == Status::Editing => {
                let source = LoginSource::Key(state.api_key.clone());
                return start(app, source);
            }
            _ => {}
        }
        Task::none()
    }
}

fn start(app: &mut AppModel, source: LoginSource) -> Task<Message> {
    let Some(state) = app.command_code_login.as_mut() else {
        return Task::none();
    };
    if let Some(handle) = app.command_code_login_handle.take() {
        handle.abort();
    }
    let task = login::authenticate(state, source)
        .map(|event| cosmic::Action::App(CommandCodeLoginFlow::wrap_event(event)));
    let (task, handle) = task.abortable();
    app.command_code_login_handle = Some(handle);
    task
}

fn finish(
    app: &mut AppModel,
    result: Result<login::CommandCodeLoginSuccess, String>,
) -> Task<Message> {
    let Some(state) = app.command_code_login.as_ref() else {
        return Task::none();
    };
    let flow_id = state.flow_id.clone();
    let account_id = state.account_id.clone();
    let previous_key = account::load_key(&account_id).ok();
    let is_reauthentication = state.reauth;
    let committed = result.and_then(|success| login::commit(&app.config, state, success));
    match committed {
        Ok(managed) => {
            let selected = managed.id.clone();
            match apply_login_success_checked(
                app,
                ProviderId::CommandCode,
                &flow_id,
                account_id.clone(),
                move |config| {
                    account::apply(config, managed);
                    select_account_after_login(config, ProviderId::CommandCode, selected);
                },
            ) {
                Ok(task) => {
                    app.command_code_login = None;
                    return task;
                }
                Err(()) => {
                    let error =
                        rollback_credentials(&account_id, previous_key, is_reauthentication)
                            .err()
                            .unwrap_or_else(|| {
                                "Could not save Command Code account settings".to_string()
                            });
                    fail(app, error);
                }
            }
        }
        Err(error) => fail(app, error),
    }
    Task::none()
}

fn rollback_credentials(
    account_id: &str,
    previous_key: Option<String>,
    is_reauthentication: bool,
) -> Result<(), String> {
    if let Some(key) = previous_key {
        account::write_key(account_id, &key).map_err(|_| {
            "Could not save settings or restore the previous Command Code credential".to_string()
        })
    } else if is_reauthentication {
        Ok(())
    } else {
        account::storage()
            .delete_account(account_id)
            .map(|_| ())
            .map_err(|_| {
                "Could not save settings or remove the new Command Code credential".to_string()
            })
    }
}

fn fail(app: &mut AppModel, error: String) {
    if let Some(state) = app.command_code_login.as_mut() {
        state.status = Status::Editing;
        state.login_url = None;
        state.error = Some(error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_failure_removes_only_new_account_storage() {
        let _env = crate::test_support::test_env();
        account::write_key("fixture-new", "fixture-new-key").unwrap();
        rollback_credentials("fixture-new", None, false).unwrap();
        assert!(account::load_key("fixture-new").is_err());
    }

    #[test]
    fn config_failure_restores_a_readable_previous_credential() {
        let _env = crate::test_support::test_env();
        account::write_key("fixture-existing", "fixture-replacement-key").unwrap();
        rollback_credentials(
            "fixture-existing",
            Some("fixture-old-key".to_string()),
            true,
        )
        .unwrap();
        assert_eq!(
            account::load_key("fixture-existing").unwrap(),
            "fixture-old-key"
        );
    }

    #[test]
    fn config_failure_never_deletes_an_existing_reauthentication_directory() {
        let _env = crate::test_support::test_env();
        account::write_key("fixture-existing", "fixture-candidate-key").unwrap();
        rollback_credentials("fixture-existing", None, true).unwrap();
        assert_eq!(
            account::load_key("fixture-existing").unwrap(),
            "fixture-candidate-key"
        );
    }
}
