// SPDX-License-Identifier: MPL-2.0

mod applet;
mod host_auth_watch;
mod login;
mod popup_view;
mod provider_actions;
mod provider_assets;
mod refresh;
mod state;
#[cfg(test)]
mod tests;
mod window;

pub(crate) use self::applet::applet_settings;
use self::applet::{applet_button, applet_button_size, applet_indicator, select_provider};
use self::popup_view::ProviderLoginStates;
use self::provider_assets::{provider_icon_handle, provider_icon_variant};
use self::refresh::{
    refresh_provider_account_statuses_task, refresh_provider_task, refresh_provider_tasks,
};
use self::window::{
    format_retry_delay, open_url, popup_size_limits_with_max_width, popup_size_tuple, resize_popup,
    update_check_task, update_retry_delay, update_retry_task,
};
use crate::config::{
    Config, ManagedClaudeAccountConfig, ManagedCodexAccountConfig, ManagedCursorAccountConfig,
    PanelIconStyle, ResetTimeFormat, UsageAmountFormat,
};
use crate::demo_env;
use crate::model::{
    AccountSelectionStatus, AppState, ProviderAccountRuntimeState, ProviderHealth, ProviderId,
};
use crate::providers::claude::{self, ClaudeLoginEvent, ClaudeLoginState, ClaudeLoginStatus};
use crate::providers::codex::{self, CodexLoginEvent, CodexLoginState, CodexLoginStatus};
use crate::providers::cursor::{self, CursorScanResult, CursorScanState};
use crate::providers::registry;
use crate::runtime;
use crate::runtime::ProviderRefreshResult;
use crate::updates::UpdateStatus;
use crate::usage_display;
use cosmic::app::Task;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::task::Handle;
use cosmic::iced::time;
use cosmic::iced::widget::{progress_bar, row};
use cosmic::iced::window::Id;
use cosmic::iced::{Alignment, Background, Length, Limits, Shadow, Size, Subscription};
use cosmic::prelude::*;
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::theme::Button as CosmicButton;
use cosmic::widget;
use std::time::Duration;

const REFRESH_INTERVAL_MIN_SECS: u64 = 10;
const POPUP_MAX_HEIGHT: u16 = 1080;
const APPLET_BAR_WIDTH_HEIGHT_MULTIPLIER: u16 = 2;
const APPLET_ICON_GAP: f32 = 6.0;
const APPLET_ACCOUNT_GAP: f32 = 4.0;
const APPLET_PERCENT_ACCOUNT_GAP: f32 = 4.0;
const APPLET_PERCENT_GLYPH_WIDTH: f32 = 7.25;
const APPLET_PERCENT_CELL_HORIZONTAL_PAD: f32 = 8.0;
const UPDATE_RETRY_INITIAL_SECS: u64 = 15;
const UPDATE_RETRY_MAX_SECS: u64 = 15 * 60;

