// SPDX-License-Identifier: MPL-2.0

use crate::app_refresh::refresh_provider_tasks;
use crate::config::Config;
use crate::model::{AppState, ProviderId, ProviderRuntimeState};
use crate::popup_view;
use crate::provider_assets::{provider_icon_handle, provider_icon_variant};
use crate::runtime;
use crate::updates::UpdateStatus;
use crate::usage_display;
use cosmic::app::Task;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::time;
use cosmic::iced::widget::{column, progress_bar, row};
use cosmic::iced::window::Id;
use cosmic::iced::{Alignment, Length, Limits, Size, Subscription};
use cosmic::prelude::*;
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::theme::Button as CosmicButton;
use cosmic::widget;
use std::time::Duration;

const REFRESH_INTERVAL_MIN_SECS: u64 = 10;
const POPUP_WIDTH: u16 = 420;
const POPUP_MAX_HEIGHT: u16 = 1080;
const APPLET_BAR_WIDTH_HEIGHT_MULTIPLIER: u16 = 2;

pub struct AppModel {
    core: cosmic::Core,
    popup: Option<Id>,
    config: Config,
    state: AppState,
    selected_provider: ProviderId,
    show_settings: bool,
    update_status: UpdateStatus,
    launch_mode: LaunchMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    Panel,
    Standalone,
}

#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    UpdateConfig(Config),
    Tick,
    RefreshNow,
    Refreshed(Box<AppState>),
    ProviderRefreshed(Box<ProviderRuntimeState>),
    SelectProvider(ProviderId),
    ToggleSettings,
    SetProviderEnabled(ProviderId, bool),
    SetRefreshInterval(u64),
    UpdateChecked(UpdateStatus),
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

        let config = cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
            .map(|ctx| match Config::get_entry(&ctx) {
                Ok(cfg) => cfg.with_env_overrides(),
                Err((_errors, cfg)) => cfg.with_env_overrides(),
            })
            .unwrap_or_default();

        let initial_config = config.clone();
        let (applet_width, applet_height) = applet_button_size(&core);
        core.applet.suggested_bounds = Some(Size::new(applet_width, applet_height));
        let app = AppModel {
            core,
            popup: None,
            config,
            state: AppState::empty(),
            selected_provider: ProviderId::Codex,
            show_settings: false,
            update_status: UpdateStatus::Unchecked,
            launch_mode,
        };

        let load_task = Task::perform(
            async move { runtime::load_initial_state(&initial_config) },
            |state| cosmic::Action::App(Message::Refreshed(Box::new(state))),
        );
        let update_task = Task::perform(
            async { crate::updates::check(&runtime::http_client()).await },
            |status| cosmic::Action::App(Message::UpdateChecked(status)),
        );

        (app, Task::batch([load_task, update_task]))
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let indicator = applet_indicator(&self.state, self.selected_provider, &self.core);
        let button: Element<'_, Message> = applet_button(&self.core, indicator)
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
            self.selected_provider,
            self.show_settings,
            &self.update_status,
        );
        self.core
            .applet
            .popup_container(content)
            .max_height(f32::from(POPUP_MAX_HEIGHT))
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
                .map(|update| Message::UpdateConfig(update.config.with_env_overrides())),
            time::every(Duration::from_secs(interval_secs)).map(|_| Message::Tick),
        ])
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::UpdateConfig(config) => {
                self.config = config;
            }

            Message::TogglePopup => {
                return if let Some(p) = self.popup.take() {
                    cosmic::task::message(cosmic::Action::Cosmic(cosmic::app::Action::Surface(
                        destroy_popup(p),
                    )))
                } else {
                    cosmic::task::message(cosmic::Action::Cosmic(cosmic::app::Action::Surface(
                        app_popup::<Self>(
                            |state| {
                                let new_id = Id::unique();
                                state.popup.replace(new_id);
                                let mut popup_settings = state.core.applet.get_popup_settings(
                                    state.core.main_window_id().unwrap(),
                                    new_id,
                                    None,
                                    None,
                                    None,
                                );
                                popup_settings.positioner.size_limits = Limits::NONE
                                    .max_width(f32::from(POPUP_WIDTH))
                                    .min_width(300.0)
                                    .max_height(f32::from(POPUP_MAX_HEIGHT));
                                popup_settings
                            },
                            None,
                        ),
                    )))
                };
            }

            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                }
            }

            Message::Tick | Message::RefreshNow => {
                return refresh_provider_tasks(&self.config, &mut self.state);
            }

            Message::Refreshed(loaded_state) => {
                return apply_refreshed_state(
                    &self.config,
                    &mut self.state,
                    &mut self.selected_provider,
                    *loaded_state,
                );
            }

            Message::ProviderRefreshed(provider_state) => {
                self.state.upsert_provider(*provider_state);
                runtime::persist_state(&self.state);
                self.selected_provider = select_provider(self.selected_provider, &self.state);
            }

            Message::SelectProvider(provider) => {
                self.selected_provider = provider;
            }

            Message::ToggleSettings => {
                self.show_settings = !self.show_settings;
            }

            Message::UpdateChecked(status) => {
                self.update_status = status;
            }

            Message::OpenUrl(url) => {
                if let Err(e) = std::process::Command::new("xdg-open").arg(&url).spawn() {
                    tracing::warn!(url = %url, error = %e, "failed to open url");
                }
            }

            Message::Quit => {
                std::process::exit(0);
            }

            Message::SetProviderEnabled(provider, enabled) => {
                return self.set_provider_enabled(provider, enabled);
            }

            Message::SetRefreshInterval(interval_seconds) => {
                return self.set_refresh_interval(interval_seconds);
            }
        }
        Task::none()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

