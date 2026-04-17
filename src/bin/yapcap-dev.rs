/// Standalone dev window — renders the popup view in a normal resizable window.
/// Run with:  cargo run --bin yapcap-dev
///
/// No panel required. All providers refresh on the same schedule as the real
/// applet so you can iterate on UI without reinstalling.
use yapcap::{
    app_refresh::refresh_provider_tasks,
    config::AppConfig,
    cosmic_app::Message,
    logging,
    model::{AppState, ProviderId},
    popup_view::popup_content,
    runtime,
};

use cosmic::app::{Core, Task};
use cosmic::iced::{Limits, Size, Subscription, time};
use std::time::Duration;

struct DevApp {
    core: Core,
    config: AppConfig,
    state: AppState,
    selected_provider: ProviderId,
}

impl cosmic::Application for DevApp {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = "com.topi.YapCap.Dev";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: ()) -> (Self, Task<Message>) {
        let config = AppConfig::load().unwrap_or_else(|e| {
            eprintln!("failed to load config: {e}");
            AppConfig::default()
        });
        let initial_config = config.clone();
        let app = Self {
            core,
            config,
            state: AppState::empty(),
            selected_provider: ProviderId::Codex,
        };
        let task = Task::perform(
            async move { runtime::load_initial_state(&initial_config).await },
            |state| cosmic::Action::App(Message::Refreshed(state)),
        );
        (app, task)
    }

    fn view(&self) -> cosmic::Element<'_, Message> {
        popup_content(&self.state, self.selected_provider)
    }

    fn subscription(&self) -> Subscription<Message> {
        time::every(Duration::from_secs(
            self.config.refresh_interval_seconds.max(10),
        ))
        .map(|_| Message::Tick)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SelectProvider(provider) => {
                self.selected_provider = provider;
            }
            Message::Tick | Message::RefreshNow => {
                return refresh_provider_tasks(&self.config, &mut self.state);
            }
            Message::Refreshed(state) => {
                self.state = state;
                if !self
                    .state
                    .providers
                    .iter()
                    .any(|p| p.provider == self.selected_provider)
                {
                    if let Some(first) = self.state.providers.first() {
                        self.selected_provider = first.provider;
                    }
                }
            }
            Message::ProviderRefreshed(provider_state) => {
                self.state.upsert_provider(provider_state);
                runtime::persist_state(&self.state);
                if !self
                    .state
                    .providers
                    .iter()
                    .any(|p| p.provider == self.selected_provider)
                {
                    if let Some(first) = self.state.providers.first() {
                        self.selected_provider = first.provider;
                    }
                }
            }
            Message::Quit => return cosmic::iced::exit(),
            // Applet-only messages — no-op in standalone mode.
            Message::Surface(_) | Message::PopupClosed(_) => {}
        }
        Task::none()
    }
}

fn main() -> cosmic::iced::Result {
    let config = AppConfig::load().unwrap_or_default();
    let _logging_guard = logging::init(&config.log_level).ok();

    cosmic::app::run::<DevApp>(
        cosmic::app::Settings::default()
            .size(Size::new(460.0, 700.0))
            .size_limits(Limits::new(
                Size::new(380.0, 420.0),
                Size::new(600.0, 900.0),
            )),
        (),
    )
}
