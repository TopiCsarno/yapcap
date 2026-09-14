use super::applet::{
    AppletBarLayout, applet_bar_layout, applet_bar_width, applet_button_size,
    applet_fallback_button_size, applet_percent_cell_alignment, applet_percent_cell_width,
    applet_percent_text, panel_button_size, panel_cells, select_provider,
};
use super::popup_view::{
    account_page_next, account_page_previous, clamp_account_page, pager_account_label,
    provider_viewport, provider_viewport_navigation_visible,
};
use super::refresh::should_refresh_account_statuses;
use super::{
    APPLET_ICON_GAP, AppModel, AppState, Config, LaunchMode, Message, PanelIconStyle, PopupRoute,
    ProviderId, UsageAmountFormat, automatic_refresh_poll_interval, format_retry_delay,
    popup_size_limits, update_retry_delay,
};
use crate::account_storage::{NewProviderAccount, ProviderAccountStorage, ProviderAccountTokens};
use crate::config::{
    ManagedClaudeAccountConfig, ManagedCodexAccountConfig, ManagedCopilotAccountConfig,
    ManagedCursorAccountConfig, ManagedGeminiAccountConfig, ManagedKimiAccountConfig,
    ManagedMinimaxAccountConfig, ManagedZaiAccountConfig,
};
use crate::model::{
    AccountSelectionStatus, ProviderAccountRuntimeState, ProviderIdentity, ProviderRuntimeState,
    UsageHeadline, UsageSnapshot, UsageWindow,
};
use crate::providers::cursor::CursorScanState;
use crate::refresh_owner::{ProcessInfo, RefreshOwner, RefreshOwnerAttempt};
use crate::shared_state::{
    ProviderRefreshRequest, RefreshRequestReason, SharedControlState, SharedRuntimeState,
};
use crate::updates::UpdateStatus;
use chrono::Utc;
use std::path::PathBuf;
use std::time::Duration;

#[test]
fn update_retry_delay_backs_off_to_cap() {
    assert_eq!(update_retry_delay(1), Duration::from_secs(15));
    assert_eq!(update_retry_delay(2), Duration::from_secs(30));
    assert_eq!(update_retry_delay(7), Duration::from_secs(15 * 60));
    assert_eq!(update_retry_delay(20), Duration::from_secs(15 * 60));
}

#[test]
fn retry_delay_format_is_compact() {
    assert_eq!(format_retry_delay(Duration::from_secs(15)), "15s");
    assert_eq!(format_retry_delay(Duration::from_secs(60)), "1m");
    assert_eq!(format_retry_delay(Duration::from_secs(75)), "1m 15s");
}

#[test]
fn automatic_refresh_poll_checks_more_often_than_default_refresh_interval() {
    assert_eq!(automatic_refresh_poll_interval(), Duration::from_secs(10));
}

#[test]
fn popup_size_limits_allow_tall_account_details() {
    assert_eq!(popup_size_limits().max().height, 1200.0);
}

#[test]
fn owner_tick_runs_automatic_refresh() {
    let owner = refresh_owner("owner-tick");
    let mut app = test_app(Some(owner));
    ready_configured_copilot_provider(&mut app);

    let _task = app.handle_message(Message::Tick);

    assert!(
        app.state
            .provider(ProviderId::Copilot)
            .unwrap()
            .is_refreshing
    );
}

#[test]
fn non_owner_tick_does_not_run_automatic_refresh() {
    let mut app = test_app(None);
    ready_selected_provider(&mut app.state, ProviderId::Codex);

    let _task = app.handle_message(Message::Tick);

    assert!(!app.state.provider(ProviderId::Codex).unwrap().is_refreshing);
}

#[test]
fn owner_tick_skips_disabled_provider() {
    let owner = refresh_owner("owner-disabled");
    let mut app = test_app(Some(owner));
    app.config.cursor_enablement = crate::config::ProviderEnablement::Disabled;
    app.state.provider_mut(ProviderId::Cursor).unwrap().enabled = false;
    ready_selected_provider(&mut app.state, ProviderId::Cursor);

    let _task = app.handle_message(Message::Tick);

    let cursor = app.state.provider(ProviderId::Cursor).unwrap();
    assert!(!cursor.is_refreshing);
}

#[test]
fn non_owner_refresh_now_writes_shared_control_requests_without_refreshing() {
    let mut app = test_app(None);
    ready_selected_provider(&mut app.state, ProviderId::Codex);

    let task = app.handle_message(Message::RefreshNow);

    assert_eq!(task.units(), 0);
    assert_eq!(app.shared_control.requests.len(), ProviderId::ALL.len());
    assert!(
        app.shared_control
            .requests
            .iter()
            .all(|request| request.reason == RefreshRequestReason::User)
    );
    assert!(!app.state.provider(ProviderId::Codex).unwrap().is_refreshing);
}

#[test]
fn provider_viewport_moves_one_provider_without_changing_the_selected_provider() {
    let mut app = test_app(None);
    for provider in ProviderId::ALL {
        app.state.provider_mut(provider).unwrap().enabled = true;
    }

    let _ = app.handle_message(Message::PageProviderViewport(super::PagerDirection::Next));

    assert_eq!(app.provider_viewport_offset, 1);
    assert_eq!(app.selected_provider, ProviderId::Codex);

    let _ = app.handle_message(Message::PageProviderViewport(
        super::PagerDirection::Previous,
    ));

    assert_eq!(app.provider_viewport_offset, 0);
}

#[test]
fn provider_viewport_stops_at_each_end_without_wrapping() {
    let mut app = test_app(None);
    for provider in ProviderId::ALL {
        app.state.provider_mut(provider).unwrap().enabled = true;
    }

    for _ in 0..ProviderId::ALL.len() {
        let _ = app.handle_message(Message::PageProviderViewport(super::PagerDirection::Next));
    }

    assert_eq!(app.provider_viewport_offset, ProviderId::ALL.len() - 6);
    assert_eq!(app.selected_provider, ProviderId::Codex);

    let _ = app.handle_message(Message::PageProviderViewport(
        super::PagerDirection::Previous,
    ));

    assert_eq!(app.provider_viewport_offset, ProviderId::ALL.len() - 7);
}

#[test]
fn selecting_a_provider_preserves_the_viewport_offset() {
    let mut app = test_app(None);
    for provider in ProviderId::ALL {
        app.state.provider_mut(provider).unwrap().enabled = true;
    }
    app.provider_viewport_offset = 2;

    let _ = app.handle_message(Message::SelectProvider(ProviderId::Gemini));

    assert_eq!(app.selected_provider, ProviderId::Gemini);
    assert_eq!(app.provider_viewport_offset, 2);
}

#[test]
fn refresh_now_excludes_disabled_providers_from_requests() {
    let mut app = test_app(None);
    app.config.claude_enablement = crate::config::ProviderEnablement::Disabled;
    app.state.provider_mut(ProviderId::Claude).unwrap().enabled = false;

    let _task = app.handle_message(Message::RefreshNow);

    assert!(
        !app.shared_control
            .requests
            .iter()
            .any(|request| request.provider == ProviderId::Claude)
    );
}

#[test]
fn owner_observing_shared_control_runs_requested_refresh() {
    let owner = refresh_owner("owner-request");
    let mut app = test_app(Some(owner));
    ready_configured_copilot_provider(&mut app);

    let task = app.handle_message(Message::UpdateSharedControl(
        Box::new(control_request(ProviderId::Copilot)),
        vec!["requests"],
    ));

    assert!(task.units() > 0);
    assert!(
        app.state
            .provider(ProviderId::Copilot)
            .unwrap()
            .is_refreshing
    );
}

#[test]
fn owner_ignores_duplicate_request_for_refreshing_provider() {
    let owner = refresh_owner("owner-duplicate");
    let mut app = test_app(Some(owner));
    ready_selected_provider(&mut app.state, ProviderId::Codex);
    app.state
        .provider_mut(ProviderId::Codex)
        .unwrap()
        .is_refreshing = true;

    let task = app.handle_message(Message::UpdateSharedControl(
        Box::new(control_request(ProviderId::Codex)),
        vec!["requests"],
    ));

    assert_eq!(task.units(), 0);
    assert!(app.state.provider(ProviderId::Codex).unwrap().is_refreshing);
}

#[test]
fn owner_consumes_request_for_not_ready_provider() {
    let _env = crate::test_support::test_env();
    let owner = refresh_owner("owner-not-ready-request");
    let mut app = test_app(Some(owner));

    let task = app.handle_message(Message::UpdateSharedControl(
        Box::new(control_request(ProviderId::Codex)),
        vec!["requests"],
    ));

    assert_eq!(task.units(), 0);
    assert!(app.shared_control.requests.is_empty());
    assert!(!app.state.provider(ProviderId::Codex).unwrap().is_refreshing);
}

#[test]
fn owner_ignores_stale_shared_control_update() {
    let owner = refresh_owner("owner-stale-control");
    let mut app = test_app(Some(owner));
    ready_selected_provider(&mut app.state, ProviderId::Cursor);
    app.shared_control.generation = 2;

    let stale = control_request(ProviderId::Cursor);
    let task = app.handle_message(Message::UpdateSharedControl(
        Box::new(stale),
        vec!["requests"],
    ));

    assert_eq!(task.units(), 0);
    assert_eq!(app.shared_control.generation, 2);
    assert!(app.shared_control.requests.is_empty());
    assert!(
        !app.state
            .provider(ProviderId::Cursor)
            .unwrap()
            .is_refreshing
    );
}

