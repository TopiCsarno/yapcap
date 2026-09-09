// SPDX-License-Identifier: MPL-2.0

mod applet;
mod host_auth_watch;
mod login;

use self::login::LoginFlow;
mod popup_view;
mod provider_actions;
mod provider_assets;
mod refresh;
mod session;
mod state;
#[cfg(test)]
mod tests;
mod window;

pub(crate) use self::applet::applet_settings;
use self::applet::{
    applet_button, applet_fallback_indicator, applet_indicator, panel_button_size,
    panel_fallback_active, select_provider,
};
use self::popup_view::ProviderLoginStates;
use self::provider_assets::{provider_icon_handle, provider_icon_variant};
use self::refresh::{
    RefreshSkipDiagnostics, automatic_refresh_provider_tasks_for_process,
    refresh_provider_account_statuses_task, refresh_provider_task_for_process,
    selected_account_refresh_due,
};
use self::window::{
    format_retry_delay, open_url, update_check_task, update_retry_delay, update_retry_task,
};
use crate::config::{
    APP_ID, Config, ManagedClaudeAccountConfig, ManagedCodexAccountConfig,
    ManagedCursorAccountConfig, PanelIconStyle, ResetTimeFormat, UsageAmountFormat,
};
use crate::demo_env;
use crate::model::{
    AccountSelectionStatus, AppState, ProviderAccountRuntimeState, ProviderHealth, ProviderId,
};
use crate::providers::antigravity::{
    self, AntigravityLoginEvent, AntigravityLoginState, AntigravityLoginStatus,
};
use crate::providers::claude::{self, ClaudeLoginEvent, ClaudeLoginState, ClaudeLoginStatus};
use crate::providers::codex::{self, CodexLoginEvent, CodexLoginState, CodexLoginStatus};
use crate::providers::copilot::{self, CopilotLoginEvent, CopilotLoginState, CopilotLoginStatus};
use crate::providers::cursor::{self, CursorScanResult, CursorScanState};
use crate::providers::gemini::{self, GeminiLoginEvent, GeminiLoginState, GeminiLoginStatus};
use crate::providers::grok::{self, GrokLoginEvent, GrokLoginState, GrokLoginStatus};
use crate::providers::kimi::{
    self,
    login::{KimiLoginEvent, KimiLoginState},
};
use crate::providers::minimax::{self, MinimaxLoginEvent, MinimaxLoginState};
use crate::providers::opencode_go::login::{OpenCodeGoLoginEvent, OpenCodeGoLoginState};
use crate::providers::registry;
use crate::refresh_owner::{
    self, ProcessInfo, RefreshOwner, RefreshOwnerAttempt, RefreshOwnerWaiter,
};
use crate::runtime;
use crate::runtime::{ProviderRefreshResult, RefreshProcessContext};
use crate::shared_state::{
    self, ProviderRefreshRequest, RefreshRequestReason, SharedControlState, SharedRuntimeState,
    SharedStateWriter,
};
use crate::updates::UpdateStatus;
use crate::usage_display;
use chrono::Utc;
use cosmic::app::Task;
use cosmic::cosmic_config::CosmicConfigEntry;
use cosmic::iced::task::Handle;
use cosmic::iced::time;
use cosmic::iced::widget::{progress_bar, row};
use cosmic::iced::window::Id;
use cosmic::iced::{Alignment, Length, Limits, Size, Subscription};
use cosmic::prelude::*;
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::theme::Button as CosmicButton;
use cosmic::widget;
use std::time::Duration;

const AUTOMATIC_REFRESH_POLL_INTERVAL_SECS: u64 = 10;
const APPLET_BAR_WIDTH_HEIGHT_MULTIPLIER: u16 = 2;
const APPLET_ICON_GAP: f32 = 6.0;
const APPLET_PERCENT_GLYPH_WIDTH: f32 = 7.25;
const APPLET_PERCENT_CELL_HORIZONTAL_PAD: f32 = 8.0;
const UPDATE_RETRY_INITIAL_SECS: u64 = 15;
const UPDATE_RETRY_MAX_SECS: u64 = 15 * 60;
const POPUP_MAX_HEIGHT: f32 = 1200.0;

fn automatic_refresh_poll_interval() -> Duration {
    Duration::from_secs(AUTOMATIC_REFRESH_POLL_INTERVAL_SECS)
}

fn popup_size_limits() -> Limits {
    Limits::NONE
        .min_height(1.0)
        .min_width(396.0)
        .max_width(396.0)
        .max_height(POPUP_MAX_HEIGHT)
}

