// SPDX-License-Identifier: MPL-2.0

use super::support::test_app;
use crate::app::login::{CommandCodeLoginFlow, LoginFlow, cancel_login, start_login};
use crate::providers::command_code::login::{
    CommandCodeLoginEvent as Event, CommandCodeLoginStatus as Status,
};

#[test]
fn new_login_is_an_explicit_choice_without_reading_cli_credentials() {
    let _env = crate::test_support::test_env();
    let mut app = test_app();
    let config = app.config.clone();
    let task = start_login::<CommandCodeLoginFlow>(&mut app);
    assert_eq!(task.units(), 0);
    let state = app.command_code_login.as_ref().unwrap();
    assert_eq!(state.status, Status::Editing);
    assert!(state.api_key.is_empty());
    assert!(state.login_url.is_none());
    assert_eq!(app.config, config);
}

#[test]
fn stale_completion_cannot_change_the_current_login() {
    let _env = crate::test_support::test_env();
    let mut app = test_app();
    let _ = start_login::<CommandCodeLoginFlow>(&mut app);
    app.command_code_login.as_mut().unwrap().status = Status::Running;
    let _ = CommandCodeLoginFlow::on_event(
        &mut app,
        Event::Finished {
            flow_id: "stale-flow".to_string(),
            result: Box::new(Err("stale-error".to_string())),
        },
    );
    let state = app.command_code_login.as_ref().unwrap();
    assert_eq!(state.status, Status::Running);
    assert!(state.error.is_none());
}

#[test]
fn failed_validation_returns_to_editing_without_saving_credentials() {
    let _env = crate::test_support::test_env();
    let mut app = test_app();
    let _ = start_login::<CommandCodeLoginFlow>(&mut app);
    let state = app.command_code_login.as_mut().unwrap();
    state.status = Status::Running;
    let flow_id = state.flow_id.clone();
    let _ = CommandCodeLoginFlow::on_event(
        &mut app,
        Event::Finished {
            flow_id,
            result: Box::new(Err("Command Code login required".to_string())),
        },
    );
    let state = app.command_code_login.as_ref().unwrap();
    assert_eq!(state.status, Status::Editing);
    assert!(state.error.is_some());
    assert!(app.config.command_code_managed_accounts.is_empty());
    assert!(!crate::config::paths().command_code_accounts_dir.exists());
}

#[test]
fn canceled_login_ignores_late_events_and_keeps_existing_accounts() {
    let _env = crate::test_support::test_env();
    let mut app = test_app();
    app.config
        .selected_codex_account_ids
        .push("existing-codex".to_string());
    let config = app.config.clone();
    let _ = start_login::<CommandCodeLoginFlow>(&mut app);
    let flow_id = app.command_code_login.as_ref().unwrap().flow_id.clone();
    cancel_login::<CommandCodeLoginFlow>(&mut app);
    let task = CommandCodeLoginFlow::on_event(
        &mut app,
        Event::Finished {
            flow_id,
            result: Box::new(Err("late".to_string())),
        },
    );
    assert_eq!(task.units(), 0);
    assert!(app.command_code_login.is_none());
    assert_eq!(app.config, config);
}
