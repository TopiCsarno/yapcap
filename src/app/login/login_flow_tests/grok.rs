// SPDX-License-Identifier: MPL-2.0

use super::support::{grok_account, isolated_xdg, test_app};
use crate::app::login::{GrokLoginFlow, LoginFlow};
use crate::app::session::{cancel_login, start_login};
use crate::model::ProviderId;
use crate::providers::grok::{GrokLoginEvent, GrokLoginState, GrokLoginStatus, GrokLoginSuccess};

fn running_state(flow_id: &str) -> GrokLoginState {
    GrokLoginState {
        flow_id: flow_id.to_string(),
        status: GrokLoginStatus::Running,
        login_url: None,
        error: None,
        importing_from_host_cli: false,
    }
}

#[test]
fn grok_login_state_transitions_on_cancel() {
    let (_env, _root) = isolated_xdg("grok-cancel");
    let mut app = test_app();
    assert!(app.grok_login.is_none());

    let _ = start_login(&mut app, ProviderId::Grok);
    assert!(app.grok_login.is_some());
    let login = app.grok_login.as_ref().unwrap();
    assert_eq!(login.status, GrokLoginStatus::Running);
    assert!(app.grok_login_handle.is_some());

    cancel_login(&mut app, ProviderId::Grok);
    assert!(app.grok_login.is_none());
    assert!(app.grok_login_handle.is_none());
}

#[test]
fn on_event_finished_ok_applies_account_and_succeeds() {
    let (_env, _root) = isolated_xdg("grok-finished-ok");
    let mut app = test_app();
    app.grok_login = Some(running_state("flow"));

    let _ = GrokLoginFlow::on_event(
        &mut app,
        GrokLoginEvent::Finished {
            flow_id: "flow".to_string(),
            result: Box::new(Ok(GrokLoginSuccess {
                account: grok_account("new-grok"),
            })),
        },
    );

    assert!(app.grok_login.is_none());
    assert!(
        app.config
            .grok_managed_accounts
            .iter()
            .any(|account| account.id == "new-grok")
    );
    assert_eq!(
        app.config.selected_grok_account_ids,
        vec!["new-grok".to_string()]
    );
}

#[test]
fn on_event_finished_error_marks_failed_and_commits_nothing() {
    let (_env, _root) = isolated_xdg("grok-finished-err");
    let mut app = test_app();
    app.grok_login = Some(running_state("flow"));

    let _ = GrokLoginFlow::on_event(
        &mut app,
        GrokLoginEvent::Finished {
            flow_id: "flow".to_string(),
            result: Box::new(Err("network error".to_string())),
        },
    );

    let login = app.grok_login.as_ref().unwrap();
    assert_eq!(login.status, GrokLoginStatus::Failed);
    assert_eq!(login.error.as_deref(), Some("network error"));
    assert!(app.config.grok_managed_accounts.is_empty());
}

#[test]
fn on_event_ignores_mismatched_flow_id() {
    let (_env, _root) = isolated_xdg("grok-mismatch");
    let mut app = test_app();
    app.grok_login = Some(running_state("current"));

    let _ = GrokLoginFlow::on_event(
        &mut app,
        GrokLoginEvent::Finished {
            flow_id: "stale".to_string(),
            result: Box::new(Ok(GrokLoginSuccess {
                account: grok_account("stale-account"),
            })),
        },
    );

    assert_eq!(
        app.grok_login.as_ref().unwrap().status,
        GrokLoginStatus::Running
    );
    assert!(app.config.grok_managed_accounts.is_empty());
}

#[test]
fn on_event_updates_login_url() {
    let (_env, _root) = isolated_xdg("grok-login-url");
    let mut app = test_app();
    app.grok_login = Some(running_state("flow"));

    let _ = GrokLoginFlow::on_event(
        &mut app,
        GrokLoginEvent::LoginUrl {
            flow_id: "flow".to_string(),
            url: "https://auth.x.ai/oauth/authorize?test=1".to_string(),
        },
    );

    let login = app.grok_login.as_ref().unwrap();
    assert_eq!(
        login.login_url.as_deref(),
        Some("https://auth.x.ai/oauth/authorize?test=1")
    );
}
