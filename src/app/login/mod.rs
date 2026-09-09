mod flows;
mod legacy;

pub(crate) use flows::{
    AntigravityLoginFlow, ClaudeLoginFlow, CodexLoginFlow, CopilotLoginFlow, GeminiLoginFlow,
    GrokLoginFlow, KimiLoginFlow, MinimaxLoginFlow, OpenCodeGoLoginFlow,
};

use super::{
    AntigravityLoginEvent, AppModel, ClaudeLoginEvent, CodexLoginEvent, Config, CopilotLoginEvent,
    GeminiLoginEvent, GrokLoginEvent, Handle, KimiLoginEvent, Message, MinimaxLoginEvent,
    OpenCodeGoLoginEvent, ProviderId, Task, runtime,
};
use crate::shared_state::RefreshRequestReason;

pub(crate) trait LoginFlow: Sized {
    type State: Clone;
    type Event: Send + 'static;

    const PROVIDER: ProviderId;

    fn state(app: &AppModel) -> &Option<Self::State>;
    fn state_mut(app: &mut AppModel) -> &mut Option<Self::State>;
    fn handle_mut(app: &mut AppModel) -> &mut Option<Handle>;
    fn is_running(state: &Self::State) -> bool;
    fn log_id(state: &Self::State) -> &str;
    fn status_debug(state: &Self::State) -> String;
    fn account_exists(config: &Config, account_id: &str) -> bool;
    fn failed_state(error: String) -> Self::State;
    fn prepare(config: Config) -> Result<(Self::State, cosmic::iced::Task<Self::Event>), String>;
    fn prepare_for_reauth(
        config: Config,
        account_id: &str,
    ) -> Result<(Self::State, cosmic::iced::Task<Self::Event>), String> {
        let _ = account_id;
        Self::prepare(config)
    }
    fn wrap_event(event: Self::Event) -> Message;
    fn on_event(app: &mut AppModel, event: Self::Event) -> Task<Message>;
}

pub(super) fn start_login<F: LoginFlow>(app: &mut AppModel) -> Task<Message> {
    if F::state(app).as_ref().is_some_and(F::is_running) {
        log_login_already_running(&app.process_info.id, F::PROVIDER);
        return Task::none();
    }
    log_login_requested(&app.process_info.id, F::PROVIDER);
    *F::state_mut(app) = None;
    begin_login::<F>(app, F::prepare(app.config.clone()))
}

pub(super) fn start_with<F: LoginFlow>(
    app: &mut AppModel,
    prepare: impl FnOnce(Config) -> Result<(F::State, cosmic::iced::Task<F::Event>), String>,
) -> Task<Message> {
    if F::state(app).as_ref().is_some_and(F::is_running) {
        log_login_already_running(&app.process_info.id, F::PROVIDER);
        return Task::none();
    }
    log_login_requested(&app.process_info.id, F::PROVIDER);
    *F::state_mut(app) = None;
    begin_login::<F>(app, prepare(app.config.clone()))
}

pub(super) fn reauthenticate<F: LoginFlow>(app: &mut AppModel, account_id: &str) -> Task<Message> {
    if !F::account_exists(&app.config, account_id) {
        log_reauth_ignored(&app.process_info.id, F::PROVIDER, account_id);
        return Task::none();
    }
    if F::state(app).as_ref().is_some_and(F::is_running) {
        log_login_already_running(&app.process_info.id, F::PROVIDER);
        return Task::none();
    }
    log_reauth_requested(&app.process_info.id, F::PROVIDER, account_id);
    *F::state_mut(app) = None;
    begin_login::<F>(app, F::prepare_for_reauth(app.config.clone(), account_id))
}

pub(super) fn cancel_login<F: LoginFlow>(app: &mut AppModel) {
    match F::state(app).as_ref() {
        Some(state) => log_login_state_cleared(
            &app.process_info.id,
            F::PROVIDER,
            F::log_id(state),
            F::status_debug(state),
        ),
        None => log_login_state_clear_ignored(&app.process_info.id, F::PROVIDER),
    }
    if let Some(handle) = F::handle_mut(app).take() {
        handle.abort();
    }
    *F::state_mut(app) = None;
}

