// SPDX-License-Identifier: MPL-2.0

use crate::app_refresh::{
    refresh_provider_account_statuses_task, refresh_provider_task, refresh_provider_tasks,
};
use crate::config::{
    Config, ManagedClaudeAccountConfig, ManagedCodexAccountConfig, ManagedCursorAccountConfig,
    PanelIconStyle, ResetTimeFormat, UsageAmountFormat, paths,
};
use crate::demo_env;
use crate::model::{
    AccountSelectionStatus, AppState, ProviderAccountRuntimeState, ProviderHealth, ProviderId,
};
use crate::popup_view;
use crate::provider_assets::{provider_icon_handle, provider_icon_variant};
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
use cosmic::iced::widget::{column, progress_bar, row};
use cosmic::iced::window::Id;
use cosmic::iced::{Alignment, Background, Length, Limits, Shadow, Size, Subscription};
use cosmic::prelude::*;
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::theme::Button as CosmicButton;
use cosmic::widget;
use std::path::Path;
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
            popup_view::ProviderLoginStates {
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

            Message::OpenUrl(url) => {
                open_url(&url);
            }

            Message::Quit => {
                std::process::exit(0);
            }

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

            Message::DeleteCodexAccount(account_id) => {
                return Some(self.delete_codex_account(&account_id));
            }

            Message::DeleteClaudeAccount(account_id) => {
                return Some(self.delete_claude_account(&account_id));
            }

            Message::StartCodexLogin => return Some(self.start_codex_login()),

            Message::CancelCodexLogin => {
                self.cancel_codex_login();
            }

            Message::CodexLoginEvent(event) => {
                return Some(self.handle_codex_login_event(*event));
            }

            Message::StartClaudeLogin => {
                return Some(self.start_claude_login());
            }

            Message::CancelClaudeLogin => {
                self.cancel_claude_login();
            }

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

pub fn applet_settings() -> cosmic::app::Settings {
    let preview_core = cosmic::Core::default();
    let config =
        cosmic_config::Config::new(<AppModel as cosmic::Application>::APP_ID, Config::VERSION)
            .ok()
            .map(|ctx| match Config::get_entry(&ctx) {
                Ok(cfg) | Err((_, cfg)) => cfg,
            })
            .unwrap_or_default();
    let n_accounts = ProviderId::ALL
        .iter()
        .filter(|&&p| config.provider_enabled(p))
        .map(|&p| config.selected_account_ids(p).len().max(1))
        .max()
        .unwrap_or(1);
    let (width, height) = applet_button_size(&preview_core, config.panel_icon_style, n_accounts);

    cosmic::app::Settings::default()
        .size(Size::new(width, height))
        .size_limits(
            Limits::NONE
                .min_width(width)
                .max_width(width)
                .min_height(height)
                .max_height(height),
        )
        .resizable(None)
        .client_decorations(false)
        .default_text_size(14.0)
        .transparent(true)
}

fn applet_indicator<'a>(
    state: &AppState,
    selected_provider: ProviderId,
    style: PanelIconStyle,
    usage_amount_format: UsageAmountFormat,
    core: &cosmic::Core,
    n_accounts: usize,
) -> Element<'a, Message> {
    let (suggested_w, suggested_h) = core.applet.suggested_size(false);
    let compact_px = suggested_w.min(suggested_h);
    let logo_size_px = compact_px.saturating_sub(8).max(11);
    let logo_size = f32::from(logo_size_px);
    let bar_width = applet_bar_width(suggested_w, suggested_h);
    let percent = selected_provider_percent(state, selected_provider, usage_amount_format);

    let account_percents =
        selected_provider_all_percents(state, selected_provider, usage_amount_format);

    let bars_row = {
        let mut r = row![].align_y(Alignment::Center);
        for i in 0..n_accounts {
            if i > 0 {
                r = r.push(
                    cosmic::iced::widget::Space::new().width(Length::Fixed(APPLET_ACCOUNT_GAP)),
                );
            }
            let (p0, p1) = account_percents.get(i).copied().unwrap_or((0.0, 0.0));
            r = r.push(
                column![
                    progress_bar(0.0..=100.0, p0).girth(Length::Fixed(6.0)),
                    progress_bar(0.0..=100.0, p1).girth(Length::Fixed(3.0)),
                ]
                .spacing(3)
                .width(Length::Fixed(bar_width)),
            );
        }
        r
    };

    match style {
        PanelIconStyle::LogoAndBars => row![
            provider_logo(selected_provider, logo_size_px, logo_size),
            bars_row,
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into(),
        PanelIconStyle::BarsOnly => bars_row.into(),
        PanelIconStyle::LogoAndPercent => row![
            provider_logo(selected_provider, logo_size_px, logo_size),
            widget::text(applet_percent_text(percent)).size(13),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into(),
        PanelIconStyle::PercentOnly => widget::text(applet_percent_text(percent)).size(13).into(),
    }
}

fn provider_logo<'a>(
    provider: ProviderId,
    logo_size_px: u16,
    logo_size: f32,
) -> Element<'a, Message> {
    widget::icon::icon(provider_icon_handle(provider, provider_icon_variant()))
        .size(logo_size_px)
        .width(Length::Fixed(logo_size))
        .height(Length::Fixed(logo_size))
        .into()
}

fn applet_button<'a>(
    core: &cosmic::Core,
    style: PanelIconStyle,
    n_accounts: usize,
    content: impl Into<Element<'a, Message>>,
) -> widget::Button<'a, Message> {
    let (major_padding, minor_padding) = core.applet.suggested_padding(true);
    let horizontal_padding = if core.applet.is_horizontal() {
        major_padding
    } else {
        minor_padding
    };
    let (width, height) = applet_button_size(core, style, n_accounts);

    widget::button::custom(
        widget::layer_container(content)
            .padding(cosmic::iced::Padding::from([0, horizontal_padding]))
            .align_y(cosmic::iced::alignment::Vertical::Center.into()),
    )
    .padding(0)
    .width(Length::Fixed(width))
    .height(Length::Fixed(height))
    .class(CosmicButton::AppletIcon)
}