pub struct AppModel {
    core: cosmic::Core,
    popup: Option<Id>,
    config: Config,
    state: AppState,
    detection: crate::detection::DetectionSnapshot,
    selected_provider: ProviderId,
    detail_account_page: usize,
    provider_viewport_offset: usize,
    popup_route: PopupRoute,
    update_status: UpdateStatus,
    launch_mode: LaunchMode,
    shared_control: SharedControlState,
    process_info: ProcessInfo,
    refresh_owner: Option<RefreshOwner>,
    codex_login: Option<CodexLoginState>,
    codex_login_handle: Option<Handle>,
    claude_login: Option<ClaudeLoginState>,
    claude_login_handle: Option<Handle>,
    cursor_scan: CursorScanState,
    cursor_scan_result: Option<CursorScanResult>,
    gemini_login: Option<GeminiLoginState>,
    gemini_login_handle: Option<Handle>,
    copilot_login: Option<CopilotLoginState>,
    copilot_login_handle: Option<Handle>,
    minimax_login: Option<MinimaxLoginState>,
    minimax_login_handle: Option<Handle>,
    kimi_login: Option<KimiLoginState>,
    kimi_login_handle: Option<Handle>,
    antigravity_login: Option<AntigravityLoginState>,
    antigravity_login_handle: Option<Handle>,
    opencode_go_login: Option<OpenCodeGoLoginState>,
    opencode_go_login_handle: Option<Handle>,
    pub grok_login: Option<GrokLoginState>,
    pub grok_login_handle: Option<Handle>,
}

