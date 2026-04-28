// SPDX-License-Identifier: MPL-2.0

mod applet;
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
use crate::providers::cursor::{self, CursorLoginEvent, CursorLoginState, CursorLoginStatus};
use crate::providers::interface::ProviderDiscoveredAccount;
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
const APPLET_PERCENT_DIGITS: f32 = 7.0;
const APPLET_PERCENT_GLYPH_WIDTH: f32 = 8.0;
const APPLET_PERCENT_TEXT_WIDTH: f32 = APPLET_PERCENT_DIGITS * APPLET_PERCENT_GLYPH_WIDTH;
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
    cursor_login: Option<CursorLoginState>,
    cursor_login_handle: Option<Handle>,
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
    AuthFileChanged(ProviderId),
    ProviderRefreshed(Box<ProviderRefreshResult>),
    SelectProvider(ProviderId),
    NavigateTo(PopupRoute),
    SetProviderEnabled(ProviderId, bool),
    ToggleAccountSelection(ProviderId, String),
    DeleteCodexAccount(String),
    StartCodexLogin,
    CancelCodexLogin,
    CodexLoginEvent(Box<CodexLoginEvent>),
    DeleteClaudeAccount(String),
    StartClaudeLogin,
    CancelClaudeLogin,
    ClaudeLoginEvent(Box<ClaudeLoginEvent>),
    DeleteCursorAccount(String),
    ReauthenticateCursorAccount(String),
    StartCursorLogin,
    CancelCursorLogin,
    CursorLoginEvent(Box<CursorLoginEvent>),
    ProviderAccountsDiscovered(ProviderId, Vec<ProviderDiscoveredAccount>),
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
        registry::startup_cleanup();

        let config = cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
            .map(|ctx| {
                let mut config = match Config::get_entry(&ctx) {
                    Ok(cfg) => cfg.with_env_overrides(),
                    Err((_errors, cfg)) => cfg.with_env_overrides(),
                };
                let mut changed = registry::startup_sync(&mut config);
                changed |= registry::initialize_provider_visibility(&mut config, &ProviderId::ALL);
                if changed {
                    let _ = config.write_entry(&ctx);
                }
                #[cfg(debug_assertions)]
                registry::startup_debug_apply(&config);
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
        let n_accounts_init = state.selected_accounts(selected_provider).len().max(1);
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
            cursor_login: None,
            cursor_login_handle: None,
        };

        let update_task = update_check_task(0);
        let discover_task = {
            let config = initial_config.clone();
            let client = crate::runtime::http_client();
            Task::perform(
                async move { registry::browser_account_discovery(config, client).await },
                |accounts| {
                    cosmic::Action::App(Message::ProviderAccountsDiscovered(
                        ProviderId::Cursor,
                        accounts,
                    ))
                },
            )
        };

        let startup = if demo_env::is_active() {
            Task::none()
        } else {
            Task::batch([refresh_task, update_task, discover_task, cursor_status_task])
        };

        (app, startup)
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let n_accounts = self
            .state
            .selected_accounts(self.selected_provider)
            .len()
            .max(1);
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
                cursor: self.cursor_login.as_ref(),
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
                .map(|update| Message::UpdateConfig(Box::new(update.config.with_env_overrides()))),
            time::every(Duration::from_secs(interval_secs)).map(|_| Message::Tick),
            crate::file_watcher::subscription().map(|ev| match ev {
                crate::file_watcher::WatcherEvent::AuthFileChanged(p) => {
                    Message::AuthFileChanged(p)
                }
            }),
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

            Message::AuthFileChanged(provider) => {
                return Some(self.handle_auth_file_changed(provider));
            }

            Message::ProviderRefreshed(refresh_result) => {
                return Some(self.handle_provider_refreshed(*refresh_result));
            }

            Message::ProviderAccountsDiscovered(provider, accounts) => {
                return Some(self.handle_provider_accounts_discovered(provider, &accounts));
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

            Message::StartCodexLogin => return Some(self.start_codex_login()),

            Message::CancelCodexLogin => self.cancel_codex_login(),

            Message::CodexLoginEvent(event) => {
                return Some(self.handle_codex_login_event(*event));
            }

            Message::StartClaudeLogin => {
                return Some(self.start_claude_login());
            }

            Message::CancelClaudeLogin => self.cancel_claude_login(),

            Message::ClaudeLoginEvent(event) => {
                return Some(self.handle_claude_login_event(*event));
            }

            Message::DeleteCursorAccount(_)
            | Message::ReauthenticateCursorAccount(_)
            | Message::StartCursorLogin
            | Message::CancelCursorLogin
            | Message::CursorLoginEvent(_) => unreachable!(),
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
            Message::StartCursorLogin => {
                CursorMessageResult::handled(Some(self.start_cursor_login()))
            }
            Message::CancelCursorLogin => {
                self.cancel_cursor_login();
                CursorMessageResult::handled(None)
            }
            Message::CursorLoginEvent(event) => CursorMessageResult::handled(Some(
                self.handle_cursor_login_event((**event).clone()),
            )),
            _ => CursorMessageResult::Unhandled,
        }
    }

    fn handle_provider_accounts_discovered(
        &mut self,
        provider: ProviderId,
        new_accounts: &[ProviderDiscoveredAccount],
    ) -> Task<Message> {
        let should_finalize_provider_visibility = provider == ProviderId::Cursor
            && self.config.provider_visibility_mode
                == crate::config::ProviderVisibilityMode::AutoInitPending;
        self.write_config(|config| {
            if !new_accounts.is_empty() {
                registry::upsert_discovered_accounts(config, new_accounts);
            }
            if should_finalize_provider_visibility {
                registry::initialize_provider_visibility(config, &[ProviderId::Cursor]);
                registry::finalize_provider_visibility_initialization(config);
            }
        });
        #[cfg(debug_assertions)]
        registry::startup_debug_apply_for_accounts(
            &new_accounts
                .iter()
                .filter_map(|account| match &account.handle {
                    crate::providers::interface::ProviderAccountHandle::Cursor(account) => {
                        Some(account.clone())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>(),
        );
        runtime::reconcile_provider(&self.config, &mut self.state, provider);
        self.selected_provider = select_provider(self.selected_provider, &self.state);
        let refresh_task = self
            .config
            .provider_enabled(provider)
            .then(|| refresh_provider_task(&self.config, &mut self.state, provider));
        let status_task =
            refresh_provider_account_statuses_task(&self.config, &self.state, provider);
        match refresh_task {
            Some(refresh_task) if status_task.units() > 0 => {
                Task::batch([refresh_task, status_task])
            }
            Some(refresh_task) => refresh_task,
            None => status_task,
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
