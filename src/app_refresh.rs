// SPDX-License-Identifier: MPL-2.0

use crate::app::Message;
use crate::config::Config;
use crate::model::{AppState, ProviderId};
use crate::runtime;
use cosmic::app::Task;

pub fn refresh_provider_tasks(config: &Config, state: &mut AppState) -> Task<Message> {
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
                |provider_state| {
                    cosmic::Action::App(Message::ProviderRefreshed(Box::new(provider_state)))
                },
            ));
        }
    }

    if tasks.is_empty() {
        Task::none()
    } else {
        Task::batch(tasks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_tasks_mark_enabled_providers_refreshing() {
        let config = Config::default();
        let mut state = AppState::empty();

        let _tasks = refresh_provider_tasks(&config, &mut state);

        for provider in ProviderId::ALL {
            let entry = state.provider(provider).unwrap();
            assert!(entry.enabled);
            assert!(entry.is_refreshing);
        }
    }

    #[test]
    fn refresh_tasks_skip_disabled_provider() {
        let config = Config {
            cursor_enabled: false,
            ..Config::default()
        };
        let mut state = AppState::empty();
        for p in &mut state.providers {
            p.enabled = config.provider_enabled(p.provider);
        }

        let _tasks = refresh_provider_tasks(&config, &mut state);

        let cursor = state.provider(ProviderId::Cursor).unwrap();
        assert!(!cursor.enabled);
        assert!(!cursor.is_refreshing);
    }

    #[test]
    fn refresh_tasks_skip_already_refreshing_provider() {
        let config = Config::default();
        let mut state = AppState::empty();
        state.mark_provider_refreshing(ProviderId::Codex, true);

        let _tasks = refresh_provider_tasks(&config, &mut state);

        assert!(state.provider(ProviderId::Codex).unwrap().is_refreshing);
    }
}