#[test]
fn owner_ignores_same_generation_conflicting_shared_control_update() {
    let owner = refresh_owner("owner-same-generation-control");
    let mut app = test_app(Some(owner));
    ready_selected_provider(&mut app.state, ProviderId::Cursor);
    app.shared_control.generation = 1;

    let same_generation = control_request(ProviderId::Cursor);
    let task = app.handle_message(Message::UpdateSharedControl(
        Box::new(same_generation),
        vec!["requests"],
    ));

    assert_eq!(task.units(), 0);
    assert_eq!(app.shared_control.generation, 1);
    assert!(app.shared_control.requests.is_empty());
    assert!(
        !app.state
            .provider(ProviderId::Cursor)
            .unwrap()
            .is_refreshing
    );
}

#[test]
fn shared_control_metadata_notification_does_not_apply_partial_document() {
    let owner = refresh_owner("owner-partial-control");
    let mut app = test_app(Some(owner));
    let partial = control_request(ProviderId::Codex);

    let task = app.handle_message(Message::UpdateSharedControl(
        Box::new(partial),
        vec!["generation"],
    ));

    assert_eq!(task.units(), 0);
    assert_eq!(app.shared_control.generation, 0);
    assert!(app.shared_control.requests.is_empty());
}

#[test]
fn shared_refresh_evaluation_lists_unique_requesters() {
    let mut control = SharedControlState::default();
    control.upsert_request(refresh_request(ProviderId::Codex, "process-a"));
    control.upsert_request(refresh_request(ProviderId::Claude, "process-b"));
    control.upsert_request(refresh_request(ProviderId::Cursor, "process-a"));

    let evaluation = super::SharedRefreshEvaluationLog::from_requests(&control);

    assert_eq!(evaluation.requesters(), "process-a,process-b");
}

#[test]
fn shared_runtime_metadata_notification_does_not_apply_partial_document() {
    let mut app = test_app(None);
    let initial_state = app.state.clone();
    let mut partial_state = initial_state.clone();
    partial_state.provider_mut(ProviderId::Codex).unwrap().error = Some("partial".to_string());

    let task = app.handle_message(Message::UpdateSharedRuntime(
        Box::new(SharedRuntimeState::new(partial_state, 1)),
        vec!["generation"],
    ));

    assert_eq!(task.units(), 0);
    assert_eq!(app.state, initial_state);
}

#[test]
fn shared_runtime_update_preserves_refreshing_provider() {
    let _guard = crate::test_support::env_lock();
    let mut app = test_app(None);
    app.config.codex_enablement = crate::config::ProviderEnablement::Enabled;
    let mut shared_state = app.state.clone();
    shared_state
        .provider_mut(ProviderId::Codex)
        .unwrap()
        .is_refreshing = true;

    let task = app.handle_message(Message::UpdateSharedRuntime(
        Box::new(SharedRuntimeState::new(shared_state, 1)),
        Vec::new(),
    ));

    assert_eq!(task.units(), 0);
    assert!(app.state.provider(ProviderId::Codex).unwrap().is_refreshing);
}

#[test]
fn owner_refresh_now_processes_new_shared_control_snapshot() {
    let _guard = crate::test_support::env_lock();
    let owner = refresh_owner("owner-refresh-now-new-control");
    let mut app = test_app(Some(owner));
    ready_configured_copilot_provider(&mut app);

    let task = app.handle_message(Message::RefreshNow);

    assert!(task.units() > 0);
    assert!(
        app.state
            .provider(ProviderId::Copilot)
            .unwrap()
            .is_refreshing
    );
}

#[test]
fn provider_refresh_completion_consumes_shared_control_request() {
    let owner = refresh_owner("owner-consume-request");
    let mut app = test_app(Some(owner));
    ready_configured_copilot_provider(&mut app);
    app.shared_control = control_request(ProviderId::Codex);
    app.refresh_batches.push(super::ProviderRefreshBatch {
        provider: ProviderId::Codex,
        token: 1,
        started_at: Utc::now(),
        pending_account_ids: vec!["default".to_string()],
        request: app.shared_control.requests.first().cloned(),
    });
    app.state.begin_provider_refresh(ProviderId::Codex);
    let provider = ProviderRuntimeState {
        provider: ProviderId::Codex,
        enabled: true,
        selected_account_ids: vec!["default".to_string()],
        active_account_id: Some("default".to_string()),
        system_active_account_id: None,
        account_status: AccountSelectionStatus::Ready,
        is_refreshing: false,
        refresh_started_at: None,
        legacy_display_snapshot: None,
        error: None,
    };

    let _task = app.handle_message(Message::ProviderRefreshed(Box::new(
        crate::runtime::ProviderRefreshResult {
            batch_token: 1,
            account_id: "default".to_string(),
            provider,
            accounts: Vec::new(),
        },
    )));

    assert!(app.shared_control.requests.is_empty());
}

#[test]
fn refresh_outcomes_merge_without_reverting_newer_radio_selection() {
    let mut app = test_app(None);
    ready_two_copilot_accounts(&mut app);
    begin_test_batch(&mut app, 1, &["a", "b"]);

    let _ = app.handle_message(Message::ToggleAccountSelection(
        ProviderId::Copilot,
        "b".to_string(),
    ));
    let _ = app.handle_message(Message::ProviderRefreshed(Box::new(refresh_result(
        &app,
        1,
        "a",
        "refreshed-a",
    ))));
    assert_eq!(app.config.selected_copilot_account_ids, ["b"]);
    assert_eq!(
        app.state
            .active_account(ProviderId::Copilot)
            .unwrap()
            .account_id,
        "b"
    );
    assert!(
        app.state
            .provider(ProviderId::Copilot)
            .unwrap()
            .is_refreshing
    );

    let _ = app.handle_message(Message::ProviderRefreshed(Box::new(refresh_result(
        &app,
        1,
        "b",
        "refreshed-b",
    ))));
    assert_eq!(app.config.selected_copilot_account_ids, ["b"]);
    assert_eq!(
        app.state.account(ProviderId::Copilot, "a").unwrap().label,
        "refreshed-a"
    );
    assert_eq!(
        app.state.account(ProviderId::Copilot, "b").unwrap().label,
        "refreshed-b"
    );
    assert!(
        !app.state
            .provider(ProviderId::Copilot)
            .unwrap()
            .is_refreshing
    );
}

#[test]
fn reverse_refresh_outcomes_preserve_newer_radio_selection() {
    let mut app = test_app(None);
    ready_two_copilot_accounts(&mut app);
    begin_test_batch(&mut app, 1, &["a", "b"]);

    let _ = app.handle_message(Message::ToggleAccountSelection(
        ProviderId::Copilot,
        "b".to_string(),
    ));
    let _ = app.handle_message(Message::ProviderRefreshed(Box::new(refresh_result(
        &app,
        1,
        "b",
        "refreshed-b",
    ))));
    let _ = app.handle_message(Message::ProviderRefreshed(Box::new(refresh_result(
        &app,
        1,
        "a",
        "refreshed-a",
    ))));

    assert_eq!(app.config.selected_copilot_account_ids, ["b"]);
    assert_eq!(
        app.state
            .active_account(ProviderId::Copilot)
            .unwrap()
            .account_id,
        "b"
    );
    assert_eq!(
        app.state.account(ProviderId::Copilot, "a").unwrap().label,
        "refreshed-a"
    );
    assert_eq!(
        app.state.account(ProviderId::Copilot, "b").unwrap().label,
        "refreshed-b"
    );
}

#[test]
fn refresh_batch_ignores_duplicate_and_stale_outcomes() {
    let mut app = test_app(None);
    ready_two_copilot_accounts(&mut app);
    begin_test_batch(&mut app, 1, &["a", "b"]);
    let started_at = app
        .state
        .provider(ProviderId::Copilot)
        .unwrap()
        .refresh_started_at;

    let _ = app.handle_message(Message::ProviderRefreshed(Box::new(refresh_result(
        &app, 1, "a", "first",
    ))));
    let _ = app.handle_message(Message::ProviderRefreshed(Box::new(refresh_result(
        &app,
        1,
        "a",
        "duplicate",
    ))));
    assert_eq!(
        app.state.account(ProviderId::Copilot, "a").unwrap().label,
        "first"
    );
    assert_eq!(
        app.state
            .provider(ProviderId::Copilot)
            .unwrap()
            .refresh_started_at,
        started_at
    );
    assert!(
        app.state
            .provider(ProviderId::Copilot)
            .unwrap()
            .is_refreshing
    );

    app.state.finish_provider_refresh(ProviderId::Copilot);
    let _ = app.terminate_refresh_batch(ProviderId::Copilot);
    begin_test_batch(&mut app, 2, &["b"]);
    let _ = app.handle_message(Message::ProviderRefreshed(Box::new(refresh_result(
        &app, 1, "b", "late",
    ))));
    assert_eq!(
        app.state.account(ProviderId::Copilot, "b").unwrap().label,
        "b"
    );
    assert!(
        app.state
            .provider(ProviderId::Copilot)
            .unwrap()
            .is_refreshing
    );
}

