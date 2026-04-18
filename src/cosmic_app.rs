use crate::app_refresh::refresh_provider_tasks;
use crate::config::AppConfig;
use crate::model::{AppState, ProviderId, ProviderRuntimeState};
use crate::popup_view::popup_content;
use crate::provider_assets::{ProviderIconVariant, provider_icon_handle};
use crate::runtime;
use crate::updates::{self, UpdateStatus};
use crate::usage_display;
use cosmic::app::{Core, Task};
use cosmic::iced::time;
use cosmic::iced::widget::{column, progress_bar, row};
use cosmic::iced::window::Id;
use cosmic::iced::{Alignment, Background, Length, Rectangle, Subscription};
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::theme::Button as CosmicButton;
use cosmic::{Element, iced, task, widget};
use std::time::Duration;

const POPUP_WIDTH: u32 = 420;
// Fixed at creation: xdg_popup surfaces can't grow, so size for a full provider.
const POPUP_HEIGHT: u32 = 720;

pub struct AppModel {
    pub(crate) core: Core,
    pub(crate) popup: Option<Id>,
    pub(crate) config: AppConfig,
    pub(crate) state: AppState,
    pub(crate) selected_provider: ProviderId,
    pub(crate) show_settings: bool,
    pub(crate) update_status: UpdateStatus,
}

#[derive(Debug, Clone)]
// `Surface` carries cosmic's Action struct, which is ~64 bytes larger than the other
// variants. Boxing it would break the `Message::Surface` fn-pointer handoff to
// `applet_tooltip`, so we accept the size delta here.
#[allow(clippy::large_enum_variant)]
pub enum Message {
    PopupClosed(Id),
    Surface(cosmic::surface::Action),
    SelectProvider(ProviderId),
    Tick,
    RefreshNow,
    Refreshed(AppState),
    ProviderRefreshed(ProviderRuntimeState),
    ToggleSettings,
    SetProviderEnabled(ProviderId, bool),
    UpdateChecked(UpdateStatus),
    OpenUrl(String),
    Quit,
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = "com.topi.YapCap";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Self::Message>) {
        let config = match AppConfig::load() {
            Ok(config) => config,
            Err(error) => {
                eprintln!("failed to load config: {error}");
                AppConfig::default()
            }
        };
        let initial_config = config.clone();
        let model = Self {
            core,
            popup: None,
            config,
            state: AppState::empty(),
            selected_provider: ProviderId::Codex,
            show_settings: false,
            update_status: UpdateStatus::Unchecked,
        };
        (
            model,
            Task::batch([
                Task::perform(
                    async move { runtime::load_initial_state(&initial_config).await },
                    |state| cosmic::Action::App(Message::Refreshed(state)),
                ),
                Task::perform(
                    async move {
                        let client = reqwest::Client::new();
                        updates::check(&client).await
                    },
                    |status| cosmic::Action::App(Message::UpdateChecked(status)),
                ),
            ]),
        )
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let have_popup = self.popup;
        let button = applet_button(
            &self.core,
            applet_indicator(&self.state, self.selected_provider, &self.core),
        )
        .on_press_with_rectangle(move |offset, bounds| {
            if let Some(id) = have_popup {
                Message::Surface(destroy_popup(id))
            } else {
                Message::Surface(app_popup::<AppModel>(
                    move |state: &mut AppModel| {
                        let popup_id = Id::unique();
                        state.popup = Some(popup_id);
                        let initial_size = (POPUP_WIDTH, POPUP_HEIGHT);
                        let mut popup_settings =
                            if let Some(main_window_id) = state.core.main_window_id() {
                                state.core.applet.get_popup_settings(
                                    main_window_id,
                                    popup_id,
                                    Some(initial_size),
                                    None,
                                    None,
                                )
                            } else {
                                state.core.applet.get_popup_settings(
                                    popup_id,
                                    popup_id,
                                    Some(initial_size),
                                    None,
                                    None,
                                )
                            };
                        popup_settings.positioner.anchor_rect = Rectangle {
                            x: (bounds.x - offset.x) as i32,
                            y: (bounds.y - offset.y) as i32,
                            width: bounds.width as i32,
                            height: bounds.height as i32,
                        };
                        popup_settings.positioner.size = Some(initial_size);
                        popup_settings.positioner.size_limits = cosmic::iced::Limits::NONE
                            .width(POPUP_WIDTH as f32)
                            .height(POPUP_HEIGHT as f32);
                        popup_settings
                    },
                    Some(Box::new(move |state: &AppModel| {
                        let content = popup_content(
                            &state.state,
                            state.selected_provider,
                            state.show_settings,
                            &state.update_status,
                        );
                        popup_container(content).map(cosmic::Action::App)
                    })),
                ))
            }
        });

        Element::from(
            self.core
                .applet
                .autosize_window(self.core.applet.applet_tooltip::<Message>(
                    button,
                    "YapCap",
                    self.popup.is_some(),
                    Message::Surface,
                    None,
                )),
        )
    }

    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        "YapCap".into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        time::every(Duration::from_secs(
            self.config.refresh_interval_seconds.max(10),
        ))
        .map(|_| Message::Tick)
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                }
            }
            Message::Surface(action) => {
                return task::message(cosmic::Action::Cosmic(cosmic::app::Action::Surface(action)));
            }
            Message::SelectProvider(provider) => {
                self.selected_provider = provider;
            }
            Message::Tick | Message::RefreshNow => {
                return refresh_provider_tasks(&self.config, &mut self.state);
            }
            Message::Refreshed(state) => {
                self.state = state;
                self.selected_provider = select_provider(self.selected_provider, &self.state);
            }
            Message::ProviderRefreshed(provider_state) => {
                self.state.upsert_provider(provider_state);
                runtime::persist_state(&self.state);
                self.selected_provider = select_provider(self.selected_provider, &self.state);
            }
            Message::ToggleSettings => {
                self.show_settings = !self.show_settings;
            }
            Message::SetProviderEnabled(provider, enabled) => {
                match provider {
                    ProviderId::Codex => self.config.codex_enabled = enabled,
                    ProviderId::Claude => self.config.claude_enabled = enabled,
                    ProviderId::Cursor => self.config.cursor_enabled = enabled,
                }
                if let Err(error) = self.config.save() {
                    eprintln!("failed to save config: {error}");
                }
                for entry in &mut self.state.providers {
                    entry.enabled = self.config.provider_enabled(entry.provider);
                }
                self.selected_provider = select_provider(self.selected_provider, &self.state);
                if enabled {
                    return refresh_provider_tasks(&self.config, &mut self.state);
                }
            }
            Message::UpdateChecked(status) => {
                self.update_status = status;
            }
            Message::OpenUrl(url) => {
                if let Err(error) = std::process::Command::new("xdg-open").arg(&url).spawn() {
                    eprintln!("failed to open url {url}: {error}");
                }
            }
            Message::Quit => {
                return iced::exit();
            }
        }
        Task::none()
    }

    fn style(&self) -> Option<iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

