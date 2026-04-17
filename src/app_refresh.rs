use crate::config::AppConfig;
use crate::cosmic_app::Message;
use crate::model::{AppState, ProviderId};
use crate::runtime;
use cosmic::app::Task;

pub fn refresh_provider_tasks(config: &AppConfig, state: &mut AppState) -> Task<Message> {
    let mut tasks = Vec::new();

    for provider in ProviderId::ALL {
        let enabled = config.provider_enabled(provider);
        let already_refreshing = state
            .provider(provider)
            .is_some_and(|entry| entry.is_refreshing);
        state.mark_provider_refreshing(provider, enabled);

        if enabled && !already_refreshing {
            let config = config.clone();
            let previous = state.provider(provider).cloned();
            tasks.push(Task::perform(
                async move { runtime::refresh_one(config, provider, previous).await },
                |provider_state| cosmic::Action::App(Message::ProviderRefreshed(provider_state)),
            ));
        }
    }

    if tasks.is_empty() {
        Task::none()
    } else {
        Task::batch(tasks)
    }
}