#[test]
fn owner_shared_runtime_update_restores_local_batch_marker_and_blocks_rescheduling() {
    let owner = refresh_owner("owner-local-batch-marker");
    let mut app = test_app(Some(owner));
    ready_two_copilot_accounts(&mut app);
    begin_test_batch(&mut app, 1, &["a", "b"]);
    let started_at = app
        .state
        .provider(ProviderId::Copilot)
        .unwrap()
        .refresh_started_at;
    let mut incoming = app.state.clone();
    incoming
        .provider_mut(ProviderId::Copilot)
        .unwrap()
        .is_refreshing = false;
    incoming
        .provider_mut(ProviderId::Copilot)
        .unwrap()
        .refresh_started_at = None;

    let _ = app.handle_message(Message::UpdateSharedRuntime(
        Box::new(SharedRuntimeState::new(incoming, 1)),
        vec!["app_state"],
    ));
    let task = app.handle_message(Message::UpdateSharedControl(
        Box::new(control_request(ProviderId::Copilot)),
        vec!["requests"],
    ));

    assert_eq!(task.units(), 0);
    assert!(
        app.state
            .provider(ProviderId::Copilot)
            .unwrap()
            .is_refreshing
    );
    assert_eq!(
        app.state
            .provider(ProviderId::Copilot)
            .unwrap()
            .refresh_started_at,
        started_at
    );
    assert_eq!(app.refresh_batches.len(), 1);
    assert_eq!(app.shared_control.requests.len(), 1);
}

#[test]
fn flagged_candidate_refreshes_when_radio_state_is_login_required() {
    let owner = refresh_owner("flagged-login-required-refresh");
    let mut app = test_app(Some(owner));
    ready_two_copilot_accounts(&mut app);
    app.config.panel_copilot_account_ids = vec!["b".to_string()];
    app.state
        .provider_mut(ProviderId::Copilot)
        .unwrap()
        .account_status = AccountSelectionStatus::LoginRequired;

    let task = app.automatic_refresh_task();

    assert_eq!(task.units(), 2);
    assert_eq!(app.refresh_batches[0].pending_account_ids, ["a", "b"]);
    assert!(
        app.state
            .provider(ProviderId::Copilot)
            .unwrap()
            .is_refreshing
    );
}

#[test]
fn empty_selection_is_adopted_before_refresh_outcomes() {
    let _env = crate::test_support::test_env();
    let owner = refresh_owner("empty-selection-refresh");
    let mut app = test_app(Some(owner));
    app.config.copilot_enablement = crate::config::ProviderEnablement::Enabled;
    app.config.copilot_managed_accounts =
        vec![copilot_account("a", "a"), copilot_account("b", "b")];
    app.config.panel_copilot_account_ids = vec!["a".to_string(), "b".to_string()];
    runtime_reconcile_provider(&app.config, &mut app.state, ProviderId::Copilot);
    assert_eq!(
        app.state
            .provider(ProviderId::Copilot)
            .unwrap()
            .account_status,
        AccountSelectionStatus::SelectionRequired
    );

    let task = app.automatic_refresh_task();

    assert!(task.units() > 0);
    assert_eq!(app.config.selected_copilot_account_ids, ["a"]);
    assert_eq!(app.refresh_batches[0].pending_account_ids, ["a", "b"]);
}

#[test]
fn singleton_account_is_adopted_before_dispatch() {
    let _env = crate::test_support::test_env();
    let owner = refresh_owner("singleton-selection-refresh");
    let mut app = test_app(Some(owner));
    app.config.copilot_enablement = crate::config::ProviderEnablement::Enabled;
    app.config.copilot_managed_accounts = vec![copilot_account("only", "only")];
    runtime_reconcile_provider(&app.config, &mut app.state, ProviderId::Copilot);
    app.config.selected_copilot_account_ids.clear();
    app.state
        .provider_mut(ProviderId::Copilot)
        .unwrap()
        .selected_account_ids
        .clear();
    app.state
        .provider_mut(ProviderId::Copilot)
        .unwrap()
        .account_status = AccountSelectionStatus::SelectionRequired;

    let task = app.automatic_refresh_task();

    assert!(task.units() > 0);
    assert_eq!(app.config.selected_copilot_account_ids, ["only"]);
    assert_eq!(app.refresh_batches[0].pending_account_ids, ["only"]);
}

#[test]
fn action_required_flag_does_not_make_a_fresh_selected_account_due() {
    let mut app = test_app(None);
    ready_two_copilot_accounts(&mut app);
    app.config.panel_copilot_account_ids = vec!["b".to_string()];
    let selected = app
        .state
        .provider_accounts
        .iter_mut()
        .find(|account| account.provider == ProviderId::Copilot && account.account_id == "a")
        .unwrap();
    selected.last_success_at = Some(Utc::now());
    let flagged = app
        .state
        .provider_accounts
        .iter_mut()
        .find(|account| account.provider == ProviderId::Copilot && account.account_id == "b")
        .unwrap();
    flagged.auth_state = crate::model::AuthState::ActionRequired;
    flagged.last_success_at = None;

    assert!(!super::provider_refresh_due(
        &app.config,
        &app.state,
        ProviderId::Copilot
    ));
}

#[test]
fn config_update_applies_selected_provider_without_changing_popup_route() {
    let mut app = test_app(None);
    app.popup = Some(cosmic::iced::window::Id::unique());
    app.popup_route = PopupRoute::Settings;
    let mut config = app.config.clone();
    config.selected_provider = ProviderId::Claude;
    config.claude_enablement = crate::config::ProviderEnablement::Enabled;

    let _task = app.handle_message(Message::UpdateConfig(
        Box::new(config),
        vec!["selected_provider", "claude_enablement"],
    ));

    assert_eq!(app.selected_provider, ProviderId::Claude);
    assert_eq!(app.popup_route, PopupRoute::Settings);
    assert!(app.popup.is_some());
}

#[test]
fn provider_management_routes_are_explicit_and_account_scoped() {
    let mut app = test_app(None);

    let _task = app.handle_message(Message::NavigateTo(PopupRoute::ManageProviders));
    assert_eq!(app.popup_route, PopupRoute::ManageProviders);

    let _task = app.handle_message(Message::NavigateTo(PopupRoute::ManageAccounts(
        ProviderId::Copilot,
    )));
    assert_eq!(
        app.popup_route,
        PopupRoute::ManageAccounts(ProviderId::Copilot)
    );
}

#[test]
fn about_route_returns_to_provider_detail() {
    let mut app = test_app(None);

    let _task = app.handle_message(Message::NavigateTo(PopupRoute::About));
    assert_eq!(app.popup_route, PopupRoute::About);

    let _task = app.handle_message(Message::NavigateTo(PopupRoute::ProviderDetail));
    assert_eq!(app.popup_route, PopupRoute::ProviderDetail);
}

#[test]
fn global_settings_messages_preserve_each_configuration_control() {
    let mut app = test_app(None);

    let _task = app.handle_message(Message::SetRefreshInterval(900));
    let _task = app.handle_message(Message::SetPanelIconStyle(PanelIconStyle::PercentOnly));
    let _task = app.handle_message(Message::SetResetTimeFormat(
        crate::config::ResetTimeFormat::Absolute,
    ));
    let _task = app.handle_message(Message::SetUsageAmountFormat(UsageAmountFormat::Left));

    assert_eq!(app.config.refresh_interval_seconds, 900);
    assert_eq!(app.config.panel_icon_style, PanelIconStyle::PercentOnly);
    assert_eq!(
        app.config.reset_time_format,
        crate::config::ResetTimeFormat::Absolute
    );
    assert_eq!(app.config.usage_amount_format, UsageAmountFormat::Left);
}

#[test]
fn update_check_messages_keep_current_error_and_available_states() {
    let mut app = test_app(None);

    let _task = app.handle_message(Message::UpdateChecked {
        status: UpdateStatus::NoUpdate,
        attempt: 0,
    });
    assert_eq!(app.update_status, UpdateStatus::NoUpdate);

    let available = UpdateStatus::UpdateAvailable {
        version: "9.9.9".to_string(),
        url: "https://example.invalid/release".to_string(),
    };
    let _task = app.handle_message(Message::UpdateChecked {
        status: available.clone(),
        attempt: 0,
    });
    assert_eq!(app.update_status, available);

    let _task = app.handle_message(Message::UpdateChecked {
        status: UpdateStatus::Error("offline".to_string()),
        attempt: 0,
    });
    assert!(
        matches!(&app.update_status, UpdateStatus::Error(reason) if reason.contains("offline"))
    );

    let _task = app.handle_message(Message::CheckUpdates);
    assert_eq!(app.update_status, UpdateStatus::Unchecked);
}

#[test]
fn demo_startup_keeps_update_check_task() {
    let startup = super::startup_task(
        true,
        cosmic::app::Task::none(),
        cosmic::app::Task::done(cosmic::Action::App(Message::Tick)),
        cosmic::app::Task::none(),
        cosmic::app::Task::none(),
    );

    assert_eq!(startup.units(), 1);
}

