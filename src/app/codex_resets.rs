// SPDX-License-Identifier: MPL-2.0

use super::{AppModel, Message, PopupRoute, ProviderId, Task};
use crate::providers::codex::reset_credits::{self, ResetOption, ResetOutcome};
use crate::shared_state::RefreshRequestReason;
use std::time::Duration;

const SUCCESS_DISPLAY_TIME: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub(crate) enum ResetPhase {
    Checking,
    Confirm,
    Submitting,
    Success,
    NothingToReset,
    NoCredit,
    Failed(&'static str),
}

#[derive(Debug, Clone)]
pub(crate) struct PendingReset {
    pub account_id: String,
    pub account_label: String,
    pub option: Option<ResetOption>,
    pub phase: ResetPhase,
    request_id: Option<String>,
}

impl AppModel {
    pub(super) fn prepare_codex_reset(&mut self, account_id: &str) -> Task<Message> {
        if self.selected_provider != ProviderId::Codex || self.popup.is_none() {
            return Task::none();
        }
        let Some(account) = self
            .state
            .accounts_for(ProviderId::Codex)
            .into_iter()
            .find(|account| account.account_id == account_id)
        else {
            return Task::none();
        };
        self.codex_reset = Some(PendingReset {
            account_id: account_id.to_string(),
            account_label: account.label.clone(),
            option: None,
            phase: ResetPhase::Checking,
            request_id: None,
        });
        self.popup_route = PopupRoute::CodexReset;
        let requested = account_id.to_string();
        let checked = requested.clone();
        Task::perform(
            async move { reset_credits::prepare(&checked).await },
            move |result| cosmic::Action::App(Message::CodexResetPrepared(requested, result)),
        )
    }

    pub(super) fn codex_reset_prepared(
        &mut self,
        account_id: &str,
        result: Result<ResetOption, &'static str>,
    ) {
        let Some(pending) = self.codex_reset.as_mut() else {
            return;
        };
        if pending.account_id != account_id
            || self.popup_route != PopupRoute::CodexReset
            || !matches!(pending.phase, ResetPhase::Checking)
        {
            return;
        }
        match result {
            Ok(option) => {
                pending.option = Some(option);
                pending.phase = ResetPhase::Confirm;
            }
            Err(reason) => pending.phase = ResetPhase::Failed(reason),
        }
    }

    pub(super) fn confirm_codex_reset(&mut self) -> Task<Message> {
        if self.popup_route != PopupRoute::CodexReset || self.popup.is_none() {
            return Task::none();
        }
        let Some(pending) = self.codex_reset.as_mut() else {
            return Task::none();
        };
        if !matches!(pending.phase, ResetPhase::Confirm | ResetPhase::Failed(_))
            || pending.option.is_none()
            || !self
                .state
                .accounts_for(ProviderId::Codex)
                .iter()
                .any(|account| account.account_id == pending.account_id)
        {
            return Task::none();
        }
        let request_id = pending
            .request_id
            .get_or_insert_with(|| uuid::Uuid::new_v4().to_string())
            .clone();
        let account_id = pending.account_id.clone();
        let credit_id = pending
            .option
            .as_ref()
            .and_then(|option| option.credit_id.clone());
        pending.phase = ResetPhase::Submitting;
        let target = account_id.clone();
        let request = request_id.clone();
        Task::perform(
            async move { reset_credits::consume(&target, credit_id.as_deref(), &request).await },
            move |result| {
                cosmic::Action::App(Message::CodexResetConsumed(account_id, request_id, result))
            },
        )
    }

    pub(super) fn codex_reset_consumed(
        &mut self,
        account_id: &str,
        request_id: &str,
        result: Result<ResetOutcome, &'static str>,
    ) -> Task<Message> {
        let Some(pending) = self.codex_reset.as_mut() else {
            return Task::none();
        };
        if pending.account_id != account_id
            || pending.request_id.as_deref() != Some(request_id)
            || !matches!(pending.phase, ResetPhase::Submitting)
        {
            return Task::none();
        }
        pending.phase = match result {
            Ok(ResetOutcome::Reset | ResetOutcome::AlreadyRedeemed) => ResetPhase::Success,
            Ok(ResetOutcome::NothingToReset) => ResetPhase::NothingToReset,
            Ok(ResetOutcome::NoCredit) => ResetPhase::NoCredit,
            Err(reason) => ResetPhase::Failed(reason),
        };
        match pending.phase {
            ResetPhase::Success => {
                let account_id = account_id.to_string();
                let request_id = request_id.to_string();
                let refresh = self.request_provider_refresh(
                    ProviderId::Codex,
                    RefreshRequestReason::AccountAction,
                );
                let dismiss = Task::perform(
                    async move { tokio::time::sleep(SUCCESS_DISPLAY_TIME).await },
                    move |()| {
                        cosmic::Action::App(Message::CodexResetSuccessTimeout(
                            account_id, request_id,
                        ))
                    },
                );
                Task::batch(vec![refresh, dismiss])
            }
            ResetPhase::NoCredit => self
                .request_provider_refresh(ProviderId::Codex, RefreshRequestReason::AccountAction),
            _ => Task::none(),
        }
    }

    pub(super) fn finish_codex_reset_success(&mut self, account_id: &str, request_id: &str) {
        if self.popup_route != PopupRoute::CodexReset || self.popup.is_none() {
            return;
        }
        if let Some(pending) = self.codex_reset.as_ref()
            && pending.account_id == account_id
            && pending.request_id.as_deref() == Some(request_id)
            && matches!(pending.phase, ResetPhase::Success)
        {
            self.popup_route = PopupRoute::ProviderDetail;
            self.codex_reset = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ProviderAccountRuntimeState;
    use cosmic::iced::window::Id;

    #[test]
    fn confirmation_is_scoped_to_one_account_and_retries_with_same_key() {
        let mut app = crate::app::tests::test_app(None);
        let account = "codex-test";
        app.state.upsert_account(ProviderAccountRuntimeState::empty(
            ProviderId::Codex,
            account.to_string(),
            "Test account".to_string(),
        ));
        app.popup = Some(Id::unique());
        app.popup_route = PopupRoute::CodexReset;
        app.codex_reset = Some(PendingReset {
            account_id: account.to_string(),
            account_label: "Test account".to_string(),
            option: Some(ResetOption {
                credit_id: Some("synthetic-credit".to_string()),
                expires_at: None,
                available_count: 1,
            }),
            phase: ResetPhase::Confirm,
            request_id: None,
        });

        assert_eq!(app.confirm_codex_reset().units(), 1);
        let key = app
            .codex_reset
            .as_ref()
            .unwrap()
            .request_id
            .clone()
            .unwrap();
        assert_eq!(app.confirm_codex_reset().units(), 0);
        assert_eq!(
            app.codex_reset_consumed("wrong-account", &key, Ok(ResetOutcome::Reset))
                .units(),
            0
        );
        assert!(matches!(
            app.codex_reset.as_ref().unwrap().phase,
            ResetPhase::Submitting
        ));
        assert_eq!(
            app.codex_reset_consumed(account, &key, Err("network error"))
                .units(),
            0
        );
        assert!(matches!(
            app.codex_reset.as_ref().unwrap().phase,
            ResetPhase::Failed(_)
        ));
        assert_eq!(app.confirm_codex_reset().units(), 1);
        assert_eq!(
            app.codex_reset.as_ref().unwrap().request_id.as_deref(),
            Some(key.as_str())
        );

        app.popup_route = PopupRoute::ProviderDetail;
        assert_eq!(app.confirm_codex_reset().units(), 0);
    }

    #[test]
    fn no_popup_never_schedules_consumption() {
        let mut app = crate::app::tests::test_app(None);
        assert_eq!(app.confirm_codex_reset().units(), 0);
    }

    #[test]
    fn success_returns_to_the_account_only_for_the_matching_open_dialog() {
        let mut app = crate::app::tests::test_app(None);
        app.popup = Some(Id::unique());
        app.popup_route = PopupRoute::CodexReset;
        app.codex_reset = Some(PendingReset {
            account_id: "first-account".to_string(),
            account_label: "First account".to_string(),
            option: None,
            phase: ResetPhase::Success,
            request_id: Some("first-request".to_string()),
        });

        app.finish_codex_reset_success("another-account", "first-request");
        app.finish_codex_reset_success("first-account", "another-request");
        assert_eq!(app.popup_route, PopupRoute::CodexReset);

        app.popup = None;
        app.finish_codex_reset_success("first-account", "first-request");
        assert_eq!(app.popup_route, PopupRoute::CodexReset);

        app.popup = Some(Id::unique());
        app.finish_codex_reset_success("first-account", "first-request");
        assert_eq!(app.popup_route, PopupRoute::ProviderDetail);
        assert!(app.codex_reset.is_none());
    }

    #[test]
    fn success_timer_never_dismisses_a_non_success_or_a_different_route() {
        let mut app = crate::app::tests::test_app(None);
        app.popup = Some(Id::unique());
        app.popup_route = PopupRoute::CodexReset;
        app.codex_reset = Some(PendingReset {
            account_id: "first-account".to_string(),
            account_label: "First account".to_string(),
            option: None,
            phase: ResetPhase::NothingToReset,
            request_id: Some("first-request".to_string()),
        });
        app.finish_codex_reset_success("first-account", "first-request");
        assert_eq!(app.popup_route, PopupRoute::CodexReset);

        app.codex_reset.as_mut().unwrap().phase = ResetPhase::Success;
        app.popup_route = PopupRoute::ProviderDetail;
        app.finish_codex_reset_success("first-account", "first-request");
        assert_eq!(app.popup_route, PopupRoute::ProviderDetail);
        assert!(app.codex_reset.is_some());
    }
}