fn applet_button_size(core: &cosmic::Core, style: PanelIconStyle, n_accounts: usize) -> (f32, f32) {
    let (suggested_w, suggested_h) = core.applet.suggested_size(false);
    let (major_padding, minor_padding) = core.applet.suggested_padding(true);
    let (horizontal_padding, vertical_padding) = if core.applet.is_horizontal() {
        (major_padding, minor_padding)
    } else {
        (minor_padding, major_padding)
    };
    let compact_px = suggested_w.min(suggested_h);
    let logo_width = f32::from(compact_px.saturating_sub(8).max(11));
    let bar_width = applet_bar_width(suggested_w, suggested_h);
    let n = f32::from(u8::try_from(n_accounts.max(1)).unwrap_or(u8::MAX));
    let bars_total = n * bar_width + (n - 1.0) * APPLET_ACCOUNT_GAP;
    let content_width = match style {
        PanelIconStyle::LogoAndBars => logo_width + APPLET_ICON_GAP + bars_total,
        PanelIconStyle::BarsOnly => bars_total,
        PanelIconStyle::LogoAndPercent => logo_width + APPLET_ICON_GAP + APPLET_PERCENT_TEXT_WIDTH,
        PanelIconStyle::PercentOnly => APPLET_PERCENT_TEXT_WIDTH,
    };
    let width = content_width + f32::from(2 * horizontal_padding);
    let height = f32::from(suggested_h + 2 * vertical_padding);

    (width, height)
}

fn applet_bar_width(suggested_w: u16, suggested_h: u16) -> f32 {
    let min_width = suggested_h.saturating_mul(APPLET_BAR_WIDTH_HEIGHT_MULTIPLIER);

    f32::from(suggested_w.max(min_width))
}

fn applet_percent_text(percent: f32) -> String {
    format!("{percent:.1}%")
}

fn selected_provider_all_percents(
    state: &AppState,
    selected_provider: ProviderId,
    usage_amount_format: UsageAmountFormat,
) -> Vec<(f32, f32)> {
    let now = chrono::Utc::now();
    let accounts = state.selected_accounts(selected_provider);
    if accounts.is_empty() {
        let snapshot = state
            .provider(selected_provider)
            .and_then(|p| p.legacy_display_snapshot.as_ref());
        let (w0, w1) = snapshot.map_or((None, None), |s| s.applet_windows());
        let p0 = w0.map_or(0.0, |w| {
            usage_display::displayed_amount_percent(w, now, usage_amount_format)
        });
        let p1 = w1.map_or(0.0, |w| {
            usage_display::displayed_amount_percent(w, now, usage_amount_format)
        });
        return vec![(p0, p1)];
    }
    accounts
        .iter()
        .map(|account| {
            let (w0, w1) = account
                .snapshot
                .as_ref()
                .map_or((None, None), |s| s.applet_windows());
            let p0 = w0.map_or(0.0, |w| {
                usage_display::displayed_amount_percent(w, now, usage_amount_format)
            });
            let p1 = w1.map_or(0.0, |w| {
                usage_display::displayed_amount_percent(w, now, usage_amount_format)
            });
            (p0, p1)
        })
        .collect()
}

fn selected_provider_percent(
    state: &AppState,
    selected_provider: ProviderId,
    usage_amount_format: UsageAmountFormat,
) -> f32 {
    let now = chrono::Utc::now();
    state
        .providers
        .iter()
        .find(|p| p.provider == selected_provider)
        .and_then(|p| {
            state
                .active_account(p.provider)
                .and_then(|account| account.snapshot.as_ref())
                .or(p.legacy_display_snapshot.as_ref())
        })
        .and_then(|snapshot| snapshot.applet_windows().0)
        .map_or(0.0, |window| {
            usage_display::displayed_amount_percent(window, now, usage_amount_format)
        })
}

fn select_provider(current: ProviderId, state: &AppState) -> ProviderId {
    if state
        .providers
        .iter()
        .any(|p| p.provider == current && p.enabled)
    {
        current
    } else {
        state
            .providers
            .iter()
            .find(|p| p.enabled)
            .map_or(ProviderId::Codex, |p| p.provider)
    }
}

fn remove_managed_codex_home(codex_home: &Path) {
    let root = paths().codex_accounts_dir;
    let Ok(root) = root.canonicalize() else {
        return;
    };
    let Ok(metadata) = std::fs::symlink_metadata(codex_home) else {
        return;
    };
    if metadata.file_type().is_symlink() {
        tracing::warn!(path = %codex_home.display(), "refusing to delete symlinked codex account home");
        return;
    }
    let Ok(home) = codex_home.canonicalize() else {
        return;
    };
    if !home.starts_with(&root) {
        tracing::warn!(path = %home.display(), root = %root.display(), "refusing to delete codex account outside managed root");
        return;
    }
    if let Err(error) = std::fs::remove_dir_all(&home) {
        tracing::warn!(path = %home.display(), error = %error, "failed to delete codex account home");
    }
}

fn open_url(url: &str) {
    if let Err(e) = std::process::Command::new("xdg-open").arg(url).spawn() {
        tracing::warn!(url = %url, error = %e, "failed to open url");
    }
}

fn update_check_task(attempt: u32) -> Task<Message> {
    Task::perform(
        async { crate::updates::check(&runtime::http_client()).await },
        move |status| cosmic::Action::App(Message::UpdateChecked { status, attempt }),
    )
}

