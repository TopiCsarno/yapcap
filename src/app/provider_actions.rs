use super::{
    AccountSelectionStatus, AppModel, Config, CosmicConfigEntry, Id, Message, PanelIconStyle,
    PopupRoute, ProviderId, ProviderRefreshResult, ResetTimeFormat, Size, Task, UpdateStatus,
    UsageAmountFormat, app_popup, applet_button_size, claude, cosmic_config, cursor, demo_env,
    destroy_popup, format_retry_delay, popup_size_limits_with_max_width, popup_size_tuple,
    popup_view, refresh_provider_account_statuses_task, refresh_provider_task,
    refresh_provider_tasks, registry, resize_popup, runtime, select_provider, update_retry_delay,
    update_retry_task,
};
use crate::account_selection::provider_show_all_account_selection;

impl AppModel {
    pub(super) fn handle_provider_refreshed(
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
            self.update_cursor_active_account();
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

    pub(super) fn handle_update_checked(
        &mut self,
        status: UpdateStatus,
        attempt: u32,
    ) -> Task<Message> {
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

    pub(super) fn navigate_to(&mut self, route: PopupRoute) -> Option<Task<Message>> {
        let resize = self.resize_popup_to_route(&route);
        self.popup_route = route;
        resize
    }

    pub(super) fn popup_size_for_route(&self, route: &PopupRoute) -> Size {
        match route {
            PopupRoute::ProviderDetail => {
                popup_view::popup_session_size(&self.state, self.selected_provider)
            }
            PopupRoute::Settings(_) => popup_view::popup_settings_size(&self.state),
        }
    }

    pub(super) fn sync_panel_suggested_bounds(&mut self) {
        let n_accounts = self
            .state
            .display_selected_account_count(self.selected_provider);
        let (w, h) = applet_button_size(&self.core, self.config.panel_icon_style, n_accounts);
        self.core.applet.suggested_bounds = Some(Size::new(w, h));
    }

    pub(super) fn select_provider_tab(&mut self, provider: ProviderId) -> Option<Task<Message>> {
        self.selected_provider = provider;
        self.sync_panel_suggested_bounds();
        self.resize_popup_to_provider(provider)
    }

    pub(super) fn resize_popup_to_provider(
        &mut self,
        provider: ProviderId,
    ) -> Option<Task<Message>> {
        let new_size = popup_view::popup_session_size(&self.state, provider);
        self.resize_popup_to_size(new_size)
    }

    pub(super) fn resize_popup_to_route(&mut self, route: &PopupRoute) -> Option<Task<Message>> {
        let new_size = self.popup_size_for_route(route);
        self.resize_popup_to_size(new_size)
    }

    pub(super) fn resize_popup_to_size(&mut self, new_size: Size) -> Option<Task<Message>> {
        let popup_id = self.popup?;
        self.popup_size = Some(new_size);
        let (w, h) = popup_size_tuple(new_size);
        Some(resize_popup(popup_id, w, h))
    }

    pub(super) fn toggle_popup(&mut self) -> Task<Message> {
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

    pub(super) fn write_config(&mut self, f: impl FnOnce(&mut Config)) {
        if let Ok(ctx) =
            cosmic_config::Config::new(<Self as cosmic::Application>::APP_ID, Config::VERSION)
        {
            let mut new_config = self.config.clone();
            f(&mut new_config);
            let _ = new_config.write_entry(&ctx);
            self.config = new_config;
        }
    }

    pub(super) fn set_provider_enabled(
        &mut self,
        provider: ProviderId,
        enabled: bool,
    ) -> Task<Message> {
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

    pub(super) fn set_refresh_interval(&mut self, interval_seconds: u64) -> Task<Message> {
        self.write_config(|new_config| {
            new_config.refresh_interval_seconds = interval_seconds;
        });
        Task::none()
    }

    pub(super) fn set_reset_time_format(&mut self, format: ResetTimeFormat) -> Task<Message> {
        self.write_config(|new_config| {
            new_config.reset_time_format = format;
        });
        Task::none()
    }

    pub(super) fn set_usage_amount_format(&mut self, format: UsageAmountFormat) -> Task<Message> {
        self.write_config(|new_config| {
            new_config.usage_amount_format = format;
        });
        Task::none()
    }

    pub(super) fn set_panel_icon_style(&mut self, style: PanelIconStyle) -> Task<Message> {
        self.write_config(|new_config| {
            new_config.panel_icon_style = style;
        });
        self.sync_panel_suggested_bounds();
        Task::none()
    }

    pub(super) fn set_show_all_accounts(
        &mut self,
        provider: ProviderId,
        show_all: bool,
    ) -> Task<Message> {
        self.write_config(|c| {
            c.set_provider_show_all(provider, show_all);
            if show_all {
                *c.selected_account_ids_mut(provider) =
                    provider_show_all_account_selection(c, provider);
            }
        });
        runtime::reconcile_provider(&self.config, &mut self.state, provider);
        Task::none()
    }

    pub(super) fn on_host_cli_auth_changed(&mut self) {
        if demo_env::is_active() {
            return;
        }
        runtime::reconcile_provider(&self.config, &mut self.state, ProviderId::Codex);
        runtime::reconcile_provider(&self.config, &mut self.state, ProviderId::Claude);
        runtime::persist_state(&self.state);
        self.sync_panel_suggested_bounds();
    }

    pub(super) fn on_config_update(&mut self, config: Config) {
        let mut config = config;
        demo_env::apply_config(&mut config);
        self.config = config;
        runtime::reconcile_state(&self.config, &mut self.state);
        demo_env::apply(&self.config, &mut self.state);
        runtime::persist_state(&self.state);
        self.sync_panel_suggested_bounds();
    }

    pub(super) fn toggle_account_selection(
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

    pub(super) fn delete_codex_account(&mut self, account_id: &str) -> Task<Message> {
        let provider = ProviderId::Codex;
        if !self
            .config
            .codex_managed_accounts
            .iter()
            .any(|account| account.id == account_id)
        {
            return Task::none();
        }

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

    pub(super) fn delete_claude_account(&mut self, account_id: &str) -> Task<Message> {
        let provider = ProviderId::Claude;
        if !self
            .config
            .claude_managed_accounts
            .iter()
            .any(|account| account.id == account_id)
        {
            return Task::none();
        }

        claude::remove_managed_config_dir(&crate::config::managed_claude_account_dir(account_id));
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

    pub(super) fn delete_cursor_account(&mut self, account_id: &str) -> Task<Message> {
        let provider = ProviderId::Cursor;
        if cursor::find_managed_account(&self.config.cursor_managed_accounts, account_id).is_none()
        {
            return Task::none();
        }

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
}
