// SPDX-License-Identifier: MPL-2.0

use super::tests::test_app;
use super::{AppModel, Message, ProviderId};
use crate::config::ManagedCopilotAccountConfig;
use crate::model::AuthState;
use crate::providers::registry;
use crate::refresh_owner::{RefreshOwner, RefreshOwnerAttempt};
use crate::shared_state::{ProviderRefreshRequest, RefreshRequestReason, SharedControlState};
use crate::test_support::{self, TestEnv};
use chrono::Utc;

const ACCOUNT_ID: &str = "copilot-1";

fn owner(name: &str) -> RefreshOwner {
    let lock_path = std::env::temp_dir().join(format!(
        "yapcap-reauth-sync-owner-{name}-{}.lock",
        std::process::id()
    ));
    match crate::refresh_owner::try_acquire(lock_path).unwrap() {
        RefreshOwnerAttempt::Owner(owner) => owner,
        RefreshOwnerAttempt::NonOwner(_) => panic!("test lock should be available"),
    }
}

fn app_with_action_required_account(owner: Option<RefreshOwner>) -> (TestEnv, AppModel) {
    let env = test_support::test_env();
    let mut app = test_app(owner);
    app.config.copilot_managed_accounts = vec![ManagedCopilotAccountConfig {
        id: ACCOUNT_ID.to_string(),
        label: "octocat".to_string(),
        github_user_id: 1,
        login: "octocat".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: Some(Utc::now()),
    }];
    registry::toggle_account_selection(ProviderId::Copilot, &mut app.config, ACCOUNT_ID);
    crate::runtime::reconcile_provider(
        &app.config,
        &app.detection,
        &mut app.state,
        ProviderId::Copilot,
    );
    let account = app
        .state
        .provider_accounts
        .iter_mut()
        .find(|account| account.provider == ProviderId::Copilot)
        .expect("copilot account in runtime state");
    account.auth_state = AuthState::ActionRequired;
    (env, app)
}

fn account_auth_state(app: &AppModel) -> AuthState {
    app.state
        .accounts_for(ProviderId::Copilot)
        .first()
        .expect("copilot account in runtime state")
        .auth_state
        .clone()
}

fn account_action_control(reauthenticated_account_ids: Vec<String>) -> SharedControlState {
    let mut control = SharedControlState::default();
    control.upsert_request(ProviderRefreshRequest {
        provider: ProviderId::Copilot,
        reason: RefreshRequestReason::AccountAction,
        requested_at: Utc::now(),
        requesting_process_id: "non-owner-process".to_string(),
        reauthenticated_account_ids,
    });
    control
}

fn runtime_account_id(app: &AppModel) -> String {
    app.state.accounts_for(ProviderId::Copilot)[0]
        .account_id
        .clone()
}

#[test]
fn owner_clears_action_required_for_account_reauthenticated_elsewhere() {
    let (_env, mut app) = app_with_action_required_account(Some(owner("reauth-elsewhere")));
    let account_id = runtime_account_id(&app);

    let task = app.handle_message(Message::UpdateSharedControl(
        Box::new(account_action_control(vec![account_id])),
        vec!["requests"],
    ));

    assert!(task.units() > 0);
    assert_eq!(account_auth_state(&app), AuthState::Ready);
    assert!(
        app.state
            .provider(ProviderId::Copilot)
            .unwrap()
            .is_refreshing
    );
}

#[test]
fn owner_keeps_action_required_without_reauthenticated_account() {
    let (_env, mut app) = app_with_action_required_account(Some(owner("plain-account-action")));

    let task = app.handle_message(Message::UpdateSharedControl(
        Box::new(account_action_control(Vec::new())),
        vec!["requests"],
    ));

    assert_eq!(task.units(), 0);
    assert_eq!(account_auth_state(&app), AuthState::ActionRequired);
}

#[test]
fn non_owner_reauth_refresh_request_names_account() {
    let (_env, mut app) = app_with_action_required_account(None);
    let account_id = runtime_account_id(&app);

    let _ = app.request_provider_refresh_after_reauth(ProviderId::Copilot, account_id.clone());

    let request = app
        .shared_control
        .requests
        .iter()
        .find(|request| request.provider == ProviderId::Copilot)
        .expect("copilot refresh request");
    assert_eq!(request.reason, RefreshRequestReason::AccountAction);
    assert_eq!(request.reauthenticated_account_ids, vec![account_id]);
}

#[test]
fn later_refresh_request_keeps_pending_reauthenticated_account() {
    let mut control = account_action_control(vec![ACCOUNT_ID.to_string()]);

    control.upsert_request(ProviderRefreshRequest {
        provider: ProviderId::Copilot,
        reason: RefreshRequestReason::ProviderSelected,
        requested_at: Utc::now(),
        requesting_process_id: "other-process".to_string(),
        reauthenticated_account_ids: Vec::new(),
    });

    assert_eq!(control.requests.len(), 1);
    assert_eq!(
        control.requests[0].reauthenticated_account_ids,
        vec![ACCOUNT_ID.to_string()]
    );
}

#[test]
fn control_document_without_reauth_field_still_loads() {
    let raw = r#"{
        "document_version": 1,
        "generation": 3,
        "updated_at": "2026-11-23T00:00:00Z",
        "requests": [{
            "provider": "copilot",
            "reason": "account_action",
            "requested_at": "2026-11-23T00:00:00Z",
            "requesting_process_id": "old-process"
        }]
    }"#;

    let control: SharedControlState = serde_json::from_str(raw).unwrap();

    assert!(control.requests[0].reauthenticated_account_ids.is_empty());
}