fn update_retry_task(attempt: u32, delay: Duration) -> Task<Message> {
    Task::perform(
        async move {
            tokio::time::sleep(delay).await;
            attempt
        },
        |attempt| cosmic::Action::App(Message::RetryUpdateCheck(attempt)),
    )
}

fn update_retry_delay(attempt: u32) -> Duration {
    let exponent = attempt.saturating_sub(1).min(10);
    let secs = UPDATE_RETRY_INITIAL_SECS
        .saturating_mul(2_u64.saturating_pow(exponent))
        .min(UPDATE_RETRY_MAX_SECS);
    Duration::from_secs(secs)
}

fn format_retry_delay(delay: Duration) -> String {
    let secs = delay.as_secs();
    if secs < 60 {
        return format!("{secs}s");
    }
    let minutes = secs / 60;
    let seconds = secs % 60;
    if seconds == 0 {
        return format!("{minutes}m");
    }
    format!("{minutes}m {seconds}s")
}

impl AppModel {
    fn handle_provider_refreshed(
        &mut self,
        refresh_result: ProviderRefreshResult,
    ) -> Task<Message> {
        let ProviderRefreshResult { provider, accounts } = refresh_result;
        let refreshed_provider = provider.provider;
        let refreshed_selected_ids = provider.selected_account_ids.clone();
        self.state.upsert_provider(provider);
        for account in accounts {
            self.state.upsert_account(account);
        }
        if refreshed_provider == ProviderId::Codex {
            self.update_codex_metadata_from_state();
            self.clear_codex_legacy_snapshot_after_success();
        }
        if refreshed_provider == ProviderId::Claude {
            self.update_claude_metadata_from_state();
            self.clear_claude_legacy_snapshot_after_success();
        }
        if refreshed_provider == ProviderId::Cursor {
            self.update_cursor_metadata_from_state();
        }
        if self.config.selected_account_ids(refreshed_provider) != refreshed_selected_ids.as_slice()
        {
            self.write_config(|new_config| {
                new_config
                    .selected_account_ids_mut(refreshed_provider)
                    .clone_from(&refreshed_selected_ids);
            });
        }
        runtime::persist_state(&self.state);
        self.selected_provider = select_provider(self.selected_provider, &self.state);
        self.sync_panel_suggested_bounds();
        if refreshed_provider == ProviderId::Cursor {
            return refresh_provider_account_statuses_task(
                &self.config,
                &self.state,
                ProviderId::Cursor,
            );
        }
        Task::none()
    }

    fn handle_update_checked(&mut self, status: UpdateStatus, attempt: u32) -> Task<Message> {
        if let UpdateStatus::Error(reason) = status {
            let next_attempt = attempt.saturating_add(1);
            let delay = update_retry_delay(next_attempt);
            self.update_status = UpdateStatus::Error(format!(
                "{reason}; retrying in {}",
                format_retry_delay(delay)
            ));
            return update_retry_task(next_attempt, delay);
        }
        self.update_status = status;
        Task::none()
    }

    fn navigate_to(&mut self, route: PopupRoute) -> Option<Task<Message>> {
        let resize = self.resize_popup_to_route(&route);
        self.popup_route = route;
        resize
    }

    fn popup_size_for_route(&self, route: &PopupRoute) -> Size {
        match route {
            PopupRoute::ProviderDetail => {
                popup_view::popup_session_size(&self.state, self.selected_provider)
            }
            PopupRoute::Settings(_) => popup_view::popup_settings_size(&self.state),
        }
    }

    fn sync_panel_suggested_bounds(&mut self) {
        let n_accounts = self
            .state
            .selected_accounts(self.selected_provider)
            .len()
            .max(1);
        let (w, h) = applet_button_size(&self.core, self.config.panel_icon_style, n_accounts);
        self.core.applet.suggested_bounds = Some(Size::new(w, h));
    }

    fn select_provider_tab(&mut self, provider: ProviderId) -> Option<Task<Message>> {
        self.selected_provider = provider;
        self.sync_panel_suggested_bounds();
        self.resize_popup_to_provider(provider)
    }

    fn resize_popup_to_provider(&mut self, provider: ProviderId) -> Option<Task<Message>> {
        let new_size = popup_view::popup_session_size(&self.state, provider);
        self.resize_popup_to_size(new_size)
    }

    fn resize_popup_to_route(&mut self, route: &PopupRoute) -> Option<Task<Message>> {
        let new_size = self.popup_size_for_route(route);
        self.resize_popup_to_size(new_size)
    }

    fn resize_popup_to_size(&mut self, new_size: Size) -> Option<Task<Message>> {
        let popup_id = self.popup?;
        self.popup_size = Some(new_size);
        let (w, h) = popup_size_tuple(new_size);
        Some(resize_popup(popup_id, w, h))
    }

    fn toggle_popup(&mut self) -> Task<Message> {
        if let Some(p) = self.popup.take() {
            self.popup_size = None;
            return cosmic::task::message(cosmic::Action::Cosmic(cosmic::app::Action::Surface(
                destroy_popup(p),
            )));
        }

        let popup_size = self.popup_size_for_route(&self.popup_route.clone());
        let max_width = popup_view::popup_max_width(&self.state);
        self.popup_size = Some(popup_size);
        cosmic::task::message(cosmic::Action::Cosmic(cosmic::app::Action::Surface(
            app_popup::<Self>(
                move |state| {
                    let new_id = Id::unique();
                    state.popup.replace(new_id);
                    let mut popup_settings = state.core.applet.get_popup_settings(
                        state.core.main_window_id().unwrap(),
                        new_id,
                        Some(popup_size_tuple(popup_size)),
                        None,
                        None,
                    );
                    popup_settings.positioner.size_limits =
                        popup_size_limits_with_max_width(popup_size, max_width);
                    popup_settings.positioner.reactive = false;
                    popup_settings
                },
                None,
            ),
        )))
    }