#[test]
fn partial_config_update_preserves_locally_written_account() {
    let _env = crate::test_support::test_env();
    let mut app = test_app(None);
    app.config.codex_managed_accounts = vec![codex_account("codex-1")];
    app.config.selected_codex_account_ids = vec!["codex-1".to_string()];
    let partial_watcher_config = Config {
        selected_codex_account_ids: vec!["codex-1".to_string()],
        ..Config::default()
    };

    let _ = app.on_config_update(partial_watcher_config, &["selected_codex_account_ids"]);

    assert_eq!(app.config.codex_managed_accounts.len(), 1);
}

#[test]
fn selecting_stale_enabled_provider_without_configured_account_consumes_request() {
    let _env = crate::test_support::test_env();
    let mut app = test_app(None);
    ready_selected_provider(&mut app.state, ProviderId::Claude);
    selected_account_without_usage(&mut app.state, ProviderId::Claude);

    let task = app.handle_message(Message::SelectProvider(ProviderId::Claude));

    assert_eq!(task.units(), 0);
    assert_eq!(app.config.selected_provider, ProviderId::Claude);
    assert_eq!(app.selected_provider, ProviderId::Claude);
    assert!(app.shared_control.requests.is_empty());
}

#[test]
fn selecting_disabled_provider_does_not_request_refresh() {
    let mut app = test_app(None);
    app.config.cursor_enablement = crate::config::ProviderEnablement::Disabled;
    if let Some(cursor) = app.state.provider_mut(ProviderId::Cursor) {
        cursor.enabled = false;
    }

    let _task = app.handle_message(Message::SelectProvider(ProviderId::Cursor));

    assert!(app.shared_control.requests.is_empty());
}

#[test]
fn selecting_provider_preserves_local_popup_state() {
    let mut app = test_app(None);
    let popup = cosmic::iced::window::Id::unique();
    app.popup = Some(popup);
    app.popup_route = PopupRoute::ManageAccounts(ProviderId::Codex);
    ready_selected_provider(&mut app.state, ProviderId::Gemini);
    selected_account_without_usage(&mut app.state, ProviderId::Gemini);

    let _task = app.handle_message(Message::SelectProvider(ProviderId::Gemini));

    assert_eq!(app.popup, Some(popup));
    assert_eq!(
        app.popup_route,
        PopupRoute::ManageAccounts(ProviderId::Codex)
    );
}

#[test]
fn non_owner_account_selection_requests_owner_refresh_without_running_it() {
    let _env = crate::test_support::test_env();
    let mut app = test_app(None);
    app.config.copilot_managed_accounts = vec![copilot_account("copilot-1", "octocat")];
    runtime_reconcile_provider(&app.config, &mut app.state, ProviderId::Copilot);

    let task = app.handle_message(Message::ToggleAccountSelection(
        ProviderId::Copilot,
        "copilot-1".to_string(),
    ));

    assert_eq!(task.units(), 0);
    assert_eq!(
        app.config.selected_account_ids(ProviderId::Copilot),
        ["copilot-1"]
    );
    assert_eq!(app.shared_control.requests.len(), 1);
    assert_eq!(
        app.shared_control.requests[0].reason,
        RefreshRequestReason::AccountAction
    );
    assert!(
        !app.state
            .provider(ProviderId::Copilot)
            .unwrap()
            .is_refreshing
    );
}

#[test]
fn owner_account_selection_runs_requested_refresh() {
    let owner = refresh_owner("owner-account-selection");
    let mut app = test_app(Some(owner));
    app.config.copilot_managed_accounts = vec![copilot_account("copilot-1", "octocat")];
    runtime_reconcile_provider(&app.config, &mut app.state, ProviderId::Copilot);

    let task = app.handle_message(Message::ToggleAccountSelection(
        ProviderId::Copilot,
        "copilot-1".to_string(),
    ));

    assert!(task.units() > 0);
    assert!(
        app.state
            .provider(ProviderId::Copilot)
            .unwrap()
            .is_refreshing
    );
}

#[test]
fn cursor_reauthentication_starts_local_rescan() {
    let mut app = test_app(None);
    app.config
        .cursor_managed_accounts
        .push(cursor_account("cursor-1", "cursor@example.com"));

    let task = app.handle_message(Message::ReauthenticateAccount(
        ProviderId::Cursor,
        "cursor-managed:cursor-1".to_string(),
    ));

    assert!(task.units() > 0);
    assert!(matches!(app.cursor_scan, CursorScanState::Scanning));
}

#[test]
fn panel_flag_message_changes_only_flags_and_resyncs_bounds() {
    let _env = crate::test_support::test_env();
    let mut app = test_app(None);
    ready_two_copilot_accounts(&mut app);
    let original_config = app.config.clone();
    let original_state = app.state.clone();

    let task = app.handle_message(Message::ToggleAccountPanelFlag(
        ProviderId::Copilot,
        "b".to_string(),
    ));

    let mut flagged_config = original_config.clone();
    flagged_config.panel_copilot_account_ids = vec!["b".to_string()];
    assert_eq!(app.config, flagged_config);
    assert_eq!(app.state, original_state);
    assert_eq!(task.units(), 0);
    let (width, height) = panel_button_size(&app.core, &app.state, &app.config);
    assert_eq!(
        app.core.applet.suggested_bounds,
        Some(cosmic::iced::Size::new(width, height))
    );
    assert_eq!(app.shared_control.requests.len(), 1);
    assert_eq!(app.shared_control.requests[0].provider, ProviderId::Copilot);
    assert_eq!(
        app.shared_control.requests[0].reason,
        RefreshRequestReason::AccountAction
    );
    let requested = app.shared_control.clone();
    app.core.applet.suggested_bounds = None;

    let task = app.handle_message(Message::ToggleAccountPanelFlag(
        ProviderId::Copilot,
        "b".to_string(),
    ));

    assert_eq!(app.config, original_config);
    assert_eq!(app.state, original_state);
    assert_eq!(task.units(), 0);
    assert_eq!(app.shared_control, requested);
    let (width, height) = applet_fallback_button_size(&app.core);
    assert_eq!(
        app.core.applet.suggested_bounds,
        Some(cosmic::iced::Size::new(width, height))
    );
}

#[test]
fn panel_flag_message_requests_batch_safe_owner_refresh() {
    let _env = crate::test_support::test_env();
    let owner = refresh_owner("panel-flag-owner");
    let mut app = test_app(Some(owner));
    ready_two_copilot_accounts(&mut app);

    let task = app.handle_message(Message::ToggleAccountPanelFlag(
        ProviderId::Copilot,
        "b".to_string(),
    ));

    assert!(task.units() > 0);
    assert_eq!(app.config.panel_copilot_account_ids, ["b"]);
    assert_eq!(app.config.selected_copilot_account_ids, ["a"]);
    assert_eq!(
        app.state
            .provider(ProviderId::Copilot)
            .unwrap()
            .selected_account_ids,
        ["a"]
    );
    assert!(
        app.state
            .provider(ProviderId::Copilot)
            .unwrap()
            .is_refreshing
    );
    assert_eq!(app.refresh_batches.len(), 1);
    assert_eq!(app.refresh_batches[0].pending_account_ids, ["a", "b"]);
    let requested = app.shared_control.clone();

    let task = app.handle_message(Message::ToggleAccountPanelFlag(
        ProviderId::Copilot,
        "b".to_string(),
    ));

    assert_eq!(task.units(), 0);
    assert!(app.config.panel_copilot_account_ids.is_empty());
    assert_eq!(app.shared_control, requested);
    assert_eq!(app.refresh_batches[0].pending_account_ids, ["a", "b"]);
}

#[test]
fn panel_flag_message_preserves_demo_radio_selection() {
    let mut env = crate::test_support::test_env();
    env.set("YAPCAP_DEMO", "1");
    let mut app = test_app(None);
    crate::demo_env::apply_config(&mut app.config);
    crate::demo_env::apply(&app.config, &mut app.state);
    let selected_ids = app.config.selected_codex_account_ids.clone();
    let state = app.state.clone();
    let mut expected_flags = app.config.panel_codex_account_ids.clone();
    expected_flags.push("yapcap-demo:codex-free".to_string());

    let _ = app.handle_message(Message::ToggleAccountPanelFlag(
        ProviderId::Codex,
        "yapcap-demo:codex-free".to_string(),
    ));

    assert_eq!(app.config.panel_codex_account_ids, expected_flags);
    assert_eq!(app.config.selected_codex_account_ids, selected_ids);
    assert_eq!(app.state, state);
}

#[test]
fn panel_flag_message_does_not_select_when_non_owner_selection_is_empty() {
    let _env = crate::test_support::test_env();
    let mut app = test_app(None);
    ready_two_copilot_accounts(&mut app);
    app.config.selected_copilot_account_ids.clear();
    app.state
        .provider_mut(ProviderId::Copilot)
        .unwrap()
        .selected_account_ids
        .clear();

    let _ = app.handle_message(Message::ToggleAccountPanelFlag(
        ProviderId::Copilot,
        "b".to_string(),
    ));

    assert_eq!(app.config.panel_copilot_account_ids, ["b"]);
    assert!(app.config.selected_copilot_account_ids.is_empty());
    assert!(
        app.state
            .provider(ProviderId::Copilot)
            .unwrap()
            .selected_account_ids
            .is_empty()
    );
    assert_eq!(
        app.shared_control.requests[0].reason,
        RefreshRequestReason::AccountAction
    );
}

