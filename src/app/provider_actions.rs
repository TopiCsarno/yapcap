use super::{
    AccountSelectionStatus, AppModel, Config, CosmicConfigEntry, Id, Message, PagerDirection,
    PanelIconStyle, PopupRoute, ProviderId, ProviderRefreshResult, ResetTimeFormat, Size, Task,
    UpdateStatus, UsageAmountFormat, app_popup, demo_env, destroy_popup, format_retry_delay,
    panel_button_size, popup_view, refresh_provider_account_statuses_task, registry, runtime,
    select_provider, update_retry_delay, update_retry_task,
};
use crate::config::APP_ID;
use crate::shared_state::{self, ProviderRefreshRequest, RefreshRequestReason};
use chrono::Utc;

impl AppModel {
    pub(super) fn handle_provider_refreshed(
        &mut self,
        refresh_result: ProviderRefreshResult,
    ) -> Task<Message> {
        let ProviderRefreshResult { provider, accounts } = refresh_result;
        let refreshed_provider = provider.provider;
        let refreshed_selected_ids = provider.selected_account_ids.clone();
        tracing::info!(
            process_id = %self.process_info.id,
            owner_status = self.owner_status(),
            provider = refreshed_provider.label(),
            account_count = accounts.len(),
            "provider refresh finished"
        );
        self.state.upsert_provider(provider);
        for account in accounts {
            self.state.upsert_account(account);
        }
        super::session::sync_metadata_after_refresh(self, refreshed_provider);
        if self.config.selected_account_ids(refreshed_provider) != refreshed_selected_ids.as_slice()
        {
            self.write_config(|new_config| {
                new_config
                    .selected_account_ids_mut(refreshed_provider)
                    .clone_from(&refreshed_selected_ids);
            });
        }
        self.persist_runtime_if_owner("provider_refresh_finished");
        self.consume_shared_refresh_request(refreshed_provider);
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

    pub(super) fn consume_shared_refresh_request(&mut self, provider: ProviderId) {
        self.consume_shared_refresh_requests(&[provider]);
    }

    pub(super) fn consume_shared_refresh_requests(&mut self, providers: &[ProviderId]) {
        if self.refresh_owner.is_none() {
            return;
        }
        match shared_state::remove_control_requests_for_providers(
            APP_ID,
            &self.shared_control,
            providers,
        ) {
            Ok(shared_control) => {
                if shared_control == self.shared_control {
                    return;
                }
                self.shared_control = shared_control;
                if let [provider] = providers {
                    tracing::info!(
                        process_id = %self.process_info.id,
                        owner_status = self.owner_status(),
                        provider = provider.label(),
                        generation = self.shared_control.generation,
                        request_count = self.shared_control.requests.len(),
                        "shared refresh request consumed"
                    );
                } else {
                    tracing::info!(
                        process_id = %self.process_info.id,
                        owner_status = self.owner_status(),
                        provider_count = providers.len(),
                        generation = self.shared_control.generation,
                        request_count = self.shared_control.requests.len(),
                        "shared refresh requests consumed"
                    );
                }
            }
            Err(error) => {
                tracing::error!(
                    pid = self.process_info.pid,
                    process_id = %self.process_info.id,
                    owner_status = self.owner_status(),
                    provider_count = providers.len(),
                    error = ?error,
                    "failed to consume shared refresh requests"
                );
            }
        }
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

    pub(super) fn navigate_to(&mut self, route: PopupRoute) {
        tracing::info!(
            process_id = %self.process_info.id,
            from = popup_route_label(self.popup_route),
            from_provider = popup_route_provider_label(self.popup_route, self.selected_provider),
            to = popup_route_label(route),
            to_provider = popup_route_provider_label(route, self.selected_provider),
            "popup navigation requested"
        );
        self.popup_route = route;
    }

    pub(super) fn sync_panel_suggested_bounds(&mut self) {
        let (w, h) = panel_button_size(&self.core, &self.state, self.config.panel_icon_style);
        self.core.applet.suggested_bounds = Some(Size::new(w, h));
    }

    pub(super) fn page_provider_account(&mut self, direction: PagerDirection) -> Task<Message> {
        let accounts = self.state.accounts_for(self.selected_provider);
        if accounts.len() > 1 {
            let account_page = match direction {
                PagerDirection::Previous => {
                    popup_view::account_page_previous(self.detail_account_page, accounts.len())
                }
                PagerDirection::Next => {
                    popup_view::account_page_next(self.detail_account_page, accounts.len())
                }
            };
            let account_id = accounts[account_page].account_id.clone();
            return self.toggle_account_selection(self.selected_provider, &account_id);
        }
        Task::none()
    }

    pub(super) fn page_provider_viewport(&mut self, direction: PagerDirection) {
        let max_offset = popup_view::provider_viewport_max_offset_for(&self.state);
        let previous = self.provider_viewport_offset;
        self.provider_viewport_offset = match direction {
            PagerDirection::Previous => previous.saturating_sub(1),
            PagerDirection::Next => previous.saturating_add(1).min(max_offset),
        };
    }

    pub(super) fn select_provider_tab(&mut self, provider: ProviderId) -> Task<Message> {
        let previous = self.selected_provider;
        self.selected_provider = provider;
        self.detail_account_page = self.state.selected_account_index(provider);
        tracing::info!(
            process_id = %self.process_info.id,
            previous_provider = previous.label(),
            selected_provider = provider.label(),
            "provider tab selected"
        );
        self.write_config(|new_config| {
            new_config.selected_provider = provider;
        });
        self.sync_panel_suggested_bounds();
        self.request_refresh_for_selected_provider(provider)
    }

    fn request_refresh_for_selected_provider(&mut self, provider: ProviderId) -> Task<Message> {
        if !super::selected_account_refresh_due(&self.config, &self.state, provider) {
            return Task::none();
        }
        self.request_provider_refresh(provider, RefreshRequestReason::ProviderSelected)
    }

    pub(super) fn request_provider_refresh(
        &mut self,
        provider: ProviderId,
        reason: RefreshRequestReason,
    ) -> Task<Message> {
        let mut shared_control = self.shared_control.clone();
        shared_control.upsert_request(ProviderRefreshRequest {
            provider,
            reason,
            requested_at: Utc::now(),
            requesting_process_id: self.process_info.id.clone(),
        });
        match shared_state::save_control(APP_ID, &shared_control) {
            Ok(()) => {
                tracing::info!(
                    provider = provider.label(),
                    process_id = %self.process_info.id,
                    owner_status = self.owner_status(),
                    reason = ?reason,
                    generation = shared_control.generation,
                    "provider refresh request written"
                );
            }
            Err(error) => {
                tracing::error!(
                    pid = self.process_info.pid,
                    process_id = %self.process_info.id,
                    owner_status = self.owner_status(),
                    provider = provider.label(),
                    error = ?error,
                    "failed to save provider refresh request"
                );
            }
        }
        self.handle_shared_control_update(shared_control)
    }

    pub(super) fn toggle_popup(&mut self) -> Task<Message> {
        if let Some(p) = self.popup.take() {
            tracing::info!(
                process_id = %self.process_info.id,
                route = popup_route_label(self.popup_route),
                provider = popup_route_provider_label(self.popup_route, self.selected_provider),
                "popup closed by panel toggle"
            );
            return cosmic::task::message(cosmic::Action::Cosmic(cosmic::app::Action::Surface(
                destroy_popup(p),
            )));
        }

        tracing::info!(
            process_id = %self.process_info.id,
            route = popup_route_label(self.popup_route),
            provider = popup_route_provider_label(self.popup_route, self.selected_provider),
            "popup opened"
        );
        cosmic::task::message(cosmic::Action::Cosmic(cosmic::app::Action::Surface(
            app_popup::<Self>(
                |_| Default::default(),
                move |state| {
                    let new_id = Id::unique();
                    state.popup.replace(new_id);
                    let mut settings = state.core.applet.get_popup_settings(
                        state.core.main_window_id().unwrap(),
                        new_id,
                        None,
                        None,
                        None,
                    );
                    settings.positioner.size_limits = super::popup_size_limits();
                    settings
                },
                None,
            ),
        )))
    }

    pub(super) fn write_config(&mut self, f: impl FnOnce(&mut Config)) {
        let mut new_config = self.config.clone();
        f(&mut new_config);
        if new_config == self.config {
            return;
        }
        if demo_env::is_active() {
            self.config = new_config;
            return;
        }
        let ctx = match crate::config::cosmic_config_context(
            <Self as cosmic::Application>::APP_ID,
            Config::VERSION,
        ) {
            Ok(ctx) => ctx,
            Err(error) => {
                tracing::error!(
                    pid = self.process_info.pid,
                    process_id = %self.process_info.id,
                    owner_status = self.owner_status(),
                    error = ?error,
                    "failed to open config for writing"
                );
                return;
            }
        };
        if let Err(error) =
            crate::config::write_changed_config_entries(&ctx, &self.config, &new_config)
        {
            tracing::error!(
                pid = self.process_info.pid,
                process_id = %self.process_info.id,
                owner_status = self.owner_status(),
                error = ?error,
                "failed to write config"
            );
            return;
        }
        self.config = new_config;
    }

    pub(super) fn set_provider_enabled(
        &mut self,
        provider: ProviderId,
        enabled: bool,
    ) -> Task<Message> {
        let previous = self
            .state
            .provider(provider)
            .is_some_and(|entry| entry.enabled);
        if let Some(entry) = self.state.provider_mut(provider) {
            entry.enabled = enabled;
        }
        self.selected_provider = select_provider(self.selected_provider, &self.state);
        let selected_provider = self.selected_provider;
        self.write_config(|new_config| {
            new_config.set_provider_enabled(provider, enabled);
            new_config.selected_provider = selected_provider;
        });
        tracing::info!(
            process_id = %self.process_info.id,
            provider = provider.label(),
            previous,
            enabled,
            selected_provider = self.selected_provider.label(),
            "provider enabled setting changed"
        );
        runtime::reconcile_provider(&self.config, &self.detection, &mut self.state, provider);
        self.sync_panel_suggested_bounds();
        if enabled
            && self
                .state
                .provider(provider)
                .is_some_and(|entry| entry.account_status == AccountSelectionStatus::Ready)
        {
            return self.request_provider_refresh(provider, RefreshRequestReason::AccountAction);
        }
        self.persist_runtime_if_owner("provider_setting_changed");
        Task::none()
    }

    pub(super) fn set_refresh_interval(&mut self, interval_seconds: u64) -> Task<Message> {
        let previous = self.config.refresh_interval_seconds;
        self.write_config(|new_config| {
            new_config.refresh_interval_seconds = interval_seconds;
        });
        tracing::info!(
            process_id = %self.process_info.id,
            previous_seconds = previous,
            interval_seconds,
            "refresh interval setting changed"
        );
        Task::none()
    }

    pub(super) fn set_reset_time_format(&mut self, format: ResetTimeFormat) -> Task<Message> {
        let previous = self.config.reset_time_format;
        self.write_config(|new_config| {
            new_config.reset_time_format = format;
        });
        tracing::info!(
            process_id = %self.process_info.id,
            previous = ?previous,
            format = ?format,
            "reset time format setting changed"
        );
        Task::none()
    }

    pub(super) fn set_usage_amount_format(&mut self, format: UsageAmountFormat) -> Task<Message> {
        let previous = self.config.usage_amount_format;
        self.write_config(|new_config| {
            new_config.usage_amount_format = format;
        });
        tracing::info!(
            process_id = %self.process_info.id,
            previous = ?previous,
            format = ?format,
            "usage amount format setting changed"
        );
        Task::none()
    }

    pub(super) fn set_panel_icon_style(&mut self, style: PanelIconStyle) -> Task<Message> {
        let previous = self.config.panel_icon_style;
        self.write_config(|new_config| {
            new_config.panel_icon_style = style;
        });
        tracing::info!(
            process_id = %self.process_info.id,
            previous = ?previous,
            style = ?style,
            "panel icon style setting changed"
        );
        self.sync_panel_suggested_bounds();
        Task::none()
    }

    pub(super) fn on_host_cli_auth_changed(&mut self) {
        if demo_env::is_active() {
            return;
        }
        tracing::info!(
            process_id = %self.process_info.id,
            owner_status = self.owner_status(),
            "host CLI auth change detected"
        );
        let detection = crate::detection::startup_snapshot(crate::config::host_user_home_dir());
        let detection_changed = detection != self.detection;
        if detection_changed {
            tracing::info!(
                process_id = %self.process_info.id,
                detected_providers = ?detection.detected_providers(),
                "provider detection changed"
            );
            self.detection = detection;
        }

        let previous_state = self.state.clone();
        runtime::reconcile_state(&self.config, &self.detection, &mut self.state);
        if !detection_changed && self.state == previous_state {
            tracing::info!(
                process_id = %self.process_info.id,
                owner_status = self.owner_status(),
                "host CLI auth change did not affect provider state"
            );
            return;
        }
        self.selected_provider = select_provider(self.config.selected_provider, &self.state);
        tracing::info!(
            process_id = %self.process_info.id,
            owner_status = self.owner_status(),
            "host CLI auth change updated provider state"
        );
        self.persist_runtime_if_owner("host_cli_auth_changed");
        self.sync_panel_suggested_bounds();
    }

    pub(super) fn on_config_update(&mut self, update: Config, keys: &[&str]) {
        let mut config = self.config.clone();
        config.apply_watcher_update(update, keys);
        demo_env::apply_config(&mut config);
        if config == self.config {
            return;
        }
        tracing::info!(
            process_id = %self.process_info.id,
            owner_status = self.owner_status(),
            changed_keys = %keys.join(","),
            selected_provider = config.selected_provider.label(),
            enabled_provider_count = enabled_provider_count(&self.state),
            selected_account_count = selected_account_count(&config),
            managed_account_count = managed_account_count(&config),
            "config watcher update applied"
        );
        self.config = config;
        runtime::reconcile_state(&self.config, &self.detection, &mut self.state);
        demo_env::apply(&self.config, &mut self.state);
        self.selected_provider = select_provider(self.config.selected_provider, &self.state);
        self.persist_runtime_if_owner("external_config_update");
        self.sync_panel_suggested_bounds();
    }

    pub(super) fn on_shared_runtime_update(
        &mut self,
        shared_runtime: crate::shared_state::SharedRuntimeState,
    ) {
        let mut next_state = shared_runtime.app_state;
        runtime::reconcile_shared_state(&self.config, &self.detection, &mut next_state);
        demo_env::apply(&self.config, &mut next_state);
        if self.state == next_state {
            return;
        }
        let provider_statuses = shared_state::runtime_provider_status_summary(&next_state);
        let refreshing_providers = shared_state::refreshing_provider_labels(&next_state);
        tracing::info!(
            process_id = %self.process_info.id,
            owner_status = self.owner_status(),
            generation = shared_runtime.generation,
            account_count = next_state.provider_accounts.len(),
            provider_statuses = %provider_statuses,
            refreshing_providers = refreshing_providers.as_str(),
            "shared runtime observed"
        );
        self.state = next_state;
        self.selected_provider = select_provider(self.config.selected_provider, &self.state);
        self.sync_panel_suggested_bounds();
    }

    pub(super) fn toggle_account_selection(
        &mut self,
        provider: ProviderId,
        account_id: &str,
    ) -> Task<Message> {
        let was_selected = self
            .config
            .selected_account_ids(provider)
            .iter()
            .any(|id| id == account_id);
        self.write_config(|new_config| {
            registry::toggle_account_selection(provider, new_config, account_id);
        });
        runtime::reconcile_provider(&self.config, &self.detection, &mut self.state, provider);
        demo_env::apply(&self.config, &mut self.state);
        if provider == self.selected_provider {
            self.detail_account_page = self
                .state
                .accounts_for(provider)
                .iter()
                .position(|account| account.account_id == account_id)
                .unwrap_or(0);
        }
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
        tracing::info!(
            process_id = %self.process_info.id,
            provider = provider.label(),
            account_id,
            previous_selected = was_selected,
            selected = is_selected,
            selected_account_count = self.config.selected_account_ids(provider).len(),
            "account selection changed"
        );
        self.sync_panel_suggested_bounds();
        if is_selected {
            self.request_provider_refresh(provider, RefreshRequestReason::AccountAction)
        } else {
            self.persist_runtime_if_owner("account_selection_changed");
            Task::none()
        }
    }

    pub(super) fn delete_account(
        &mut self,
        provider: ProviderId,
        account_id: &str,
    ) -> Task<Message> {
        super::session::delete_account(self, provider, account_id)
    }

    pub(super) fn persist_runtime_if_owner(&self, reason: &'static str) {
        if self.refresh_owner.is_some() {
            runtime::persist_state_as(&self.state, reason, Some(self.shared_state_writer()));
        }
    }
}

pub(super) fn popup_route_label(route: PopupRoute) -> &'static str {
    match route {
        PopupRoute::ProviderDetail => "provider_detail",
        PopupRoute::Settings => "settings",
        PopupRoute::ManageProviders => "manage_providers",
        PopupRoute::ManageAccounts(provider) => match provider {
            ProviderId::Codex => "manage_accounts_codex",
            ProviderId::Claude => "manage_accounts_claude",
            ProviderId::Cursor => "manage_accounts_cursor",
            ProviderId::Gemini => "manage_accounts_gemini",
            ProviderId::Copilot => "manage_accounts_copilot",
            ProviderId::Minimax => "manage_accounts_minimax",
            ProviderId::Kimi => "manage_accounts_kimi",
            ProviderId::Antigravity => "manage_accounts_antigravity",
            ProviderId::OpenCodeGo => "manage_accounts_opencode_go",
            ProviderId::Grok => "manage_accounts_grok",
        },
        PopupRoute::About => "about",
    }
}

pub(super) fn popup_route_provider_label(
    route: PopupRoute,
    selected_provider: ProviderId,
) -> &'static str {
    match route {
        PopupRoute::ProviderDetail => selected_provider.label(),
        PopupRoute::Settings | PopupRoute::ManageProviders | PopupRoute::About => "none",
        PopupRoute::ManageAccounts(provider) => provider.label(),
    }
}

fn enabled_provider_count(state: &crate::model::AppState) -> usize {
    state
        .providers
        .iter()
        .filter(|provider| provider.enabled)
        .count()
}

fn selected_account_count(config: &Config) -> usize {
    ProviderId::ALL
        .into_iter()
        .map(|provider| config.selected_account_ids(provider).len())
        .sum()
}

fn managed_account_count(config: &Config) -> usize {
    config.codex_managed_accounts.len()
        + config.claude_managed_accounts.len()
        + config.cursor_managed_accounts.len()
        + config.gemini_managed_accounts.len()
        + config.copilot_managed_accounts.len()
        + config.minimax_managed_accounts.len()
        + config.kimi_managed_accounts.len()
        + config.antigravity_managed_accounts.len()
        + config.opencode_go_managed_accounts.len()
        + config.grok_managed_accounts.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grok_popup_route_label_matches_expected() {
        assert_eq!(
            popup_route_label(PopupRoute::ManageAccounts(ProviderId::Grok)),
            "manage_accounts_grok"
        );
    }
}