    fn write_config(&mut self, f: impl FnOnce(&mut Config)) {
        if let Ok(ctx) =
            cosmic_config::Config::new(<Self as cosmic::Application>::APP_ID, Config::VERSION)
        {
            let mut new_config = self.config.clone();
            f(&mut new_config);
            let _ = new_config.write_entry(&ctx);
            self.config = new_config;
        }
    }

    fn set_provider_enabled(&mut self, provider: ProviderId, enabled: bool) -> Task<Message> {
        if let Some(entry) = self.state.provider_mut(provider) {
            entry.enabled = enabled;
        }
        self.selected_provider = select_provider(self.selected_provider, &self.state);
        self.write_config(|new_config| match provider {
            ProviderId::Codex => new_config.codex_enabled = enabled,
            ProviderId::Claude => new_config.claude_enabled = enabled,
            ProviderId::Cursor => new_config.cursor_enabled = enabled,
        });
        if enabled {
            runtime::reconcile_provider(&self.config, &mut self.state, provider);
            return refresh_provider_tasks(&self.config, &mut self.state);
        }
        Task::none()
    }

    fn set_refresh_interval(&mut self, interval_seconds: u64) -> Task<Message> {
        self.write_config(|new_config| {
            new_config.refresh_interval_seconds = interval_seconds;
        });
        Task::none()
    }

    fn set_reset_time_format(&mut self, format: ResetTimeFormat) -> Task<Message> {
        self.write_config(|new_config| {
            new_config.reset_time_format = format;
        });
        Task::none()
    }

    fn set_usage_amount_format(&mut self, format: UsageAmountFormat) -> Task<Message> {
        self.write_config(|new_config| {
            new_config.usage_amount_format = format;
        });
        Task::none()
    }

    fn set_panel_icon_style(&mut self, style: PanelIconStyle) -> Task<Message> {
        self.write_config(|new_config| {
            new_config.panel_icon_style = style;
        });
        self.sync_panel_suggested_bounds();
        Task::none()
    }

    fn on_config_update(&mut self, config: Config) {
        self.config = config;
        runtime::reconcile_state(&self.config, &mut self.state);
        demo_env::apply(&self.config, &mut self.state);
        runtime::persist_state(&self.state);
        self.sync_panel_suggested_bounds();
    }

    fn toggle_account_selection(
        &mut self,
        provider: ProviderId,
        account_id: &str,
    ) -> Task<Message> {
        self.write_config(|new_config| {
            registry::toggle_account_selection(provider, new_config, account_id);
        });
        runtime::reconcile_provider(&self.config, &mut self.state, provider);
        let is_selected = self
            .state
            .provider(provider)
            .is_some_and(|p| p.selected_account_ids.contains(&account_id.to_string()));
        if is_selected
            && let Some(account) = self
                .state
                .provider_accounts
                .iter_mut()
                .find(|entry| entry.provider == provider && entry.account_id == account_id)
        {
            account.error = None;
        }
        self.sync_panel_suggested_bounds();
        refresh_provider_task(&self.config, &mut self.state, provider)
    }

    fn delete_codex_account(&mut self, account_id: &str) -> Task<Message> {
        let provider = ProviderId::Codex;
        let managed_account = self
            .config
            .codex_managed_accounts
            .iter()
            .find(|account| account.id == account_id)
            .cloned();

        let Some(account) = managed_account else {
            return Task::none();
        };

        remove_managed_codex_home(&account.codex_home);
        self.write_config(|new_config| {
            let _ = registry::delete_account(provider, account_id, new_config);
            registry::sync_selected_ids_with_discoveries(new_config, provider);
        });
        runtime::reconcile_provider(&self.config, &mut self.state, provider);
        runtime::persist_state(&self.state);

        if self
            .state
            .provider(ProviderId::Codex)
            .is_some_and(|provider| provider.account_status == AccountSelectionStatus::Ready)
        {
            return refresh_provider_tasks(&self.config, &mut self.state);
        }
        Task::none()
    }

    fn delete_claude_account(&mut self, account_id: &str) -> Task<Message> {
        let provider = ProviderId::Claude;
        let managed_account = self
            .config
            .claude_managed_accounts
            .iter()
            .find(|account| account.id == account_id)
            .cloned();

        let Some(account) = managed_account else {
            return Task::none();
        };

        claude::remove_managed_config_dir(&account.config_dir);
        self.write_config(|new_config| {
            let _ = registry::delete_account(provider, account_id, new_config);
            registry::sync_selected_ids_with_discoveries(new_config, provider);
        });
        runtime::reconcile_provider(&self.config, &mut self.state, provider);
        runtime::persist_state(&self.state);

        if self
            .state
            .provider(ProviderId::Claude)
            .is_some_and(|provider| provider.account_status == AccountSelectionStatus::Ready)
        {
            return refresh_provider_task(&self.config, &mut self.state, ProviderId::Claude);
        }
        Task::none()
    }

    fn delete_cursor_account(&mut self, account_id: &str) -> Task<Message> {
        let provider = ProviderId::Cursor;
        let Some(account) =
            cursor::find_managed_account(&self.config.cursor_managed_accounts, account_id).cloned()
        else {
            return Task::none();
        };

        cursor::remove_managed_profile(&account.account_root);
        self.write_config(|new_config| {
            let _ = registry::delete_account(provider, account_id, new_config);
            registry::sync_selected_ids_with_discoveries(new_config, provider);
        });
        runtime::reconcile_provider(&self.config, &mut self.state, provider);
        runtime::persist_state(&self.state);

        if self
            .state
            .provider(ProviderId::Cursor)
            .is_some_and(|provider| provider.account_status == AccountSelectionStatus::Ready)
        {
            return refresh_provider_task(&self.config, &mut self.state, ProviderId::Cursor);
        }
        Task::none()
    }