#[test]
fn panel_flag_disable_clears_flags_and_reenable_does_not_restore_them() {
    let _env = crate::test_support::test_env();
    let mut app = test_app(None);
    ready_two_copilot_accounts(&mut app);
    app.config.panel_copilot_account_ids = vec!["a".to_string(), "b".to_string()];
    app.config.panel_codex_account_ids = vec!["codex-1".to_string()];

    let _ = app.handle_message(Message::SetProviderEnabled(ProviderId::Copilot, false));

    assert!(app.config.panel_copilot_account_ids.is_empty());
    assert_eq!(app.config.panel_codex_account_ids, ["codex-1"]);
    assert!(!app.state.provider(ProviderId::Copilot).unwrap().enabled);
    let disabled_config = app.config.clone();
    let task = app.handle_message(Message::ToggleAccountPanelFlag(
        ProviderId::Copilot,
        "b".to_string(),
    ));
    assert_eq!(task.units(), 0);
    assert_eq!(app.config, disabled_config);
    assert!(app.shared_control.requests.is_empty());

    let _ = app.handle_message(Message::SetProviderEnabled(ProviderId::Copilot, true));

    assert!(app.state.provider(ProviderId::Copilot).unwrap().enabled);
    assert!(app.config.panel_copilot_account_ids.is_empty());
    assert_eq!(app.config.panel_codex_account_ids, ["codex-1"]);
    assert_eq!(app.config.selected_copilot_account_ids, ["a"]);
}

#[test]
fn demo_codex_account_selection_keeps_account_rows() {
    let mut env = crate::test_support::test_env();
    env.set("YAPCAP_DEMO", "1");
    let mut app = test_app(None);
    crate::demo_env::apply_config(&mut app.config);
    crate::demo_env::apply(&app.config, &mut app.state);

    let task = app.handle_message(Message::ToggleAccountSelection(
        ProviderId::Codex,
        "yapcap-demo:codex-free".to_string(),
    ));

    assert_eq!(task.units(), 0);
    assert_eq!(
        app.config.selected_account_ids(ProviderId::Codex),
        ["yapcap-demo:codex-free"]
    );
    assert_eq!(app.state.accounts_for(ProviderId::Codex).len(), 2);
    assert_eq!(
        app.state
            .provider(ProviderId::Codex)
            .unwrap()
            .selected_account_ids,
        ["yapcap-demo:codex-free"]
    );

    let task = app.handle_message(Message::ToggleAccountSelection(
        ProviderId::Codex,
        "yapcap-demo:codex-free".to_string(),
    ));

    assert_eq!(task.units(), 0);
    assert_eq!(
        app.config.selected_account_ids(ProviderId::Codex),
        ["yapcap-demo:codex-free"]
    );
    assert_eq!(app.state.accounts_for(ProviderId::Codex).len(), 2);
    assert_eq!(
        app.state
            .provider(ProviderId::Codex)
            .unwrap()
            .selected_account_ids,
        ["yapcap-demo:codex-free"]
    );
}

#[test]
fn non_owner_provider_disable_reconciles_locally_without_publishing_runtime() {
    let _env = crate::test_support::test_env();
    let mut app = test_app(None);
    app.config.copilot_managed_accounts = vec![copilot_account("copilot-1", "octocat")];
    app.config.selected_copilot_account_ids = vec!["copilot-1".to_string()];
    runtime_reconcile_provider(&app.config, &mut app.state, ProviderId::Copilot);

    let task = app.handle_message(Message::SetProviderEnabled(ProviderId::Copilot, false));

    assert_eq!(task.units(), 0);
    assert_eq!(
        app.config.provider_enablement(ProviderId::Copilot),
        crate::config::ProviderEnablement::Disabled
    );
    assert!(!app.state.provider(ProviderId::Copilot).unwrap().enabled);
    assert!(app.shared_control.requests.is_empty());
}

#[test]
fn select_provider_keeps_current_when_enabled() {
    let mut state = AppState::empty();
    for p in &mut state.providers {
        p.enabled = true;
    }
    assert_eq!(
        select_provider(ProviderId::Claude, &state),
        ProviderId::Claude
    );
}

#[test]
fn select_provider_falls_back_when_current_disabled() {
    let mut state = AppState::empty();
    for p in &mut state.providers {
        p.enabled = p.provider != ProviderId::Codex;
    }
    let selected = select_provider(ProviderId::Codex, &state);
    assert_ne!(selected, ProviderId::Codex);
}

#[test]
fn applet_button_size_uses_panel_icon_style() {
    let core = cosmic::Core::default();
    let (suggested_w, suggested_h) = core.applet.suggested_size(false);
    let (major_padding, minor_padding) = core.applet.suggested_padding(false);
    let horizontal_padding = if core.applet.is_horizontal() {
        major_padding
    } else {
        minor_padding
    };
    let compact_px = suggested_w.min(suggested_h);
    let logo_width = f32::from(compact_px.saturating_sub(8).max(11));
    let bar_width = applet_bar_width(suggested_w, suggested_h);
    let padding_width = f32::from(2 * horizontal_padding);
    let (logo_bars_width, height) = applet_button_size(&core, PanelIconStyle::LogoAndBars);
    let (bars_only_width, bars_only_height) = applet_button_size(&core, PanelIconStyle::BarsOnly);
    let (percent_width, percent_height) = applet_button_size(&core, PanelIconStyle::LogoAndPercent);
    let (percent_only_width, percent_only_height) =
        applet_button_size(&core, PanelIconStyle::PercentOnly);

    assert_eq!(bars_only_width, bar_width + padding_width);
    let cell_100 = applet_percent_cell_width();
    assert_eq!(percent_only_width, cell_100 + padding_width);
    assert_eq!(
        logo_bars_width,
        logo_width + APPLET_ICON_GAP + bar_width + padding_width
    );
    assert_eq!(
        percent_width,
        logo_width + APPLET_ICON_GAP + cell_100 + padding_width
    );
    assert_eq!(height, bars_only_height);
    assert_eq!(height, percent_height);
    assert_eq!(height, percent_only_height);
}

#[test]
fn panel_button_size_stays_fixed_with_multiple_stored_accounts() {
    let core = cosmic::Core::default();
    let mut config = Config {
        panel_codex_account_ids: vec!["codex-1".to_string()],
        ..Config::default()
    };
    let mut one_account = AppState::empty();
    one_account.upsert_account(ProviderAccountRuntimeState::empty(
        ProviderId::Codex,
        "codex-1",
        "Codex 1",
    ));
    let mut two_accounts = one_account.clone();
    two_accounts.upsert_account(ProviderAccountRuntimeState::empty(
        ProviderId::Codex,
        "codex-2",
        "Codex 2",
    ));

    for style in [
        PanelIconStyle::LogoAndBars,
        PanelIconStyle::BarsOnly,
        PanelIconStyle::LogoAndPercent,
        PanelIconStyle::PercentOnly,
    ] {
        config.panel_icon_style = style;
        assert_eq!(
            panel_button_size(&core, &one_account, &config),
            panel_button_size(&core, &two_accounts, &config)
        );
        assert_eq!(
            panel_button_size(&core, &one_account, &config),
            applet_button_size(&core, style)
        );
    }
}

#[test]
fn panel_cells_skip_unknown_flagged_accounts() {
    let state = AppState::empty();
    let config = Config {
        panel_codex_account_ids: vec!["unknown".to_string()],
        ..Config::default()
    };

    assert!(panel_cells(&state, &config).is_empty());
    let core = cosmic::Core::default();
    assert_eq!(
        panel_button_size(&core, &state, &config),
        applet_fallback_button_size(&core)
    );
}

#[test]
fn panel_cells_require_flags_even_when_accounts_are_selected() {
    let mut state = AppState::empty();
    state.upsert_account(ProviderAccountRuntimeState::empty(
        ProviderId::Codex,
        "codex-1",
        "Codex",
    ));

    state
        .provider_mut(ProviderId::Codex)
        .unwrap()
        .selected_account_ids = vec!["codex-1".to_string()];
    let mut config = Config::default();
    assert!(panel_cells(&state, &config).is_empty());
    let core = cosmic::Core::default();
    assert_eq!(
        panel_button_size(&core, &state, &config),
        applet_fallback_button_size(&core)
    );

    config.panel_codex_account_ids = vec!["codex-1".to_string()];
    let cells = panel_cells(&state, &config);
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].layout, AppletBarLayout::two_bar(0.0, 0.0));
}

#[test]
fn panel_cells_skip_disabled_providers() {
    let mut state = AppState::empty();
    state.upsert_provider(ProviderRuntimeState::disabled(ProviderId::Codex));
    state.upsert_account(ProviderAccountRuntimeState::empty(
        ProviderId::Codex,
        "codex-1",
        "Codex",
    ));

    let config = Config {
        panel_codex_account_ids: vec!["codex-1".to_string()],
        ..Config::default()
    };
    assert!(panel_cells(&state, &config).is_empty());
}