impl Drop for AppModel {
    fn drop(&mut self) {
        tracing::info!(
            pid = self.process_info.pid,
            process_id = %self.process_info.id,
            panel_output = ?self.process_info.panel_output,
            owner_status = self.owner_status(),
            "YapCap stopped"
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    Panel,
    Standalone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupRoute {
    ProviderDetail,
    Settings,
    ManageProviders,
    ManageAccounts(ProviderId),
    About,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagerDirection {
    Previous,
    Next,
}

#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    UpdateConfig(Box<Config>, Vec<&'static str>),
    UpdateSharedRuntime(Box<SharedRuntimeState>, Vec<&'static str>),
    UpdateSharedControl(Box<SharedControlState>, Vec<&'static str>),
    RefreshOwnershipAcquired(Result<RefreshOwner, String>),
    Tick,
    RefreshNow,
    ProviderRefreshed(Box<ProviderRefreshResult>),
    SelectProvider(ProviderId),
    NavigateTo(PopupRoute),
    SetProviderEnabled(ProviderId, bool),
    ToggleAccountSelection(ProviderId, String),
    PageProviderAccount(PagerDirection),
    PageProviderViewport(PagerDirection),
    DeleteAccount(ProviderId, String),
    UpdateClaudeLoginCode(String),
    SubmitClaudeLoginCode,
    CopyCopilotLoginCode(String),
    ClearCopilotLoginCodeCopied(String),
    ReauthenticateAccount(ProviderId, String),
    StartLogin(ProviderId),
    ImportFromOpenCode(ProviderId, Option<String>),
    RestoreFromOpenCode(ProviderId, String),
    ImportFromGrok(Option<String>),
    RestoreFromGrok(String),
    CancelLogin(ProviderId),
    LoginEvent(ProviderId, Box<login::LoginEventKind>),
    StartCursorScan,
    ConfirmCursorScan,
    DismissCursorScan,
    CursorScanComplete(CursorScanState, Option<CursorScanResult>),
    ProviderAccountStatusesRefreshed(ProviderId, Vec<ProviderAccountRuntimeState>),
    SetRefreshInterval(u64),
    SetResetTimeFormat(ResetTimeFormat),
    SetUsageAmountFormat(UsageAmountFormat),
    SetPanelIconStyle(PanelIconStyle),
    CheckUpdates,
    UpdateChecked { status: UpdateStatus, attempt: u32 },
    RetryUpdateCheck(u32),
    OpenUrl(String),
    HostCliAuthChanged,
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;
    type Flags = LaunchMode;
    type Message = Message;

    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(mut core: cosmic::Core, launch_mode: Self::Flags) -> (Self, Task<Self::Message>) {
        core.window.show_headerbar = false;
        core.window.sharp_corners = matches!(launch_mode, LaunchMode::Panel);
        core.window.show_maximize = false;
        core.window.show_minimize = false;
        core.window.use_template = false;

        let config = crate::config::cosmic_config_context(Self::APP_ID, Config::VERSION)
            .map(|ctx| {
                let mut config = match Config::get_entry(&ctx) {
                    Ok(cfg) => cfg,
                    Err((_errors, cfg)) => cfg,
                };
                let mut changed = crate::config::migrate_provider_enablement(&ctx, &mut config);
                changed |= registry::startup_sync(&mut config);
                changed |= demo_env::strip_leaked_state(&mut config);
                if changed {
                    let _ = config.write_entry(&ctx);
                }
                demo_env::apply_config(&mut config);
                config
            })
            .unwrap_or_default();

        let detection = if demo_env::is_active() {
            demo_env::detection_snapshot()
        } else {
            crate::detection::startup_snapshot(crate::config::host_user_home_dir())
        };
        tracing::info!(
            detected_providers = ?detection.detected_providers(),
            "startup provider detection"
        );
        let initial_config = config.clone();
        let shared_runtime = shared_state::load_runtime(Self::APP_ID);
        let mut shared_control = shared_state::load_control(Self::APP_ID);
        let shared_runtime_generation = shared_runtime.as_ref().map(|state| state.generation);
        let shared_control_generation = shared_control.generation;
        let lock_path = refresh_owner::lock_path(&crate::config::paths());
        let process_info = ProcessInfo::current(lock_path.clone());
        let startup_diagnostics =
            StartupDiagnostics::new(shared_runtime_generation, shared_control_generation);
        let (refresh_owner, ownership_task) =
            initialize_refresh_ownership(&process_info, &startup_diagnostics, &mut shared_control);
        let mut state = runtime::load_initial_state(&initial_config, &detection, shared_runtime);
        #[cfg(debug_assertions)]
        crate::debug_env::apply(&mut state);
        demo_env::apply(&initial_config, &mut state);
        let selected_provider = select_provider(initial_config.selected_provider, &state);
        let (applet_width, applet_height) =
            panel_button_size(&core, &state, initial_config.panel_icon_style);
        core.applet.suggested_bounds = Some(Size::new(applet_width, applet_height));
        let mut app = AppModel {
            core,
            popup: None,
            config,
            state,
            detection,
            selected_provider,
            detail_account_page: 0,
            provider_viewport_offset: 0,
            popup_route: PopupRoute::ProviderDetail,
            update_status: UpdateStatus::Unchecked,
            launch_mode,
            shared_control,
            process_info,
            refresh_owner,
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
        };
        tracing::info!(
            pid = app.process_info.pid,
            process_id = %app.process_info.id,
            panel_output = ?app.process_info.panel_output,
            owner_status = app.owner_status(),
            flatpak_status = app.process_info.flatpak_status(),
            lock_path = %app.process_info.lock_path.display(),
            launch_mode = ?app.launch_mode,
            config_version = Config::VERSION,
            shared_runtime_generation = ?shared_runtime_generation,
            shared_control_generation,
            selected_provider = app.selected_provider.label(),
            enabled_provider_count = ProviderId::ALL
                .into_iter()
                .filter(|provider| app.state.provider(*provider).is_some_and(|state| state.enabled))
                .count(),
            account_count = app.state.provider_accounts.len(),
            refresh_interval_seconds = app.config.refresh_interval_seconds,
            "YapCap started"
        );

        let refresh_task = app.automatic_refresh_task();
        let cursor_status_task = if app.refresh_owner.is_some() {
            refresh_provider_account_statuses_task(&app.config, &app.state, ProviderId::Cursor)
        } else {
            Task::none()
        };
        let update_task = update_check_task(0);
        let startup = startup_task(
            demo_env::is_active(),
            refresh_task,
            update_task,
            cursor_status_task,
            ownership_task,
        );

        (app, startup)
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let indicator = if panel_fallback_active(&self.state) {
            applet_fallback_indicator(&self.core)
        } else {
            applet_indicator(
                &self.state,
                self.selected_provider,
                self.config.panel_icon_style,
                self.config.usage_amount_format,
                &self.core,
            )
        };
        let size = panel_button_size(&self.core, &self.state, self.config.panel_icon_style);
        let button: Element<'_, Message> = applet_button(&self.core, size, indicator)
            .on_press(Message::TogglePopup)
            .into();

        match self.launch_mode {
            LaunchMode::Panel => self.core.applet.autosize_window(button).into(),
            LaunchMode::Standalone => button,
        }
    }

    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        let content = popup_view::popup_content(
            &self.state,
            &self.config,
            &self.detection,
            ProviderLoginStates {
                codex: self.codex_login.as_ref(),
                claude: self.claude_login.as_ref(),
                cursor_scan: &self.cursor_scan,
                gemini: self.gemini_login.as_ref(),
                copilot: self.copilot_login.as_ref(),
                minimax: self.minimax_login.as_ref(),
                kimi: self.kimi_login.as_ref(),
                antigravity: self.antigravity_login.as_ref(),
                opencode_go: self.opencode_go_login.as_ref(),
                grok: self.grok_login.as_ref(),
            },
            popup_view::DetailSelection {
                provider: self.selected_provider,
                account_page: self.detail_account_page,
                provider_viewport_offset: self.provider_viewport_offset,
            },
            &self.popup_route,
            &self.update_status,
        );
        self.core
            .applet
            .popup_container(content)
            .limits(popup_size_limits())
            .into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::batch(vec![
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| Message::UpdateConfig(Box::new(update.config), update.keys)),
            self.core()
                .watch_config::<SharedRuntimeState>(Self::APP_ID)
                .map(|update| Message::UpdateSharedRuntime(Box::new(update.config), update.keys)),
            self.core()
                .watch_config::<SharedControlState>(Self::APP_ID)
                .map(|update| Message::UpdateSharedControl(Box::new(update.config), update.keys)),
            time::every(automatic_refresh_poll_interval()).map(|_| Message::Tick),
            host_auth_watch::subscription(),
        ])
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        self.handle_message(message)
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

impl AppModel {
    fn handle_message(&mut self, message: Message) -> Task<Message> {
        if let Some(task) = self.handle_message_task(message) {
            return task;
        }
        Task::none()
    }

    fn handle_message_task(&mut self, message: Message) -> Option<Task<Message>> {
        match message {
            Message::UpdateConfig(config, keys) => {
                self.on_config_update(*config, &keys);
            }
            Message::UpdateSharedRuntime(shared_runtime, keys) => {
                if keys.is_empty() || keys.contains(&"app_state") {
                    self.on_shared_runtime_update(*shared_runtime);
                }
            }
            Message::UpdateSharedControl(shared_control, keys) => {
                if keys.is_empty() || keys.contains(&"requests") {
                    return Some(self.handle_shared_control_update(*shared_control));
                }
            }
            Message::RefreshOwnershipAcquired(result) => {
                return Some(self.handle_refresh_ownership_acquired(result));
            }
            Message::TogglePopup => {
                return Some(self.toggle_popup());
            }
            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                    tracing::info!(
                        process_id = %self.process_info.id,
                        route = provider_actions::popup_route_label(self.popup_route),
                        provider = provider_actions::popup_route_provider_label(
                            self.popup_route,
                            self.selected_provider,
                        ),
                        "popup closed by window manager"
                    );
                }
            }
            Message::Tick => {
                return Some(self.automatic_refresh_task());
            }
            Message::RefreshNow => {
                return Some(self.handle_refresh_now());
            }
            Message::ProviderRefreshed(refresh_result) => {
                return Some(self.handle_provider_refreshed(*refresh_result));
            }
            Message::ProviderAccountStatusesRefreshed(provider, accounts) => {
                self.handle_provider_account_statuses_refreshed(provider, accounts);
            }
            Message::SelectProvider(provider) => {
                return Some(self.select_provider_tab(provider));
            }
            Message::PageProviderAccount(direction) => {
                return Some(self.page_provider_account(direction));
            }
            Message::PageProviderViewport(direction) => {
                self.page_provider_viewport(direction);
            }
            Message::NavigateTo(route) => {
                self.navigate_to(route);
            }
            Message::UpdateChecked { status, attempt } => {
                return Some(self.handle_update_checked(status, attempt));
            }
            Message::CheckUpdates => {
                tracing::info!(process_id = %self.process_info.id, "manual update check requested");
                self.update_status = UpdateStatus::Unchecked;
                return Some(update_check_task(0));
            }
            Message::RetryUpdateCheck(attempt) => {
                if matches!(self.update_status, UpdateStatus::Error(_)) {
                    tracing::info!(
                        process_id = %self.process_info.id,
                        attempt,
                        "update check retry scheduled"
                    );
                    return Some(update_check_task(attempt));
                }
            }
            Message::OpenUrl(url) => open_url(&url),
            Message::HostCliAuthChanged => self.on_host_cli_auth_changed(),
            Message::SetProviderEnabled(provider, enabled) => {
                return Some(self.set_provider_enabled(provider, enabled));
            }
            Message::SetRefreshInterval(seconds) => {
                return Some(self.set_refresh_interval(seconds));
            }
            Message::SetResetTimeFormat(format) => {
                return Some(self.set_reset_time_format(format));
            }
            Message::SetUsageAmountFormat(format) => {
                return Some(self.set_usage_amount_format(format));
            }
            Message::SetPanelIconStyle(style) => {
                return Some(self.set_panel_icon_style(style));
            }
            Message::ToggleAccountSelection(provider, account_id) => {
                return Some(self.toggle_account_selection(provider, &account_id));
            }
            Message::DeleteAccount(provider, account_id) => {
                return Some(self.delete_account(provider, &account_id));
            }
            Message::ReauthenticateAccount(provider, account_id) => {
                return Some(session::reauthenticate(self, provider, &account_id));
            }
            Message::StartLogin(provider) => {
                return Some(session::start_login(self, provider));
            }
            Message::ImportFromOpenCode(provider, target_account_id) => {
                return Some(session::import_from_opencode(
                    self,
                    provider,
                    target_account_id,
                ));
            }
            Message::RestoreFromOpenCode(provider, account_id) => {
                return Some(session::restore_from_opencode(self, provider, account_id));
            }
            Message::ImportFromGrok(target_account_id) => {
                return Some(session::import_from_grok(self, target_account_id));
            }
            Message::RestoreFromGrok(account_id) => {
                return Some(session::restore_from_grok(self, account_id));
            }
            Message::CancelLogin(provider) => session::cancel_login(self, provider),
            Message::LoginEvent(provider, kind) => {
                return Some(match (provider, *kind) {
                    (ProviderId::Codex, login::LoginEventKind::Codex(event)) => {
                        login::CodexLoginFlow::on_event(self, event)
                    }
                    (ProviderId::Claude, login::LoginEventKind::Claude(event)) => {
                        login::ClaudeLoginFlow::on_event(self, event)
                    }
                    (ProviderId::Gemini, login::LoginEventKind::Gemini(event)) => {
                        login::GeminiLoginFlow::on_event(self, event)
                    }
                    (ProviderId::Copilot, login::LoginEventKind::Copilot(event)) => {
                        login::CopilotLoginFlow::on_event(self, event)
                    }
                    (ProviderId::Minimax, login::LoginEventKind::Minimax(event)) => {
                        login::MinimaxLoginFlow::on_event(self, event)
                    }
                    (ProviderId::Kimi, login::LoginEventKind::Kimi(event)) => {
                        login::KimiLoginFlow::on_event(self, event)
                    }
                    (ProviderId::Antigravity, login::LoginEventKind::Antigravity(event)) => {
                        login::AntigravityLoginFlow::on_event(self, event)
                    }
                    (ProviderId::OpenCodeGo, login::LoginEventKind::OpenCodeGo(event)) => {
                        login::OpenCodeGoLoginFlow::on_event(self, event)
                    }
                    (ProviderId::Grok, login::LoginEventKind::Grok(event)) => {
                        login::GrokLoginFlow::on_event(self, event)
                    }
                    _ => Task::none(),
                });
            }
            Message::CopyCopilotLoginCode(code) => {
                return Some(self.copy_copilot_login_code(code));
            }
            Message::ClearCopilotLoginCodeCopied(flow_id) => {
                self.clear_copilot_login_code_copied(&flow_id);
            }
            Message::UpdateClaudeLoginCode(code) => self.update_claude_login_code(code),
            Message::SubmitClaudeLoginCode => return Some(self.submit_claude_login_code()),
            Message::StartCursorScan => return Some(self.start_cursor_scan()),
            Message::ConfirmCursorScan => return Some(self.confirm_cursor_scan()),
            Message::DismissCursorScan => self.dismiss_cursor_scan(),
            Message::CursorScanComplete(state, result) => {
                self.handle_cursor_scan_complete(state, result);
            }
        }
        None
    }

    fn owner_status(&self) -> &'static str {
        if self.refresh_owner.is_some() {
            "owner"
        } else {
            "non_owner"
        }
    }

    pub(super) fn shared_state_writer(&self) -> SharedStateWriter<'_> {
        SharedStateWriter {
            process_id: &self.process_info.id,
            owner_status: self.owner_status(),
        }
    }

    fn refresh_task_process(&self) -> RefreshProcessContext {
        refresh_task_process(&self.process_info, self.owner_status())
    }

    fn handle_refresh_ownership_acquired(
        &mut self,
        result: Result<RefreshOwner, String>,
    ) -> Task<Message> {
        match result {
            Ok(owner) => {
                tracing::info!(
                    pid = self.process_info.pid,
                    process_id = %self.process_info.id,
                    panel_output = ?self.process_info.panel_output,
                    owner_status = "owner",
                    flatpak_status = self.process_info.flatpak_status(),
                    lock_path = %owner.lock_path().display(),
                    config_version = Config::VERSION,
                    shared_control_generation = self.shared_control.generation,
                    "refresh ownership acquired after waiting"
                );
                self.refresh_owner = Some(owner);
                clear_shared_refresh_requests(
                    &self.process_info,
                    "owner",
                    &mut self.shared_control,
                );
                self.automatic_refresh_task()
            }
            Err(error) => {
                tracing::error!(
                    pid = self.process_info.pid,
                    process_id = %self.process_info.id,
                    panel_output = ?self.process_info.panel_output,
                    owner_status = "read_only",
                    flatpak_status = self.process_info.flatpak_status(),
                    lock_path = %self.process_info.lock_path.display(),
                    config_version = Config::VERSION,
                    shared_control_generation = self.shared_control.generation,
                    error = %error,
                    "failed while waiting for refresh ownership"
                );
                Task::none()
            }
        }
    }

    fn automatic_refresh_task(&mut self) -> Task<Message> {
        let refresh_process = self.refresh_task_process();
        owner_automatic_refresh_task(
            self.refresh_owner.as_ref(),
            &self.process_info,
            self.owner_status(),
            &self.config,
            &mut self.state,
            refresh_process,
        )
    }

    fn handle_refresh_now(&mut self) -> Task<Message> {
        let requested_provider_count = ProviderId::ALL
            .into_iter()
            .filter(|provider| {
                self.state
                    .provider(*provider)
                    .is_some_and(|state| state.enabled)
            })
            .count();
        let shared_control = shared_control_with_user_refresh_requests(
            &self.state,
            &self.shared_control,
            &self.process_info.id,
        );
        match shared_state::save_control(APP_ID, &shared_control) {
            Ok(()) => {
                tracing::info!(
                    process_id = %self.process_info.id,
                    generation = shared_control.generation,
                    requested_provider_count,
                    "manual refresh requested"
                );
            }
            Err(error) => {
                tracing::error!(
                    pid = self.process_info.pid,
                    process_id = %self.process_info.id,
                    owner_status = self.owner_status(),
                    error = ?error,
                    "failed to save shared refresh requests"
                );
            }
        }
        self.handle_shared_control_update(shared_control)
    }

    fn handle_shared_control_update(
        &mut self,
        shared_control: SharedControlState,
    ) -> Task<Message> {
        if shared_control.generation <= self.shared_control.generation {
            return Task::none();
        }
        tracing::info!(
            process_id = %self.process_info.id,
            owner_status = if self.refresh_owner.is_some() { "owner" } else { "non_owner" },
            generation = shared_control.generation,
            request_count = shared_control.requests.len(),
            "shared control observed"
        );
        self.shared_control = shared_control;
        let refresh_process = self.refresh_task_process();
        let (task, consumed_providers) = owner_shared_control_refresh_task(
            self.refresh_owner.as_ref(),
            &self.process_info,
            self.owner_status(),
            &self.config,
            &mut self.state,
            &self.shared_control,
            refresh_process,
        );
        if !consumed_providers.is_empty() {
            self.consume_shared_refresh_requests(&consumed_providers);
        }
        task
    }

    fn handle_provider_account_statuses_refreshed(
        &mut self,
        provider: ProviderId,
        accounts: Vec<ProviderAccountRuntimeState>,
    ) {
        tracing::info!(
            process_id = %self.process_info.id,
            owner_status = self.owner_status(),
            provider = provider.label(),
            account_count = accounts.len(),
            "provider account statuses refreshed"
        );
        for account in accounts {
            self.state.upsert_account(account);
        }
        session::sync_metadata_after_status_refresh(self, provider);
        self.sync_panel_suggested_bounds();
        self.persist_runtime_if_owner("account_status_refresh");
    }
}

fn startup_task(
    demo_active: bool,
    refresh_task: Task<Message>,
    update_task: Task<Message>,
    cursor_status_task: Task<Message>,
    ownership_task: Task<Message>,
) -> Task<Message> {
    if demo_active {
        update_task
    } else {
        Task::batch([
            refresh_task,
            update_task,
            cursor_status_task,
            ownership_task,
        ])
    }
}

fn owner_automatic_refresh_task(
    refresh_owner: Option<&RefreshOwner>,
    process_info: &ProcessInfo,
    owner_status: &'static str,
    config: &Config,
    state: &mut AppState,
    process: RefreshProcessContext,
) -> Task<Message> {
    if refresh_owner.is_none() {
        return Task::none();
    }
    if runtime::resolve_stale_refreshes(state) {
        runtime::persist_state_as(
            state,
            "stale_refresh_resolved",
            Some(SharedStateWriter {
                process_id: &process_info.id,
                owner_status,
            }),
        );
    }
    let task = automatic_refresh_provider_tasks_for_process(config, state, Some(process));
    if task.units() > 0 {
        runtime::persist_state_as(
            state,
            "automatic_refresh_started",
            Some(SharedStateWriter {
                process_id: &process_info.id,
                owner_status,
            }),
        );
    }
    task
}

fn owner_shared_control_refresh_task(
    refresh_owner: Option<&RefreshOwner>,
    process_info: &ProcessInfo,
    owner_status: &'static str,
    config: &Config,
    state: &mut AppState,
    shared_control: &SharedControlState,
    process: RefreshProcessContext,
) -> (Task<Message>, Vec<ProviderId>) {
    if refresh_owner.is_none() {
        return (Task::none(), Vec::new());
    }

    let mut consumed_providers = Vec::new();
    let request_count = shared_control.requests.len();
    let mut evaluation = SharedRefreshEvaluationLog::from_requests(shared_control);
    let providers = shared_control
        .requests
        .iter()
        .filter_map(|request| {
            let force = matches!(
                request.reason,
                RefreshRequestReason::User | RefreshRequestReason::AccountAction
            );
            if !state
                .provider(request.provider)
                .is_some_and(|entry| entry.enabled)
            {
                evaluation.record_outcome(request.provider, "disabled");
                consumed_providers.push(request.provider);
                return None;
            }
            let Some(provider_state) = state.provider(request.provider) else {
                evaluation.record_outcome(request.provider, "missing_provider_state");
                return Some((request.provider, force));
            };
            if provider_state.is_refreshing {
                evaluation.record_outcome(request.provider, "already_refreshing");
                consumed_providers.push(request.provider);
                return None;
            }
            if provider_state.account_status != AccountSelectionStatus::Ready {
                let diagnostics = RefreshSkipDiagnostics::for_provider(state, request.provider);
                let skip_reason = diagnostics.not_ready_reason();
                evaluation.record_outcome(request.provider, skip_reason);
                consumed_providers.push(request.provider);
                return None;
            }
            Some((request.provider, force))
        })
        .collect::<Vec<_>>();

    let tasks = providers
        .into_iter()
        .map(|(provider, force)| {
            refresh_provider_task_for_process(config, state, provider, Some(process.clone()), force)
        })
        .filter(|task| task.units() > 0)
        .collect::<Vec<_>>();

    tracing::info!(
        process_id = %process_info.id,
        owner_status,
        generation = shared_control.generation,
        request_count,
        scheduled_provider_count = tasks.len(),
        skipped_provider_count = consumed_providers.len(),
        unresolved_provider_count = request_count
            .saturating_sub(tasks.len())
            .saturating_sub(consumed_providers.len()),
        request_reasons = %evaluation.request_reasons(),
        requesters = %evaluation.requesters(),
        outcomes = %evaluation.outcomes(),
        "owner evaluated shared refresh requests"
    );

    if tasks.is_empty() {
        (Task::none(), consumed_providers)
    } else {
        runtime::persist_state_as(
            state,
            "shared_refresh_started",
            Some(SharedStateWriter {
                process_id: &process_info.id,
                owner_status,
            }),
        );
        (Task::batch(tasks), consumed_providers)
    }
}

#[derive(Default)]
struct SharedRefreshEvaluationLog {
    user_request_count: usize,
    account_action_request_count: usize,
    provider_selected_request_count: usize,
    requesters: Vec<String>,
    outcomes: Vec<String>,
}

impl SharedRefreshEvaluationLog {
    fn from_requests(shared_control: &SharedControlState) -> Self {
        let mut summary = Self::default();
        for request in &shared_control.requests {
            if !summary.requesters.contains(&request.requesting_process_id) {
                summary
                    .requesters
                    .push(request.requesting_process_id.clone());
            }
            match request.reason {
                RefreshRequestReason::User => summary.user_request_count += 1,
                RefreshRequestReason::AccountAction => summary.account_action_request_count += 1,
                RefreshRequestReason::ProviderSelected => {
                    summary.provider_selected_request_count += 1;
                }
            }
        }
        summary
    }