    fn start_codex_login(&mut self) -> Task<Message> {
        if self
            .codex_login
            .as_ref()
            .is_some_and(|login| login.status == CodexLoginStatus::Running)
        {
            return Task::none();
        }
        self.codex_login = None;
        let (state, task) = match codex::prepare(self.config.clone()) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.codex_login = Some(CodexLoginState {
                    flow_id: "failed".to_string(),
                    status: CodexLoginStatus::Failed,
                    login_url: None,
                    output: Vec::new(),
                    error: Some(error),
                });
                return Task::none();
            }
        };
        self.start_codex_login_task(state, task)
    }

    fn start_codex_login_task(
        &mut self,
        state: CodexLoginState,
        task: cosmic::iced::Task<CodexLoginEvent>,
    ) -> Task<Message> {
        self.codex_login = Some(state);
        let task = task.map(|event| cosmic::Action::App(Message::CodexLoginEvent(Box::new(event))));
        let (task, handle) = task.abortable();
        self.codex_login_handle = Some(handle);
        task
    }

    fn cancel_codex_login(&mut self) {
        if let Some(handle) = self.codex_login_handle.take() {
            handle.abort();
        }
        self.codex_login = None;
    }

    fn handle_codex_login_event(&mut self, event: CodexLoginEvent) -> Task<Message> {
        match event {
            CodexLoginEvent::Output {
                flow_id,
                line,
                login_url,
            } => {
                let Some(login) = self.codex_login.as_mut() else {
                    return Task::none();
                };
                if login.flow_id != flow_id {
                    return Task::none();
                }
                if let Some(url) = login_url {
                    login.login_url = Some(url);
                }
                login.output.push(line);
                if login.output.len() > 8 {
                    login.output.remove(0);
                }
                Task::none()
            }
            CodexLoginEvent::Finished { flow_id, result } => {
                let Some(login) = self.codex_login.as_mut() else {
                    return Task::none();
                };
                if login.flow_id != flow_id {
                    return Task::none();
                }
                self.codex_login_handle = None;
                match *result {
                    Ok(success) => {
                        login.status = CodexLoginStatus::Succeeded;
                        login.error = None;
                        let account_id = success.account.id.clone();
                        let account_label = success.account.label.clone();
                        self.write_config(|new_config| {
                            codex::apply_login_account(new_config, success.account.clone());
                        });
                        runtime::reconcile_provider(
                            &self.config,
                            &mut self.state,
                            ProviderId::Codex,
                        );
                        let mut account = ProviderAccountRuntimeState::empty(
                            ProviderId::Codex,
                            account_id.clone(),
                            account_label,
                        );
                        if let Some(snapshot) = success.snapshot {
                            account.source_label = Some(snapshot.source.clone());
                            account.last_success_at = Some(chrono::Utc::now());
                            account.health = crate::model::ProviderHealth::Ok;
                            account.snapshot = Some(snapshot);
                        }
                        account.auth_state = crate::model::AuthState::Ready;
                        account.error = None;
                        let refresh_succeeded =
                            account.health == ProviderHealth::Ok && account.snapshot.is_some();
                        self.state.upsert_account(account);
                        if let Some(provider) = self.state.provider_mut(ProviderId::Codex) {
                            if !provider.selected_account_ids.contains(&account_id) {
                                provider.selected_account_ids.push(account_id);
                            }
                            provider.account_status = AccountSelectionStatus::Ready;
                            provider.error = None;
                            if refresh_succeeded {
                                provider.legacy_display_snapshot = None;
                            }
                        }
                        runtime::persist_state(&self.state);
                        refresh_provider_tasks(&self.config, &mut self.state)
                    }
                    Err(error) => {
                        login.status = CodexLoginStatus::Failed;
                        login.error = Some(error);
                        Task::none()
                    }
                }
            }
        }
    }

    fn update_codex_metadata_from_state(&mut self) {
        let updates = self
            .state
            .accounts_for(ProviderId::Codex)
            .into_iter()
            .filter_map(codex_managed_metadata_update)
            .collect::<Vec<_>>();
        if updates.is_empty() {
            return;
        }

        self.write_config(|new_config| {
            for update in &updates {
                if let Some(account) = new_config
                    .codex_managed_accounts
                    .iter_mut()
                    .find(|account| account.id == update.id)
                {
                    apply_codex_metadata_update(account, update);
                }
            }
        });
        runtime::reconcile_provider(&self.config, &mut self.state, ProviderId::Codex);
    }

    fn clear_codex_legacy_snapshot_after_success(&mut self) {
        let active_ok = self
            .state
            .active_account(ProviderId::Codex)
            .is_some_and(|account| {
                account.health == ProviderHealth::Ok && account.snapshot.is_some()
            });
        if !active_ok {
            return;
        }
        if let Some(provider) = self.state.provider_mut(ProviderId::Codex) {
            provider.legacy_display_snapshot = None;
        }
    }

    fn update_claude_metadata_from_state(&mut self) {
        let updates = self
            .state
            .accounts_for(ProviderId::Claude)
            .into_iter()
            .filter_map(claude_managed_metadata_update)
            .collect::<Vec<_>>();
        if updates.is_empty() {
            return;
        }

        self.write_config(|new_config| {
            for update in &updates {
                if let Some(account) = new_config
                    .claude_managed_accounts
                    .iter_mut()
                    .find(|account| account.id == update.id)
                {
                    apply_claude_metadata_update(account, update);
                }
            }
        });
        runtime::reconcile_provider(&self.config, &mut self.state, ProviderId::Claude);
    }

    fn clear_claude_legacy_snapshot_after_success(&mut self) {
        let active_ok = self
            .state
            .active_account(ProviderId::Claude)
            .is_some_and(|account| {
                account.health == ProviderHealth::Ok && account.snapshot.is_some()
            });
        if !active_ok {
            return;
        }
        if let Some(provider) = self.state.provider_mut(ProviderId::Claude) {
            provider.legacy_display_snapshot = None;
        }
    }

    fn update_cursor_metadata_from_state(&mut self) {
        let updates = self
            .state
            .accounts_for(ProviderId::Cursor)
            .into_iter()
            .filter_map(cursor_managed_metadata_update)
            .collect::<Vec<_>>();
        if updates.is_empty() {
            return;
        }
        self.write_config(|new_config| {
            for update in &updates {
                if let Some(account) = new_config.cursor_managed_accounts.iter_mut().find(|a| {
                    (!a.id.is_empty() && a.id == update.config_id) || a.email == update.config_id
                }) {
                    apply_cursor_metadata_update(account, update);
                }
            }
        });
        runtime::reconcile_provider(&self.config, &mut self.state, ProviderId::Cursor);
    }

    fn start_claude_login(&mut self) -> Task<Message> {
        if self
            .claude_login
            .as_ref()
            .is_some_and(|login| login.status == ClaudeLoginStatus::Running)
        {
            return Task::none();
        }
        self.claude_login = None;
        let (state, task) = match claude::prepare(self.config.clone()) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.claude_login = Some(ClaudeLoginState {
                    flow_id: "failed".to_string(),
                    status: ClaudeLoginStatus::Failed,
                    login_url: None,
                    output: Vec::new(),
                    error: Some(error),
                });
                return Task::none();
            }
        };
        self.claude_login = Some(state);
        let task =
            task.map(|event| cosmic::Action::App(Message::ClaudeLoginEvent(Box::new(event))));
        let (task, handle) = task.abortable();
        self.claude_login_handle = Some(handle);
        task
    }

    fn cancel_claude_login(&mut self) {
        if let Some(handle) = self.claude_login_handle.take() {
            handle.abort();
        }
        self.claude_login = None;
    }

    fn handle_claude_login_event(&mut self, event: ClaudeLoginEvent) -> Task<Message> {
        match event {
            ClaudeLoginEvent::Output {
                flow_id,
                line,
                login_url,
            } => {
                let Some(login) = self.claude_login.as_mut() else {
                    return Task::none();
                };
                if login.flow_id != flow_id {
                    return Task::none();
                }
                if let Some(url) = login_url {
                    login.login_url = Some(url);
                }
                login.output.push(line);
                if login.output.len() > 8 {
                    login.output.remove(0);
                }
                Task::none()
            }
            ClaudeLoginEvent::Finished { flow_id, result } => {
                let Some(login) = self.claude_login.as_mut() else {
                    return Task::none();
                };
                if login.flow_id != flow_id {
                    return Task::none();
                }
                self.claude_login_handle = None;
                match *result {
                    Ok(success) => {
                        login.status = ClaudeLoginStatus::Succeeded;
                        login.error = None;
                        let account_id = success.account.id.clone();
                        let account_label = success.account.label.clone();
                        self.write_config(|new_config| {
                            claude::apply_login_account(new_config, success.account.clone());
                        });
                        runtime::reconcile_provider(
                            &self.config,
                            &mut self.state,
                            ProviderId::Claude,
                        );
                        let mut account = ProviderAccountRuntimeState::empty(
                            ProviderId::Claude,
                            account_id.clone(),
                            account_label,
                        );
                        if let Some(snapshot) = success.snapshot {
                            account.source_label = Some(snapshot.source.clone());
                            account.last_success_at = Some(chrono::Utc::now());
                            account.health = crate::model::ProviderHealth::Ok;
                            account.snapshot = Some(snapshot);
                        }
                        account.auth_state = crate::model::AuthState::Ready;
                        account.error = None;
                        let refresh_succeeded =
                            account.health == ProviderHealth::Ok && account.snapshot.is_some();
                        self.state.upsert_account(account);
                        if let Some(provider) = self.state.provider_mut(ProviderId::Claude) {
                            if !provider.selected_account_ids.contains(&account_id) {
                                provider.selected_account_ids.push(account_id);
                            }
                            provider.account_status = AccountSelectionStatus::Ready;
                            provider.error = None;
                            if refresh_succeeded {
                                provider.legacy_display_snapshot = None;
                            }
                        }
                        runtime::persist_state(&self.state);
                        refresh_provider_task(&self.config, &mut self.state, ProviderId::Claude)
                    }
                    Err(error) => {
                        login.status = ClaudeLoginStatus::Failed;
                        login.error = Some(error);
                        Task::none()
                    }
                }
            }
        }
    }

    fn start_cursor_login(&mut self) -> Task<Message> {
        if self
            .cursor_login
            .as_ref()
            .is_some_and(|login| login.status == CursorLoginStatus::Running)
        {
            return Task::none();
        }
        cursor::cleanup_pending_dirs();
        self.cursor_login = None;
        let (state, task) = match cursor::prepare(self.config.cursor_browser) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.cursor_login = Some(CursorLoginState {
                    flow_id: "failed".to_string(),
                    status: CursorLoginStatus::Failed,
                    browser: self.config.cursor_browser,
                    login_url: cursor::LOGIN_URL.to_string(),
                    error: Some(error),
                });
                return Task::none();
            }
        };
        self.cursor_login = Some(state);
        let task =
            task.map(|event| cosmic::Action::App(Message::CursorLoginEvent(Box::new(event))));
        let (task, handle) = task.abortable();
        self.cursor_login_handle = Some(handle);
        task
    }

    fn reauthenticate_cursor_account(&mut self, account_id: &str) -> Task<Message> {
        if cursor::find_managed_account(&self.config.cursor_managed_accounts, account_id).is_none()
        {
            return Task::none();
        }
        self.start_cursor_login()
    }

    fn cancel_cursor_login(&mut self) {
        if let Some(handle) = self.cursor_login_handle.take() {
            handle.abort();
        }
        cursor::cleanup_pending_dirs();
        self.cursor_login = None;
    }

    fn handle_cursor_login_event(&mut self, event: CursorLoginEvent) -> Task<Message> {
        match event {
            CursorLoginEvent::Finished { flow_id, result } => {
                let Some(login) = self.cursor_login.as_mut() else {
                    return Task::none();
                };
                if login.flow_id != flow_id {
                    return Task::none();
                }
                self.cursor_login_handle = None;
                cursor::cleanup_pending_dirs();
                match *result {
                    Ok(success) => {
                        login.status = CursorLoginStatus::Succeeded;
                        login.error = None;
                        let mut applied_account = success.account.clone();
                        self.write_config(|new_config| {
                            applied_account =
                                cursor::upsert_managed_account(new_config, success.account.clone());
                        });
                        runtime::reconcile_provider(
                            &self.config,
                            &mut self.state,
                            ProviderId::Cursor,
                        );
                        let account_id = cursor::managed_account_id(&applied_account.id);
                        let account_label = applied_account.email.clone();
                        let mut account = ProviderAccountRuntimeState::empty(
                            ProviderId::Cursor,
                            account_id.clone(),
                            account_label,
                        );
                        if let Some(snapshot) = success.snapshot {
                            account.source_label = Some(snapshot.source.clone());
                            account.last_success_at = Some(chrono::Utc::now());
                            account.health = crate::model::ProviderHealth::Ok;
                            account.snapshot = Some(snapshot);
                        }
                        account.auth_state = crate::model::AuthState::Ready;
                        account.error = None;
                        self.state.upsert_account(account);
                        if let Some(provider) = self.state.provider_mut(ProviderId::Cursor) {
                            if !provider.selected_account_ids.contains(&account_id) {
                                provider.selected_account_ids.push(account_id);
                            }
                            provider.account_status = AccountSelectionStatus::Ready;
                            provider.error = None;
                        }
                        runtime::persist_state(&self.state);
                        refresh_provider_task(&self.config, &mut self.state, ProviderId::Cursor)
                    }
                    Err(error) => {
                        login.status = CursorLoginStatus::Failed;
                        login.error = Some(error);
                        Task::none()
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
struct CodexMetadataUpdate {
    id: String,
    label: Option<String>,
    email: Option<String>,
    provider_account_id: Option<String>,
}

fn codex_managed_metadata_update(
    account: &ProviderAccountRuntimeState,
) -> Option<CodexMetadataUpdate> {
    let snapshot = account.snapshot.as_ref()?;
    Some(CodexMetadataUpdate {
        id: account.account_id.clone(),
        label: snapshot.identity.email.clone(),
        email: snapshot.identity.email.clone(),
        provider_account_id: snapshot.identity.account_id.clone(),
    })
}

fn apply_codex_metadata_update(
    account: &mut ManagedCodexAccountConfig,
    update: &CodexMetadataUpdate,
) {
    if let Some(label) = &update.label
        && account.label == "Codex account"
    {
        account.label.clone_from(label);
    }
    if update.email.is_some() {
        account.email.clone_from(&update.email);
    }
    if update.provider_account_id.is_some() {
        account
            .provider_account_id
            .clone_from(&update.provider_account_id);
    }
    account.updated_at = chrono::Utc::now();
}

#[derive(Clone)]
struct ClaudeMetadataUpdate {
    id: String,
    label: Option<String>,
    email: Option<String>,
    subscription_type: Option<String>,
}

fn claude_managed_metadata_update(
    account: &ProviderAccountRuntimeState,
) -> Option<ClaudeMetadataUpdate> {
    let snapshot = account.snapshot.as_ref()?;
    Some(ClaudeMetadataUpdate {
        id: account.account_id.clone(),
        label: snapshot.identity.email.clone(),
        email: snapshot.identity.email.clone(),
        subscription_type: snapshot.identity.plan.clone(),
    })
}

fn apply_claude_metadata_update(
    account: &mut ManagedClaudeAccountConfig,
    update: &ClaudeMetadataUpdate,
) {
    if let Some(label) = &update.label {
        account.label.clone_from(label);
    }
    if update.email.is_some() {
        account.email.clone_from(&update.email);
    }
    if update.subscription_type.is_some() {
        account
            .subscription_type
            .clone_from(&update.subscription_type);
    }
    account.updated_at = chrono::Utc::now();
}

#[derive(Clone)]
struct CursorMetadataUpdate {
    config_id: String,
    email: String,
    display_name: Option<String>,
    plan: Option<String>,
}

fn cursor_managed_metadata_update(
    account: &ProviderAccountRuntimeState,
) -> Option<CursorMetadataUpdate> {
    let config_id = cursor::managed_config_id(&account.account_id)?;
    let snapshot = account.snapshot.as_ref()?;
    Some(CursorMetadataUpdate {
        config_id: config_id.to_string(),
        email: snapshot
            .identity
            .email
            .as_deref()
            .map_or_else(|| config_id.to_string(), cursor::normalized_email),
        display_name: snapshot.identity.display_name.clone(),
        plan: snapshot.identity.plan.clone(),
    })
}

fn apply_cursor_metadata_update(
    account: &mut ManagedCursorAccountConfig,
    update: &CursorMetadataUpdate,
) {
    account.label.clone_from(&update.email);
    account.email.clone_from(&update.email);
    if update.display_name.is_some() {
        account.display_name.clone_from(&update.display_name);
    }
    if update.plan.is_some() {
        account.plan.clone_from(&update.plan);
    }
    account.updated_at = chrono::Utc::now();
}

fn resize_popup(id: Id, width: u32, height: u32) -> Task<Message> {
    cosmic::iced::platform_specific::shell::wayland::commands::popup::set_size(id, width, height)
}

fn popup_size_limits_with_max_width(size: Size, max_width: f32) -> Limits {
    Limits::NONE
        .min_width(1.0)
        .max_width(max_width.max(size.width))
        .height(size.height.clamp(1.0, f32::from(POPUP_MAX_HEIGHT)))
}

fn popup_size_tuple(size: Size) -> (u32, u32) {
    (
        rounded_dimension_to_u32(size.width),
        rounded_dimension_to_u32(size.height),
    )
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn rounded_dimension_to_u32(value: f32) -> u32 {
    const MAX_U32_F32: f32 = 4_294_967_295.0;

    if !value.is_finite() {
        return 0;
    }

    let rounded = value.round().clamp(0.0, MAX_U32_F32);
    rounded as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        ProviderAccountRuntimeState, ProviderIdentity, UsageHeadline, UsageSnapshot, UsageWindow,
    };
    use chrono::Utc;

    #[test]
    fn popup_limits_allow_wider_max() {
        let limits = popup_size_limits_with_max_width(Size::new(420.0, 640.0), 840.0);

        assert_eq!(limits.min().width, 1.0);
        assert_eq!(limits.max().width, 840.0);
        assert_eq!(limits.min().height, 640.0);
        assert_eq!(limits.max().height, 640.0);
    }

    #[test]
    fn popup_size_tuple_rounds_logical_size() {
        assert_eq!(popup_size_tuple(Size::new(419.6, 640.2)), (420, 640));
    }

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
        let (major_padding, minor_padding) = core.applet.suggested_padding(true);
        let horizontal_padding = if core.applet.is_horizontal() {
            major_padding
        } else {
            minor_padding
        };
        let compact_px = suggested_w.min(suggested_h);
        let logo_width = f32::from(compact_px.saturating_sub(8).max(11));
        let bar_width = applet_bar_width(suggested_w, suggested_h);
        let padding_width = f32::from(2 * horizontal_padding);
        let (logo_bars_width, height) = applet_button_size(&core, PanelIconStyle::LogoAndBars, 1);
        let (bars_only_width, bars_only_height) =
            applet_button_size(&core, PanelIconStyle::BarsOnly, 1);
        let (percent_width, percent_height) =
            applet_button_size(&core, PanelIconStyle::LogoAndPercent, 1);
        let (percent_only_width, percent_only_height) =
            applet_button_size(&core, PanelIconStyle::PercentOnly, 1);

        assert_eq!(bars_only_width, bar_width + padding_width);
        assert_eq!(
            percent_only_width,
            APPLET_PERCENT_TEXT_WIDTH + padding_width
        );
        assert_eq!(
            logo_bars_width,
            logo_width + APPLET_ICON_GAP + bar_width + padding_width
        );
        assert_eq!(
            percent_width,
            logo_width + APPLET_ICON_GAP + APPLET_PERCENT_TEXT_WIDTH + padding_width
        );
        assert_eq!(height, bars_only_height);
        assert_eq!(height, percent_height);
        assert_eq!(height, percent_only_height);
    }

    #[test]
    fn applet_button_size_scales_with_account_count() {
        let core = cosmic::Core::default();
        let (w1, _) = applet_button_size(&core, PanelIconStyle::BarsOnly, 1);
        let (w2, _) = applet_button_size(&core, PanelIconStyle::BarsOnly, 2);
        let (w3, _) = applet_button_size(&core, PanelIconStyle::BarsOnly, 3);
        let (suggested_w, suggested_h) = core.applet.suggested_size(false);
        let bar_width = applet_bar_width(suggested_w, suggested_h);
        assert_eq!(w2 - w1, bar_width + APPLET_ACCOUNT_GAP);
        assert_eq!(w3 - w2, bar_width + APPLET_ACCOUNT_GAP);
        let (lw2, _) = applet_button_size(&core, PanelIconStyle::LogoAndBars, 2);
        let (lw1, _) = applet_button_size(&core, PanelIconStyle::LogoAndBars, 1);
        assert_eq!(lw2 - lw1, bar_width + APPLET_ACCOUNT_GAP);
    }

    #[test]
    fn applet_percent_text_uses_one_decimal_digit() {
        assert_eq!(applet_percent_text(86.54), "86.5%");
        assert_eq!(applet_percent_text(100.0), "100.0%");
    }

    #[test]
    fn selected_provider_percent_uses_first_panel_window() {
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
                },
                UsageWindow {
                    label: "Weekly".to_string(),
                    used_percent: 42.0,
                    reset_at: None,
                    window_seconds: None,
                    reset_description: None,
                },
            ],
            provider_cost: None,
            identity: ProviderIdentity::default(),
        });

        state
            .provider_mut(ProviderId::Codex)
            .unwrap()
            .selected_account_ids = vec!["codex-1".to_string()];
        state.upsert_account(account);

        assert_eq!(
            selected_provider_percent(&state, ProviderId::Codex, UsageAmountFormat::Used),
            86.5
        );
        assert_eq!(
            selected_provider_percent(&state, ProviderId::Codex, UsageAmountFormat::Left),
            13.5
        );
    }
}