pub fn applet_settings() -> cosmic::app::Settings {
    let preview_core = cosmic::Core::default();
    let (width, height) = applet_button_size(&preview_core);

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
    core: &cosmic::Core,
) -> Element<'a, Message> {
    let (suggested_w, suggested_h) = core.applet.suggested_size(false);
    let compact_px = suggested_w.min(suggested_h);
    let logo_size_px = compact_px.saturating_sub(8).max(11);
    let logo_size = f32::from(logo_size_px);
    let bar_width = applet_bar_width(suggested_w, suggested_h);
    let (top_usage, bottom_usage) = selected_provider_percents(state, selected_provider);

    let bars = column![
        progress_bar(0.0..=100.0, top_usage)
            .length(Length::Fixed(bar_width))
            .girth(Length::Fixed(6.0)),
        progress_bar(0.0..=100.0, bottom_usage)
            .length(Length::Fixed(bar_width))
            .girth(Length::Fixed(3.0)),
    ]
    .spacing(3)
    .width(Length::Fixed(bar_width));

    row![
        widget::icon::icon(provider_icon_handle(
            selected_provider,
            provider_icon_variant(),
        ))
        .size(logo_size_px)
        .width(Length::Fixed(logo_size))
        .height(Length::Fixed(logo_size)),
        bars,
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

fn applet_button<'a>(
    core: &cosmic::Core,
    content: impl Into<Element<'a, Message>>,
) -> widget::Button<'a, Message> {
    let (major_padding, minor_padding) = core.applet.suggested_padding(true);
    let horizontal_padding = if core.applet.is_horizontal() {
        major_padding
    } else {
        minor_padding
    };
    let (width, height) = applet_button_size(core);

    widget::button::custom(
        widget::layer_container(content)
            .padding(cosmic::iced::Padding::from([0, horizontal_padding]))
            .align_y(cosmic::iced::alignment::Vertical::Center.into()),
    )
    .width(Length::Fixed(width))
    .height(Length::Fixed(height))
    .class(CosmicButton::AppletIcon)
}

fn applet_button_size(core: &cosmic::Core) -> (f32, f32) {
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
    let width = logo_width + 6.0 + bar_width + f32::from(2 * horizontal_padding);
    let height = f32::from(suggested_h + 2 * vertical_padding);

    (width, height)
}

fn applet_bar_width(suggested_w: u16, suggested_h: u16) -> f32 {
    let min_width = suggested_h.saturating_mul(APPLET_BAR_WIDTH_HEIGHT_MULTIPLIER);

    f32::from(suggested_w.max(min_width))
}

fn selected_provider_percents(state: &AppState, selected_provider: ProviderId) -> (f32, f32) {
    let now = chrono::Utc::now();
    state
        .providers
        .iter()
        .find(|p| p.provider == selected_provider)
        .and_then(|p| p.snapshot.as_ref())
        .map_or((0.0, 0.0), |snapshot| {
            let (primary, secondary) = snapshot.applet_windows();
            (
                primary.map_or(0.0, |w| usage_display::displayed_percent(w, now)),
                secondary.map_or(0.0, |w| usage_display::displayed_percent(w, now)),
            )
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

impl AppModel {
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
        self.write_config(|new_config| match provider {
            ProviderId::Codex => new_config.codex_enabled = enabled,
            ProviderId::Claude => new_config.claude_enabled = enabled,
            ProviderId::Cursor => new_config.cursor_enabled = enabled,
        });
        if enabled {
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
}

fn apply_refreshed_state(
    config: &Config,
    current_state: &mut AppState,
    selected_provider: &mut ProviderId,
    loaded_state: AppState,
) -> Task<Message> {
    *current_state = loaded_state;
    *selected_provider = select_provider(*selected_provider, current_state);
    refresh_provider_tasks(config, current_state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_refreshed_marks_enabled_providers_refreshing() {
        let config = Config::default();
        let loaded_state = AppState::empty();
        let mut current_state = AppState::empty();
        let mut selected_provider = ProviderId::Codex;

        let _task = apply_refreshed_state(
            &config,
            &mut current_state,
            &mut selected_provider,
            loaded_state,
        );

        for provider in ProviderId::ALL {
            let state = current_state.provider(provider).unwrap();
            assert!(state.enabled);
            assert!(state.is_refreshing);
        }
    }

    #[test]
    fn apply_refreshed_keeps_disabled_provider_out_of_refresh() {
        let config = Config {
            cursor_enabled: false,
            ..Config::default()
        };
        let mut loaded_state = AppState::empty();
        for provider in &mut loaded_state.providers {
            provider.enabled = config.provider_enabled(provider.provider);
        }
        let mut current_state = AppState::empty();
        let mut selected_provider = ProviderId::Cursor;

        let _task = apply_refreshed_state(
            &config,
            &mut current_state,
            &mut selected_provider,
            loaded_state,
        );

        assert_eq!(selected_provider, ProviderId::Codex);
        assert!(
            current_state
                .provider(ProviderId::Codex)
                .unwrap()
                .is_refreshing
        );
        assert!(
            current_state
                .provider(ProviderId::Claude)
                .unwrap()
                .is_refreshing
        );
        let cursor = current_state.provider(ProviderId::Cursor).unwrap();
        assert!(!cursor.enabled);
        assert!(!cursor.is_refreshing);
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
}