pub struct AppModel {
    core: cosmic::Core,
    popup: Option<Id>,
    config: Config,
    state: AppState,
    selected_provider: ProviderId,
    popup_route: PopupRoute,
    update_status: UpdateStatus,
    launch_mode: LaunchMode,
    popup_size: Option<Size>,
    codex_login: Option<CodexLoginState>,
    codex_login_handle: Option<Handle>,
    claude_login: Option<ClaudeLoginState>,
    claude_login_handle: Option<Handle>,
    cursor_scan: CursorScanState,
    cursor_scan_result: Option<CursorScanResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    Panel,
    Standalone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PopupRoute {
    ProviderDetail,
    Settings(SettingsRoute),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsRoute {
    General,
    Provider(ProviderId),
}

#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    UpdateConfig(Box<Config>),
    Tick,
    RefreshNow,
    ProviderRefreshed(Box<ProviderRefreshResult>),
    SelectProvider(ProviderId),
    NavigateTo(PopupRoute),
    SetProviderEnabled(ProviderId, bool),
    ToggleAccountSelection(ProviderId, String),
    DeleteCodexAccount(String),
    ReauthenticateCodexAccount(String),
    StartCodexLogin,
    CancelCodexLogin,
    CodexLoginEvent(Box<CodexLoginEvent>),
    DeleteClaudeAccount(String),
    ReauthenticateClaudeAccount(String),
    StartClaudeLogin,
    UpdateClaudeLoginCode(String),
    SubmitClaudeLoginCode,
    CancelClaudeLogin,
    ClaudeLoginEvent(Box<ClaudeLoginEvent>),
    DeleteCursorAccount(String),
    ReauthenticateCursorAccount(String),
    StartCursorScan,
    ConfirmCursorScan,
    DismissCursorScan,
    CursorScanComplete(CursorScanState, Option<CursorScanResult>),
    ProviderAccountStatusesRefreshed(ProviderId, Vec<ProviderAccountRuntimeState>),
    SetRefreshInterval(u64),
    SetResetTimeFormat(ResetTimeFormat),
    SetUsageAmountFormat(UsageAmountFormat),
    SetPanelIconStyle(PanelIconStyle),
    SetShowAllAccounts(ProviderId, bool),
    CheckUpdates,
    UpdateChecked { status: UpdateStatus, attempt: u32 },
    RetryUpdateCheck(u32),
    OpenUrl(String),
    Quit,
    HostCliAuthChanged,
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;
    type Flags = LaunchMode;
    type Message = Message;

    const APP_ID: &'static str = "com.topi.YapCap";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(mut core: cosmic::Core, launch_mode: Self::Flags) -> (Self, Task<Self::Message>) {
        core.window.show_headerbar = false;
        core.window.sharp_corners = true;
        core.window.show_maximize = false;
        core.window.show_minimize = false;
        core.window.use_template = false;

        let config = cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
            .map(|ctx| {
                let mut config = match Config::get_entry(&ctx) {
                    Ok(cfg) => cfg,
                    Err((_errors, cfg)) => cfg,
                };
                let mut changed = registry::startup_sync(&mut config);
                changed |= registry::initialize_provider_visibility(&mut config, &ProviderId::ALL);
                changed |= registry::finalize_provider_visibility_initialization(&mut config);
                if changed {
                    let _ = config.write_entry(&ctx);
                }
                demo_env::apply_config(&mut config);
                config
            })
            .unwrap_or_default();

        let initial_config = config.clone();
        let mut state = runtime::load_initial_state(&initial_config);
        #[cfg(debug_assertions)]
        crate::debug_env::apply(&mut state);
        demo_env::apply(&initial_config, &mut state);
        let selected_provider = select_provider(ProviderId::Codex, &state);
        let refresh_task = refresh_provider_tasks(&initial_config, &mut state);
        let cursor_status_task =
            refresh_provider_account_statuses_task(&initial_config, &state, ProviderId::Cursor);
        let n_accounts_init = state.display_selected_account_count(selected_provider);
        let (applet_width, applet_height) =
            applet_button_size(&core, initial_config.panel_icon_style, n_accounts_init);
        core.applet.suggested_bounds = Some(Size::new(applet_width, applet_height));
        let app = AppModel {
            core,
            popup: None,
            config,
            state,
            selected_provider,
            popup_route: PopupRoute::ProviderDetail,
            update_status: UpdateStatus::Unchecked,
            launch_mode,
            popup_size: None,
            codex_login: None,
            codex_login_handle: None,
            claude_login: None,
            claude_login_handle: None,
            cursor_scan: CursorScanState::Idle,
            cursor_scan_result: None,
        };

        let update_task = update_check_task(0);
        let startup = if demo_env::is_active() {
            Task::none()
        } else {
            Task::batch([refresh_task, update_task, cursor_status_task])
        };

        (app, startup)
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let n_accounts = self
            .state
            .display_selected_account_count(self.selected_provider);
        let indicator = applet_indicator(
            &self.state,
            self.selected_provider,
            self.config.panel_icon_style,
            self.config.usage_amount_format,
            &self.core,
            n_accounts,
        );
        let button: Element<'_, Message> = applet_button(
            &self.core,
            self.config.panel_icon_style,
            n_accounts,
            indicator,
        )
        .on_press(Message::TogglePopup)
        .into();