#[test]
fn panel_cells_follow_provider_and_account_order_not_flag_order() {
    let mut state = AppState::empty();
    let mut config = Config::default();
    for provider in ProviderId::ALL.into_iter().rev() {
        for id in ["second", "first"] {
            state.upsert_account(ProviderAccountRuntimeState::empty(provider, id, id));
        }
        *config.panel_account_ids_mut(provider) = vec![
            "first".to_string(),
            "unknown".to_string(),
            "second".to_string(),
            "first".to_string(),
        ];
    }

    let cells = panel_cells(&state, &config);
    let identities = cells
        .iter()
        .map(|cell| (cell.account.provider, cell.account.account_id.as_str()))
        .collect::<Vec<_>>();
    let expected = ProviderId::ALL
        .into_iter()
        .flat_map(|provider| [(provider, "second"), (provider, "first")])
        .collect::<Vec<_>>();
    assert_eq!(identities, expected);
}

#[test]
fn panel_button_size_grows_with_flagged_accounts() {
    let core = cosmic::Core::default();
    let state = state_with_account_percents(&[30.0, 60.0, 90.0]);
    let (major, minor) = core.applet.suggested_padding(false);
    let padding = f32::from(
        2 * if core.applet.is_horizontal() {
            major
        } else {
            minor
        },
    );
    for style in [
        PanelIconStyle::LogoAndBars,
        PanelIconStyle::BarsOnly,
        PanelIconStyle::LogoAndPercent,
        PanelIconStyle::PercentOnly,
    ] {
        let mut config = Config {
            panel_icon_style: style,
            ..Config::default()
        };
        let (single_width, height) = applet_button_size(&core, style);
        let step = single_width - padding + super::APPLET_CELL_SPACING;
        let mut expected_width = single_width;
        for id in ["codex-0", "codex-1", "codex-2"] {
            config.panel_codex_account_ids.push(id.to_string());
            assert_eq!(
                panel_button_size(&core, &state, &config),
                (expected_width, height)
            );
            expected_width += step;
        }
    }
}

#[test]
fn panel_suggested_bounds_track_flagged_cells_and_empty_fallback() {
    let mut app = test_app(None);
    app.state = state_with_account_percents(&[30.0, 60.0]);
    app.config.panel_codex_account_ids = vec!["codex-0".to_string(), "codex-1".to_string()];
    app.sync_panel_suggested_bounds();
    let (width, height) = panel_button_size(&app.core, &app.state, &app.config);
    assert_eq!(
        app.core.applet.suggested_bounds,
        Some(cosmic::iced::Size::new(width, height))
    );

    app.config.panel_codex_account_ids.clear();
    app.sync_panel_suggested_bounds();
    let (width, height) = applet_fallback_button_size(&app.core);
    assert_eq!(
        app.core.applet.suggested_bounds,
        Some(cosmic::iced::Size::new(width, height))
    );
}

#[test]
fn applet_fallback_button_size_is_icon_only() {
    let core = cosmic::Core::default();
    let (suggested_w, suggested_h) = core.applet.suggested_size(false);
    let (major_padding, minor_padding) = core.applet.suggested_padding(false);
    let (horizontal_padding, vertical_padding) = if core.applet.is_horizontal() {
        (major_padding, minor_padding)
    } else {
        (minor_padding, major_padding)
    };
    let icon_px = suggested_w.min(suggested_h);

    let (width, height) = applet_fallback_button_size(&core);

    assert_eq!(
        width,
        f32::from(icon_px) + f32::from(2 * horizontal_padding)
    );
    assert_eq!(height, f32::from(suggested_h + 2 * vertical_padding));
    let (bars_width, bars_height) = applet_button_size(&core, PanelIconStyle::LogoAndBars);
    assert!(width < bars_width);
    assert_eq!(height, bars_height);
}

#[test]
fn account_pager_wraps_and_clamps() {
    assert_eq!(account_page_next(1, 2), 0);
    assert_eq!(account_page_previous(0, 2), 1);
    assert_eq!(account_page_next(0, 0), 0);
    assert_eq!(account_page_previous(0, 0), 0);
    assert_eq!(clamp_account_page(3, 2), 1);
    assert_eq!(clamp_account_page(0, 0), 0);
}

#[test]
fn account_pager_uses_a_fallback_label() {
    assert!(pager_account_label(1, "").contains('2'));
    assert!(pager_account_label(1, "  ").contains('2'));
    assert_eq!(pager_account_label(1, "Personal"), "Personal");
}

#[test]
fn account_pager_selects_the_next_account() {
    let mut app = test_app(None);
    app.selected_provider = ProviderId::Copilot;
    app.config.copilot_managed_accounts = vec![
        copilot_account("copilot-1", "first"),
        copilot_account("copilot-2", "second"),
    ];
    app.config.selected_copilot_account_ids = vec!["copilot-1".to_string()];
    runtime_reconcile_provider(&app.config, &mut app.state, ProviderId::Copilot);
    app.state.provider_accounts[1].snapshot = Some(UsageSnapshot {
        provider: ProviderId::Copilot,
        source: "test".to_string(),
        updated_at: Utc::now(),
        headline: UsageHeadline(0),
        windows: vec![UsageWindow {
            label: "Weekly".to_string(),
            used_percent: 30.0,
            reset_at: None,
            window_seconds: None,
            reset_description: None,
            group: None,
        }],
        provider_cost: None,
        extra_usage: None,
        identity: ProviderIdentity::default(),
    });
    let _ = app.page_provider_account(super::PagerDirection::Next);

    assert_eq!(app.detail_account_page, 1);
    assert_eq!(app.config.selected_copilot_account_ids, ["copilot-2"]);
    assert_eq!(
        app.state
            .provider(ProviderId::Copilot)
            .unwrap()
            .selected_account_ids,
        ["copilot-2"]
    );
    assert_eq!(
        app.shared_control.requests[0].reason,
        RefreshRequestReason::AccountAction
    );
}

#[test]
fn provider_switch_restores_the_selected_account_page() {
    let _env = crate::test_support::test_env();
    let mut app = test_app(None);
    app.selected_provider = ProviderId::Copilot;
    app.config.copilot_managed_accounts = vec![
        copilot_account("copilot-1", "first"),
        copilot_account("copilot-2", "second"),
    ];
    runtime_reconcile_provider(&app.config, &mut app.state, ProviderId::Copilot);
    let _ = app.page_provider_account(super::PagerDirection::Next);

    let _ = app.handle_message(Message::SelectProvider(ProviderId::Claude));
    let _ = app.handle_message(Message::SelectProvider(ProviderId::Copilot));

    assert_eq!(
        app.state
            .provider(ProviderId::Copilot)
            .unwrap()
            .selected_account_ids,
        ["copilot-2"]
    );
    assert_eq!(app.detail_account_page, 1);
}

#[test]
fn provider_viewport_shifts_one_icon_and_stops_at_the_end() {
    let providers = ProviderId::ALL.to_vec();

    assert_eq!(provider_viewport(&providers, 0), &providers[..6]);
    assert_eq!(provider_viewport(&providers, 1), &providers[1..7]);
    assert_eq!(
        provider_viewport(&providers, 99),
        &providers[providers.len() - 6..]
    );
}

#[test]
fn provider_viewport_navigation_requires_scrollable_content() {
    assert!(!provider_viewport_navigation_visible(5));
    assert!(!provider_viewport_navigation_visible(6));
    assert!(provider_viewport_navigation_visible(7));
}

#[test]
fn applet_percent_cell_width_is_fixed_to_widest_normal_percent() {
    let expected =
        super::APPLET_PERCENT_CELL_HORIZONTAL_PAD + 6.0 * super::APPLET_PERCENT_GLYPH_WIDTH;

    assert_eq!(applet_percent_text(0.0), "0.0%");
    assert_eq!(applet_percent_text(86.5), "86.5%");
    assert_eq!(applet_percent_text(100.0), "100.0%");
    assert_eq!(applet_percent_cell_width(), expected);
}

#[test]
fn applet_percent_cells_left_align_text_in_fixed_slot() {
    assert_eq!(
        applet_percent_cell_alignment(),
        cosmic::iced::Alignment::Start
    );
}

#[test]
fn applet_percent_text_uses_one_decimal_through_100_percent() {
    assert_eq!(applet_percent_text(86.54), "86.5%");
    assert_eq!(applet_percent_text(100.0), "100.0%");
}

#[test]
fn panel_cells_use_first_panel_window_and_global_amount_format() {
    let mut state = AppState::empty();
    let mut account = ProviderAccountRuntimeState::empty(ProviderId::Codex, "codex-1", "Codex");
    account.snapshot = Some(UsageSnapshot {
        provider: ProviderId::Codex,
        source: "test".to_string(),
        updated_at: Utc::now(),
        headline: UsageHeadline(0),
        windows: vec![
            UsageWindow {
                label: "Session".to_string(),
                used_percent: 86.5,
                reset_at: None,
                window_seconds: None,
                reset_description: None,
                group: None,
            },
            UsageWindow {
                label: "Weekly".to_string(),
                used_percent: 42.0,
                reset_at: None,
                window_seconds: None,
                reset_description: None,
                group: None,
            },
        ],
        provider_cost: None,
        extra_usage: None,
        identity: ProviderIdentity::default(),
    });

    state
        .provider_mut(ProviderId::Codex)
        .unwrap()
        .selected_account_ids = vec!["codex-1".to_string()];
    state.upsert_account(account);

    let mut config = Config {
        panel_codex_account_ids: vec!["codex-1".to_string()],
        usage_amount_format: UsageAmountFormat::Used,
        ..Config::default()
    };
    let percents_used = panel_cells(&state, &config)[0].layout;
    assert_eq!(percents_used.primary, 86.5);
    assert_eq!(percents_used.secondary, Some(42.0));

    config.usage_amount_format = UsageAmountFormat::Left;
    let percents_left = panel_cells(&state, &config)[0].layout;
    assert_eq!(percents_left.primary, 13.5);
}