fn popup_container<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    Element::from(
        widget::container(content)
            .width(Length::Fixed(POPUP_WIDTH as f32))
            .height(Length::Fixed(POPUP_HEIGHT as f32))
            .class(cosmic::theme::Container::custom(|theme| {
                let cosmic = theme.cosmic();
                let corners = cosmic.corner_radii;
                cosmic::iced::widget::container::Style {
                    text_color: Some(cosmic.background.on.into()),
                    background: Some(Background::Color(cosmic.background.base.into())),
                    border: cosmic::iced::Border {
                        radius: corners.radius_m.into(),
                        width: 1.0,
                        color: cosmic.background.divider.into(),
                    },
                    icon_color: Some(cosmic.background.on.into()),
                    ..Default::default()
                }
            })),
    )
}

fn select_provider(current: ProviderId, state: &AppState) -> ProviderId {
    if state
        .providers
        .iter()
        .any(|provider| provider.provider == current && provider.enabled)
    {
        current
    } else {
        state
            .providers
            .iter()
            .find(|provider| provider.enabled)
            .map(|provider| provider.provider)
            .unwrap_or(ProviderId::Codex)
    }
}

fn applet_indicator<'a>(
    state: &AppState,
    selected_provider: ProviderId,
    core: &Core,
) -> Element<'a, Message> {
    let (suggested_w, suggested_h) = core.applet.suggested_size(false);
    let compact_size = f32::from(suggested_w.min(suggested_h));
    let logo_size = (compact_size - 8.0).max(11.0);
    let bar_width = f32::from(suggested_w.max(suggested_h)).max(40.0);
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

    let content = row![
        widget::icon::icon(provider_icon_handle(
            selected_provider,
            provider_icon_variant(),
        ))
        .size(logo_size as u16)
        .width(Length::Fixed(logo_size))
        .height(Length::Fixed(logo_size)),
        bars,
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    Element::from(content)
}

fn applet_button<'a>(
    core: &Core,
    content: impl Into<Element<'a, Message>>,
) -> cosmic::widget::Button<'a, Message> {
    let (_, suggested_h) = core.applet.suggested_size(false);
    let (major_padding, minor_padding) = core.applet.suggested_padding(true);
    let (horizontal_padding, vertical_padding) = if core.applet.is_horizontal() {
        (major_padding, minor_padding)
    } else {
        (minor_padding, major_padding)
    };
    let height = (suggested_h + 2 * vertical_padding) as f32;

    widget::button::custom(
        widget::layer_container(content)
            .padding(cosmic::iced::Padding::from([0, horizontal_padding]))
            .align_y(cosmic::iced::alignment::Vertical::Center.into()),
    )
    .width(Length::Shrink)
    .height(Length::Fixed(height))
    .class(CosmicButton::AppletIcon)
}

fn selected_provider_percents(state: &AppState, selected_provider: ProviderId) -> (f32, f32) {
    let now = chrono::Utc::now();
    state
        .providers
        .iter()
        .find(|provider| provider.provider == selected_provider)
        .and_then(|provider| provider.snapshot.as_ref())
        .map(|snapshot| {
            let (primary, secondary) = snapshot.applet_windows();
            (
                primary
                    .map(|window| usage_display::displayed_percent(window, now) as f32)
                    .unwrap_or(0.0),
                secondary
                    .map(|window| usage_display::displayed_percent(window, now) as f32)
                    .unwrap_or(0.0),
            )
        })
        .unwrap_or((0.0, 0.0))
}

fn provider_icon_variant() -> ProviderIconVariant {
    if cosmic::theme::is_dark() {
        ProviderIconVariant::Reversed
    } else {
        ProviderIconVariant::Default
    }
}