fn begin_login<F: LoginFlow>(
    app: &mut AppModel,
    prepared: Result<(F::State, cosmic::iced::Task<F::Event>), String>,
) -> Task<Message> {
    let (state, task) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            tracing::info!(
                process_id = %app.process_info.id,
                provider = F::PROVIDER.label(),
                error = %error,
                "login preparation failed"
            );
            *F::state_mut(app) = Some(F::failed_state(error));
            return Task::none();
        }
    };
    tracing::info!(
        process_id = %app.process_info.id,
        provider = F::PROVIDER.label(),
        flow_id = F::log_id(&state),
        "login flow started"
    );
    *F::state_mut(app) = Some(state);
    let task = task.map(|event| cosmic::Action::App(F::wrap_event(event)));
    let (task, handle) = task.abortable();
    *F::handle_mut(app) = Some(handle);
    task
}

fn apply_login_success(
    app: &mut AppModel,
    provider: ProviderId,
    flow_id: &str,
    account_id: String,
    apply: impl FnOnce(&mut Config),
) -> Task<Message> {
    tracing::info!(
        process_id = %app.process_info.id,
        provider = provider.label(),
        flow_id,
        account_id,
        "login flow succeeded"
    );
    app.write_config(|new_config| apply(new_config));
    runtime::reconcile_provider(&app.config, &app.detection, &mut app.state, provider);
    runtime::mark_account_reauthenticated(&mut app.state, provider, &account_id);
    app.sync_panel_suggested_bounds();
    app.request_provider_refresh(provider, RefreshRequestReason::AccountAction)
}

fn log_login_failed(process_id: &str, provider: ProviderId, flow_id: &str, error: &str) {
    tracing::info!(
        process_id,
        provider = provider.label(),
        flow_id,
        error,
        "login flow failed"
    );
}

fn log_login_requested(process_id: &str, provider: ProviderId) {
    tracing::info!(
        process_id,
        provider = provider.label(),
        "login flow requested"
    );
}

fn log_login_already_running(process_id: &str, provider: ProviderId) {
    tracing::info!(
        process_id,
        provider = provider.label(),
        "login flow ignored because one is already running"
    );
}

fn log_reauth_requested(process_id: &str, provider: ProviderId, account_id: &str) {
    tracing::info!(
        process_id,
        provider = provider.label(),
        account_id,
        "reauthentication requested"
    );
}

fn log_reauth_ignored(process_id: &str, provider: ProviderId, account_id: &str) {
    tracing::info!(
        process_id,
        provider = provider.label(),
        account_id,
        "reauthentication ignored because account was not configured"
    );
}

fn log_login_state_cleared(
    process_id: &str,
    provider: ProviderId,
    flow_id: &str,
    status_before: impl std::fmt::Debug,
) {
    tracing::info!(
        process_id,
        provider = provider.label(),
        flow_id,
        status_before = ?status_before,
        "login state cleared"
    );
}

fn log_login_state_clear_ignored(process_id: &str, provider: ProviderId) {
    tracing::info!(
        process_id,
        provider = provider.label(),
        reason = "no_login_state",
        "login state clear ignored"
    );
}

#[derive(Debug, Clone)]
pub(crate) enum LoginEventKind {
    Codex(CodexLoginEvent),
    Claude(ClaudeLoginEvent),
    Gemini(GeminiLoginEvent),
    Copilot(CopilotLoginEvent),
    Kimi(KimiLoginEvent),
    Minimax(MinimaxLoginEvent),
    Antigravity(AntigravityLoginEvent),
    OpenCodeGo(OpenCodeGoLoginEvent),
    Grok(GrokLoginEvent),
}

#[cfg(test)]
mod login_flow_tests;