#[test]
fn applet_bar_layout_preserves_single_bar_shape() {
    let snapshot = UsageSnapshot {
        provider: ProviderId::Copilot,
        source: "test".to_string(),
        updated_at: Utc::now(),
        headline: UsageHeadline(0),
        windows: vec![UsageWindow {
            label: "premium_interactions".to_string(),
            used_percent: 37.5,
            reset_at: None,
            window_seconds: None,
            reset_description: None,
            group: None,
        }],
        provider_cost: None,
        extra_usage: None,
        identity: ProviderIdentity::default(),
    };

    let layout = applet_bar_layout(
        snapshot.applet_windows(),
        snapshot.updated_at,
        UsageAmountFormat::Used,
    );

    assert_eq!(layout, AppletBarLayout::single_bar(37.5));
}

#[test]
fn panel_cells_use_each_flagged_account_without_changing_popup_selection() {
    let state = state_with_account_percents(&[30.0, 90.0]);
    let config = Config {
        panel_codex_account_ids: vec!["codex-1".to_string(), "codex-0".to_string()],
        usage_amount_format: UsageAmountFormat::Used,
        ..Config::default()
    };

    let cells = panel_cells(&state, &config);

    assert_eq!(cells[0].layout.primary, 30.0);
    assert_eq!(cells[1].layout.primary, 90.0);
    assert_eq!(
        state.active_account(ProviderId::Codex).unwrap().account_id,
        "codex-0"
    );
}

#[test]
fn panel_cells_fall_back_to_each_providers_legacy_snapshot() {
    let mut state = state_with_account_percents(&[30.0, 90.0]);
    let legacy = state.provider_accounts[1].snapshot.take();
    state
        .provider_mut(ProviderId::Codex)
        .unwrap()
        .legacy_display_snapshot = legacy;
    let mut other_legacy = state.provider_accounts[0].snapshot.clone().unwrap();
    other_legacy.provider = ProviderId::Minimax;
    other_legacy.windows[0].used_percent = 60.0;
    state
        .provider_mut(ProviderId::Minimax)
        .unwrap()
        .legacy_display_snapshot = Some(other_legacy);
    state.upsert_account(ProviderAccountRuntimeState::empty(
        ProviderId::Minimax,
        "minimax-0",
        "Minimax",
    ));
    let config = Config {
        panel_codex_account_ids: vec!["codex-0".to_string(), "codex-1".to_string()],
        panel_minimax_account_ids: vec!["minimax-0".to_string()],
        usage_amount_format: UsageAmountFormat::Used,
        ..Config::default()
    };

    let cells = panel_cells(&state, &config);

    assert_eq!(
        cells
            .iter()
            .map(|cell| cell.layout.primary)
            .collect::<Vec<_>>(),
        [30.0, 90.0, 60.0]
    );
}

fn state_with_account_percents(percents: &[f32]) -> AppState {
    let mut state = AppState::empty();
    let selected_account_ids = percents
        .iter()
        .enumerate()
        .map(|(i, _)| format!("codex-{i}"))
        .collect::<Vec<_>>();
    state
        .provider_mut(ProviderId::Codex)
        .unwrap()
        .selected_account_ids = selected_account_ids;

    for (i, percent) in percents.iter().enumerate() {
        let id = format!("codex-{i}");
        let mut account =
            ProviderAccountRuntimeState::empty(ProviderId::Codex, id.clone(), "Codex");
        account.snapshot = Some(UsageSnapshot {
            provider: ProviderId::Codex,
            source: "test".to_string(),
            updated_at: Utc::now(),
            headline: UsageHeadline(0),
            windows: vec![UsageWindow {
                label: "Session".to_string(),
                used_percent: *percent,
                reset_at: None,
                window_seconds: None,
                reset_description: None,
                group: None,
            }],
            provider_cost: None,
            extra_usage: None,
            identity: ProviderIdentity::default(),
        });
        state.upsert_account(account);
    }

    state
}

pub(super) fn test_app(refresh_owner: Option<RefreshOwner>) -> AppModel {
    let lock_path = std::env::temp_dir().join("yapcap-test-unused-owner.lock");
    AppModel {
        core: cosmic::Core::default(),
        popup: None,
        config: Config::default(),
        state: AppState::empty(),
        detection: crate::detection::DetectionSnapshot::default(),
        selected_provider: ProviderId::Codex,
        detail_account_page: 0,
        provider_viewport_offset: 0,
        popup_route: PopupRoute::ProviderDetail,
        update_status: UpdateStatus::Unchecked,
        launch_mode: LaunchMode::Standalone,
        shared_control: SharedControlState::default(),
        process_info: ProcessInfo {
            id: "test-process".to_string(),
            pid: std::process::id(),
            panel_output: None,
            flatpak_id: None,
            lock_path,
        },
        refresh_owner,
        refresh_batches: Vec::new(),
        next_refresh_batch_token: 1,
        codex_login: None,
        codex_login_handle: None,
        claude_login: None,
        claude_login_handle: None,
        cursor_scan: CursorScanState::Idle,
        cursor_scan_result: None,
        gemini_login: None,
        gemini_login_handle: None,
        copilot_login: None,
        copilot_login_handle: None,
        minimax_login: None,
        minimax_login_handle: None,
        kimi_login: None,
        kimi_login_handle: None,
        antigravity_login: None,
        antigravity_login_handle: None,
        opencode_go_login: None,
        opencode_go_login_handle: None,
        grok_login: None,
        grok_login_handle: None,
        zai_login: None,
        zai_login_handle: None,
    }
}

#[test]
fn test_app_starts_with_nothing_detected() {
    let app = test_app(None);
    for provider in ProviderId::ALL {
        assert!(!app.detection.detected(provider));
    }
}

fn ready_selected_provider(state: &mut AppState, provider: ProviderId) {
    let entry = state.provider_mut(provider).unwrap();
    entry.account_status = AccountSelectionStatus::Ready;
    entry.selected_account_ids = vec!["default".to_string()];
}

fn ready_configured_copilot_provider(app: &mut AppModel) {
    app.config.copilot_enablement = crate::config::ProviderEnablement::Enabled;
    app.config.copilot_managed_accounts = vec![copilot_account("default", "octocat")];
    app.config.selected_copilot_account_ids = vec!["default".to_string()];
    runtime_reconcile_provider(&app.config, &mut app.state, ProviderId::Copilot);
}

fn ready_two_copilot_accounts(app: &mut AppModel) {
    app.config.copilot_enablement = crate::config::ProviderEnablement::Enabled;
    app.config.copilot_managed_accounts =
        vec![copilot_account("a", "a"), copilot_account("b", "b")];
    app.config.selected_copilot_account_ids = vec!["a".to_string()];
    runtime_reconcile_provider(&app.config, &mut app.state, ProviderId::Copilot);
}

fn begin_test_batch(app: &mut AppModel, token: u64, account_ids: &[&str]) {
    let started_at = app
        .state
        .begin_provider_refresh(ProviderId::Copilot)
        .unwrap();
    app.refresh_batches.push(super::ProviderRefreshBatch {
        provider: ProviderId::Copilot,
        token,
        started_at,
        pending_account_ids: account_ids.iter().map(|id| (*id).to_string()).collect(),
        request: None,
    });
}

fn refresh_result(
    app: &AppModel,
    token: u64,
    account_id: &str,
    label: &str,
) -> crate::runtime::ProviderRefreshResult {
    let mut account = app
        .state
        .account(ProviderId::Copilot, account_id)
        .unwrap()
        .clone();
    account.label = label.to_string();
    crate::runtime::ProviderRefreshResult {
        batch_token: token,
        account_id: account_id.to_string(),
        provider: app.state.provider(ProviderId::Copilot).unwrap().clone(),
        accounts: vec![account],
    }
}

fn selected_account_without_usage(state: &mut AppState, provider: ProviderId) {
    let mut account = ProviderAccountRuntimeState::empty(provider, "default", provider.label());
    account.auth_state = crate::model::AuthState::Ready;
    state.upsert_account(account);
}

fn runtime_reconcile_provider(config: &Config, state: &mut AppState, provider: ProviderId) {
    crate::runtime::reconcile_provider(
        config,
        &crate::detection::DetectionSnapshot::default(),
        state,
        provider,
    );
}

fn control_request(provider: ProviderId) -> SharedControlState {
    let mut control = SharedControlState::default();
    control.upsert_request(refresh_request(provider, "test-process"));
    control
}

fn refresh_request(provider: ProviderId, process_id: &str) -> ProviderRefreshRequest {
    ProviderRefreshRequest {
        provider,
        reason: RefreshRequestReason::User,
        requested_at: Utc::now(),
        requesting_process_id: process_id.to_string(),
    }
}

fn copilot_account(id: &str, login: &str) -> ManagedCopilotAccountConfig {
    ManagedCopilotAccountConfig {
        id: id.to_string(),
        label: login.to_string(),
        github_user_id: 1,
        login: login.to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: Some(Utc::now()),
    }
}

