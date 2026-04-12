use crate::config::AppConfig;
use crate::model::{AppState, ProviderId};
use crate::popup_view::popup_content;
use crate::runtime;
use cosmic::app::{Core, Task};
use cosmic::iced::time;
use cosmic::iced::window::Id;
use cosmic::iced::{Rectangle, Subscription};
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::{Element, iced, task};
use std::time::Duration;

pub struct AppModel {
    pub(crate) core: Core,
    pub(crate) popup: Option<Id>,
    pub(crate) config: AppConfig,
    pub(crate) state: AppState,
    pub(crate) selected_provider: ProviderId,
}

#[derive(Debug, Clone)]
pub enum Message {
    PopupClosed(Id),
    Surface(cosmic::surface::Action),
    SelectProvider(ProviderId),
    Tick,
    RefreshNow,
    Refreshed(AppState),
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
        };
        (
            model,
            Task::perform(
                async move { runtime::load_initial_state(&initial_config).await },
                |state| cosmic::Action::App(Message::Refreshed(state)),
            ),
        )
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let have_popup = self.popup;
        let button =
            self.core
                .applet
                .icon_button("utilities-terminal-symbolic")
                .on_press_with_rectangle(move |offset, bounds| {
                    if let Some(id) = have_popup {
                        Message::Surface(destroy_popup(id))
                    } else {
                        Message::Surface(app_popup::<AppModel>(
                            move |state: &mut AppModel| {
                                let popup_id = Id::unique();
                                state.popup = Some(popup_id);
                                let mut popup_settings =
                                    if let Some(main_window_id) = state.core.main_window_id() {
                                        state.core.applet.get_popup_settings(
                                            main_window_id,
                                            popup_id,
                                            None,
                                            None,
                                            None,
                                        )
                                    } else {
                                        state.core.applet.get_popup_settings(
                                            popup_id, popup_id, None, None, None,
                                        )
                                    };
                                popup_settings.positioner.anchor_rect = Rectangle {
                                    x: (bounds.x - offset.x) as i32,
                                    y: (bounds.y - offset.y) as i32,
                                    width: bounds.width as i32,
                                    height: bounds.height as i32,
                                };
                                popup_settings.positioner.size_limits = cosmic::iced::Limits::NONE
                                    .min_width(380.0)
                                    .max_width(520.0)
                                    .min_height(420.0)
                                    .max_height(900.0);
                                popup_settings
                            },
                            Some(Box::new(move |state: &AppModel| {
                                let content = popup_content(&state.state, state.selected_provider);
                                Element::from(state.core.applet.popup_container(content))
                                    .map(cosmic::Action::App)
                            })),
                        ))
                    }
                });

        Element::from(self.core.applet.applet_tooltip::<Message>(
            button,
            "YapCap",
            self.popup.is_some(),
            Message::Surface,
            None,
        ))
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
                let config = self.config.clone();
                return Task::perform(
                    async move { runtime::refresh_all(&config).await },
                    |state| cosmic::Action::App(Message::Refreshed(state)),
                );
            }
            Message::Refreshed(state) => {
                self.state = state;
                self.selected_provider = select_provider(self.selected_provider, &self.state);
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

fn select_provider(current: ProviderId, state: &AppState) -> ProviderId {
    if state
        .providers
        .iter()
        .any(|provider| provider.provider == current)
    {
        current
    } else {
        state
            .providers
            .first()
            .map(|provider| provider.provider)
            .unwrap_or(ProviderId::Codex)
    }
}