        match self.launch_mode {
            LaunchMode::Panel => self.core.applet.autosize_window(button).into(),
            LaunchMode::Standalone => button,
        }
    }

    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        let popup_size = self
            .popup_size
            .unwrap_or_else(|| popup_view::popup_session_size(&self.state, self.selected_provider));
        let content = popup_view::popup_content(
            &self.state,
            &self.config,
            ProviderLoginStates {
                codex: self.codex_login.as_ref(),
                claude: self.claude_login.as_ref(),
                cursor_scan: &self.cursor_scan,
            },
            self.selected_provider,
            &self.popup_route,
            &self.update_status,
        );
        widget::container(content)
            .width(Length::Fixed(popup_size.width))
            .height(Length::Fixed(popup_size.height))
            .style(|theme| {
                let cosmic = theme.cosmic();
                let corners = cosmic.corner_radii;
                widget::container::Style {
                    text_color: Some(cosmic.background.on.into()),
                    background: Some(Background::Color(cosmic.background.base.into())),
                    border: cosmic::iced::Border {
                        radius: corners.radius_m.into(),
                        width: 1.0,
                        color: cosmic.background.divider.into(),
                    },
                    shadow: Shadow::default(),
                    icon_color: Some(cosmic.background.on.into()),
                    snap: true,
                }
            })
            .into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        let interval_secs = self
            .config
            .refresh_interval_seconds
            .max(REFRESH_INTERVAL_MIN_SECS);

        Subscription::batch(vec![
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| Message::UpdateConfig(Box::new(update.config))),
            time::every(Duration::from_secs(interval_secs)).map(|_| Message::Tick),
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
        if let CursorMessageResult::Handled(task) = self.handle_cursor_message(&message) {
            return task;
        }
        match message {
            Message::UpdateConfig(config) => {
                self.on_config_update(*config);
            }
            Message::TogglePopup => {
                return Some(self.toggle_popup());
            }
            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                    self.popup_size = None;
                }
            }
            Message::Tick | Message::RefreshNow => {
                return Some(refresh_provider_tasks(&self.config, &mut self.state));
            }
            Message::ProviderRefreshed(refresh_result) => {
                return Some(self.handle_provider_refreshed(*refresh_result));
            }
            Message::ProviderAccountStatusesRefreshed(provider, accounts) => {
                self.handle_provider_account_statuses_refreshed(provider, accounts);
            }
            Message::SelectProvider(provider) => {
                return self.select_provider_tab(provider);
            }
            Message::NavigateTo(route) => {
                return self.navigate_to(route);
            }
            Message::UpdateChecked { status, attempt } => {
                return Some(self.handle_update_checked(status, attempt));
            }
            Message::CheckUpdates => {
                self.update_status = UpdateStatus::Unchecked;
                return Some(update_check_task(0));
            }
            Message::RetryUpdateCheck(attempt) => {
                if matches!(self.update_status, UpdateStatus::Error(_)) {
                    return Some(update_check_task(attempt));
                }
            }
            Message::OpenUrl(url) => open_url(&url),
            Message::Quit => std::process::exit(0),
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
            Message::SetShowAllAccounts(provider, show_all) => {
                return Some(self.set_show_all_accounts(provider, show_all));
            }
            Message::ToggleAccountSelection(provider, account_id) => {
                return Some(self.toggle_account_selection(provider, &account_id));
            }
            Message::DeleteCodexAccount(account_id) => {
                return Some(self.delete_codex_account(&account_id));
            }
            Message::DeleteClaudeAccount(account_id) => {
                return Some(self.delete_claude_account(&account_id));
            }
            Message::ReauthenticateClaudeAccount(account_id) => {
                return Some(self.reauthenticate_claude_account(&account_id));
            }
            Message::ReauthenticateCodexAccount(account_id) => {
                return Some(self.reauthenticate_codex_account(&account_id));
            }
            Message::StartCodexLogin => return Some(self.start_codex_login()),
            Message::CancelCodexLogin => self.cancel_codex_login(),
            Message::CodexLoginEvent(event) => return Some(self.handle_codex_login_event(*event)),
            Message::StartClaudeLogin => return Some(self.start_claude_login()),
            Message::UpdateClaudeLoginCode(code) => self.update_claude_login_code(code),
            Message::SubmitClaudeLoginCode => return Some(self.submit_claude_login_code()),
            Message::CancelClaudeLogin => self.cancel_claude_login(),
            Message::ClaudeLoginEvent(event) => {
                return Some(self.handle_claude_login_event(*event));
            }
            Message::DeleteCursorAccount(_)
            | Message::ReauthenticateCursorAccount(_)
            | Message::StartCursorScan
            | Message::ConfirmCursorScan
            | Message::DismissCursorScan
            | Message::CursorScanComplete(_, _) => unreachable!(),
        }
        None
    }

    fn handle_cursor_message(&mut self, message: &Message) -> CursorMessageResult {
        match message {
            Message::DeleteCursorAccount(account_id) => {
                CursorMessageResult::handled(Some(self.delete_cursor_account(account_id)))
            }
            Message::ReauthenticateCursorAccount(account_id) => {
                CursorMessageResult::handled(Some(self.reauthenticate_cursor_account(account_id)))
            }
            Message::StartCursorScan => {
                CursorMessageResult::handled(Some(self.start_cursor_scan()))
            }
            Message::ConfirmCursorScan => {
                CursorMessageResult::handled(Some(self.confirm_cursor_scan()))
            }
            Message::DismissCursorScan => {
                self.dismiss_cursor_scan();
                CursorMessageResult::handled(None)
            }
            Message::CursorScanComplete(state, result) => {
                self.handle_cursor_scan_complete(state.clone(), result.clone());
                CursorMessageResult::handled(None)
            }
            _ => CursorMessageResult::Unhandled,
        }
    }

    fn handle_provider_account_statuses_refreshed(
        &mut self,
        provider: ProviderId,
        accounts: Vec<ProviderAccountRuntimeState>,
    ) {
        for account in accounts {
            self.state.upsert_account(account);
        }
        if provider == ProviderId::Cursor {
            self.update_cursor_metadata_from_state();
            self.update_cursor_active_account();
        }
        self.sync_panel_suggested_bounds();
        runtime::persist_state(&self.state);
    }
}

enum CursorMessageResult {
    Handled(Option<Task<Message>>),
    Unhandled,
}

impl CursorMessageResult {
    fn handled(task: Option<Task<Message>>) -> Self {
        Self::Handled(task)
    }
}