fn codex_account(id: &str) -> ManagedCodexAccountConfig {
    ManagedCodexAccountConfig {
        id: id.to_string(),
        label: id.to_string(),
        codex_home: PathBuf::from("/tmp/yapcap/codex-1"),
        email: Some("user@example.com".to_string()),
        provider_account_id: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: None,
    }
}

fn claude_account(id: &str) -> ManagedClaudeAccountConfig {
    ManagedClaudeAccountConfig {
        id: id.to_string(),
        label: id.to_string(),
        config_dir: PathBuf::from("/tmp/yapcap/claude"),
        email: Some(format!("{id}@example.com")),
        organization: None,
        subscription_type: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: None,
    }
}

fn cursor_account(id: &str, email: &str) -> ManagedCursorAccountConfig {
    ManagedCursorAccountConfig {
        id: id.to_string(),
        email: email.to_string(),
        label: email.to_string(),
        account_root: PathBuf::from("/tmp/yapcap/cursor"),
        display_name: None,
        plan: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: None,
    }
}

fn gemini_account(id: &str) -> ManagedGeminiAccountConfig {
    ManagedGeminiAccountConfig {
        id: id.to_string(),
        label: id.to_string(),
        account_root: PathBuf::from("/tmp/yapcap/gemini"),
        email: format!("{id}@example.com"),
        sub: id.to_string(),
        hd: None,
        last_tier_id: None,
        last_cloudaicompanion_project: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: None,
    }
}

fn minimax_account(id: &str) -> ManagedMinimaxAccountConfig {
    ManagedMinimaxAccountConfig {
        id: id.to_string(),
        label: id.to_string(),
        api_key_source: "env:MINIMAX_API_KEY".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: None,
    }
}

fn kimi_account(id: &str) -> ManagedKimiAccountConfig {
    ManagedKimiAccountConfig {
        id: id.to_string(),
        label: id.to_string(),
        api_key_source: "env:KIMI_API_KEY".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: None,
    }
}

fn zai_account(id: &str) -> ManagedZaiAccountConfig {
    ManagedZaiAccountConfig {
        id: id.to_string(),
        label: id.to_string(),
        api_key_source: "env:ZAI_API_KEY".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: None,
    }
}

fn antigravity_account(id: &str) -> crate::config::ManagedAntigravityAccountConfig {
    crate::config::ManagedAntigravityAccountConfig {
        id: id.to_string(),
        label: id.to_string(),
        account_root: PathBuf::from("/tmp/yapcap/antigravity"),
        email: format!("{id}@example.com"),
        sub: id.to_string(),
        last_tier_id: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: None,
    }
}

#[test]
fn cursor_status_refresh_skipped_without_accounts() {
    let _env = crate::test_support::test_env();
    let config = Config::default();

    assert_eq!(
        config.cursor_enablement,
        crate::config::ProviderEnablement::Auto
    );
    assert!(!should_refresh_account_statuses(
        &AppState::empty(),
        ProviderId::Cursor
    ));
}

#[test]
fn cursor_status_refresh_runs_with_accounts() {
    let _env = crate::test_support::test_env();
    seed_account_storage(
        crate::config::paths().cursor_accounts_dir,
        ProviderId::Cursor,
        "one",
        "one@example.com",
    );
    let mut config = Config::default();
    config
        .cursor_managed_accounts
        .push(cursor_account("one", "one@example.com"));

    let mut state = AppState::empty();
    state.provider_mut(ProviderId::Cursor).unwrap().enabled = true;
    state.upsert_account(ProviderAccountRuntimeState::empty(
        ProviderId::Cursor,
        "cursor".to_string(),
        "Cursor".to_string(),
    ));
    assert!(should_refresh_account_statuses(&state, ProviderId::Cursor));
}

fn seed_account_storage(dir: PathBuf, provider: ProviderId, id: &str, email: &str) {
    let storage = ProviderAccountStorage::new(dir);
    storage
        .replace_account(
            id.to_string(),
            NewProviderAccount {
                provider,
                email: email.to_string(),
                provider_account_id: None,
                organization_id: None,
                organization_name: None,
                tokens: ProviderAccountTokens {
                    access_token: "access".to_string(),
                    refresh_token: "refresh".to_string(),
                    expires_at: Utc::now() + chrono::Duration::hours(1),
                    scope: Vec::new(),
                    token_id: None,
                },
                snapshot: None,
            },
        )
        .unwrap();
}

#[test]
fn delete_account_requests_refresh_for_all_providers() {
    for provider in ProviderId::ALL {
        let mut env = crate::test_support::test_env();
        let state_root = std::env::temp_dir().join(format!(
            "yapcap-delete-account-test-{provider:?}-{}",
            std::process::id()
        ));
        env.set("XDG_STATE_HOME", &state_root);
        env.set("XDG_CONFIG_HOME", &state_root);

        let mut app = test_app(None);
        let keep_id = "keep";

        let remove_account_id = match provider {
            ProviderId::Codex => {
                seed_account_storage(
                    crate::config::paths().codex_accounts_dir,
                    provider,
                    keep_id,
                    "keep@example.com",
                );
                app.config
                    .codex_managed_accounts
                    .push(codex_account(keep_id));
                app.config
                    .codex_managed_accounts
                    .push(codex_account("remove"));
                app.config.selected_codex_account_ids = vec![keep_id.to_string()];
                "remove".to_string()
            }
            ProviderId::Claude => {
                app.config
                    .claude_managed_accounts
                    .push(claude_account(keep_id));
                app.config
                    .claude_managed_accounts
                    .push(claude_account("remove"));
                app.config.selected_claude_account_ids = vec![keep_id.to_string()];
                "remove".to_string()
            }
            ProviderId::Cursor => {
                seed_account_storage(
                    crate::config::paths().cursor_accounts_dir,
                    provider,
                    keep_id,
                    "keep@example.com",
                );
                app.config
                    .cursor_managed_accounts
                    .push(cursor_account(keep_id, "keep@example.com"));
                app.config
                    .cursor_managed_accounts
                    .push(cursor_account("remove", "remove@example.com"));
                app.config.selected_cursor_account_ids = vec![format!("cursor-managed:{keep_id}")];
                "cursor-managed:remove".to_string()
            }
            ProviderId::Gemini => {
                app.config.gemini_enablement = crate::config::ProviderEnablement::Enabled;
                app.config
                    .gemini_managed_accounts
                    .push(gemini_account(keep_id));
                app.config
                    .gemini_managed_accounts
                    .push(gemini_account("remove"));
                app.config.selected_gemini_account_ids = vec![keep_id.to_string()];
                "remove".to_string()
            }
            ProviderId::Copilot => {
                app.config
                    .copilot_managed_accounts
                    .push(copilot_account(keep_id, "keep"));
                app.config
                    .copilot_managed_accounts
                    .push(copilot_account("remove", "remove"));
                app.config.selected_copilot_account_ids = vec![keep_id.to_string()];
                "remove".to_string()
            }
            ProviderId::Minimax => {
                app.config
                    .minimax_managed_accounts
                    .push(minimax_account(keep_id));
                app.config
                    .minimax_managed_accounts
                    .push(minimax_account("remove"));
                app.config.selected_minimax_account_ids = vec![keep_id.to_string()];
                "remove".to_string()
            }
            ProviderId::Zai => {
                app.config.zai_managed_accounts.push(zai_account(keep_id));
                app.config.zai_managed_accounts.push(zai_account("remove"));
                app.config.selected_zai_account_ids = vec![keep_id.to_string()];
                "remove".to_string()
            }
            ProviderId::Kimi => {
                app.config.kimi_managed_accounts.push(kimi_account(keep_id));
                app.config
                    .kimi_managed_accounts
                    .push(kimi_account("remove"));
                app.config.selected_kimi_account_ids = vec![keep_id.to_string()];
                "remove".to_string()
            }
            ProviderId::Antigravity => {
                app.config
                    .antigravity_managed_accounts
                    .push(antigravity_account(keep_id));
                app.config
                    .antigravity_managed_accounts
                    .push(antigravity_account("remove"));
                app.config.selected_antigravity_account_ids = vec![keep_id.to_string()];
                "remove".to_string()
            }
            ProviderId::OpenCodeGo | ProviderId::Grok => continue,
        };

        let _task = app.delete_account(provider, &remove_account_id);

        assert!(
            !crate::providers::registry::discover_accounts(provider, &app.config)
                .iter()
                .any(|account| account.account_id == remove_account_id),
            "{provider:?} should no longer discover the deleted account"
        );
        assert_eq!(
            app.state.provider(provider).unwrap().account_status,
            AccountSelectionStatus::Ready,
            "{provider:?} should remain ready with the kept account selected"
        );
        assert!(
            app.shared_control
                .requests
                .iter()
                .any(|request| request.provider == provider),
            "{provider:?} should request a refresh after delete"
        );
    }
}

fn refresh_owner(name: &str) -> RefreshOwner {
    let lock_path = std::env::temp_dir().join(format!(
        "yapcap-app-refresh-owner-{name}-{}.lock",
        std::process::id()
    ));
    match crate::refresh_owner::try_acquire(lock_path).unwrap() {
        RefreshOwnerAttempt::Owner(owner) => owner,
        RefreshOwnerAttempt::NonOwner(_) => panic!("test lock should be available"),
    }
}