    fn record_outcome(&mut self, provider: ProviderId, outcome: &str) {
        self.outcomes
            .push(format!("{}:{outcome}", provider.label()));
    }

    fn request_reasons(&self) -> String {
        let mut reasons = Vec::new();
        if self.user_request_count > 0 {
            reasons.push(format!("user:{}", self.user_request_count));
        }
        if self.account_action_request_count > 0 {
            reasons.push(format!(
                "account_action:{}",
                self.account_action_request_count
            ));
        }
        if self.provider_selected_request_count > 0 {
            reasons.push(format!(
                "provider_selected:{}",
                self.provider_selected_request_count
            ));
        }
        if reasons.is_empty() {
            "none".to_string()
        } else {
            reasons.join(",")
        }
    }

    fn requesters(&self) -> String {
        if self.requesters.is_empty() {
            "none".to_string()
        } else {
            self.requesters.join(",")
        }
    }

    fn outcomes(&self) -> String {
        if self.outcomes.is_empty() {
            "none".to_string()
        } else {
            self.outcomes.join(",")
        }
    }
}

fn shared_control_with_user_refresh_requests(
    state: &AppState,
    shared_control: &SharedControlState,
    process_id: &str,
) -> SharedControlState {
    let mut next = shared_control.clone();
    for provider in ProviderId::ALL {
        if !state.provider(provider).is_some_and(|entry| entry.enabled) {
            continue;
        }
        next.upsert_request(ProviderRefreshRequest {
            provider,
            reason: RefreshRequestReason::User,
            requested_at: Utc::now(),
            requesting_process_id: process_id.to_string(),
        });
    }
    next
}

#[derive(Debug, Clone, Copy)]
struct StartupDiagnostics {
    shared_runtime_generation: Option<u64>,
    shared_control_generation: u64,
}

impl StartupDiagnostics {
    fn new(shared_runtime_generation: Option<u64>, shared_control_generation: u64) -> Self {
        Self {
            shared_runtime_generation,
            shared_control_generation,
        }
    }
}

fn initialize_refresh_ownership(
    process_info: &ProcessInfo,
    diagnostics: &StartupDiagnostics,
    shared_control: &mut SharedControlState,
) -> (Option<RefreshOwner>, Task<Message>) {
    match refresh_owner::try_acquire(process_info.lock_path.clone()) {
        Ok(RefreshOwnerAttempt::Owner(owner)) => {
            clear_shared_refresh_requests(process_info, "owner", shared_control);
            (Some(owner), Task::none())
        }
        Ok(RefreshOwnerAttempt::NonOwner(waiter)) => (None, refresh_owner_wait_task(waiter)),
        Err(error) => {
            tracing::error!(
                pid = process_info.pid,
                process_id = %process_info.id,
                panel_output = ?process_info.panel_output,
                owner_status = "read_only",
                flatpak_status = process_info.flatpak_status(),
                lock_path = %process_info.lock_path.display(),
                config_version = Config::VERSION,
                shared_runtime_generation = ?diagnostics.shared_runtime_generation,
                shared_control_generation = diagnostics.shared_control_generation,
                error = ?error,
                "failed to acquire refresh ownership lock"
            );
            (None, Task::none())
        }
    }
}

fn refresh_owner_wait_task(waiter: RefreshOwnerWaiter) -> Task<Message> {
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || waiter.wait())
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result.map_err(|error| error.to_string()))
        },
        |result| cosmic::Action::App(Message::RefreshOwnershipAcquired(result)),
    )
}

fn clear_shared_refresh_requests(
    process_info: &ProcessInfo,
    owner_status: &'static str,
    shared_control: &mut SharedControlState,
) {
    let had_requests = !shared_control.requests.is_empty();
    match shared_state::clear_control_requests(APP_ID, shared_control) {
        Ok(cleared) => {
            *shared_control = cleared;
            if had_requests {
                tracing::info!(
                    pid = process_info.pid,
                    process_id = %process_info.id,
                    owner_status,
                    generation = shared_control.generation,
                    "shared refresh requests cleared by refresh owner"
                );
            }
        }
        Err(error) => {
            tracing::error!(
                pid = process_info.pid,
                process_id = %process_info.id,
                owner_status,
                error = ?error,
                "failed to clear shared refresh requests"
            );
        }
    }
}

fn refresh_task_process(
    process_info: &ProcessInfo,
    owner_status: &'static str,
) -> RefreshProcessContext {
    RefreshProcessContext {
        process_id: process_info.id.clone(),
        owner_status,
    }
}
