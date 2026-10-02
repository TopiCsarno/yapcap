// SPDX-License-Identifier: MPL-2.0

use crate::app::codex_resets::{PendingReset, ResetPhase};
use crate::app::{Message, PopupRoute};
use crate::fl;
use cosmic::Element;
use cosmic::iced::Length;
use cosmic::iced::widget::{column, row};
use cosmic::widget;

pub(super) fn codex_reset_view(pending: Option<&PendingReset>) -> Element<'static, Message> {
    let Some(pending) = pending else {
        return widget::text(fl!("codex-reset-unavailable")).into();
    };
    let mut content = column![
        widget::text(fl!("codex-reset-confirm-title")).size(18),
        widget::text(pending.account_label.clone()).size(14),
    ]
    .spacing(12)
    .width(Length::Fill);
    let detail = match &pending.phase {
        ResetPhase::Checking => fl!("codex-reset-checking"),
        ResetPhase::Confirm => {
            let option = pending.option.as_ref().unwrap();
            let count = i64::try_from(option.available_count).unwrap_or(i64::MAX);
            let mut text = fl!("codex-reset-confirm-detail", count = count);
            if let Some(expiry) = option.expires_at {
                let date = expiry.format("%Y-%m-%d %H:%M UTC").to_string();
                text.push(' ');
                text.push_str(&fl!("codex-reset-popup-expiry", date = date.as_str()));
            }
            text
        }
        ResetPhase::Submitting => fl!("codex-reset-submitting"),
        ResetPhase::Success => fl!("codex-reset-success"),
        ResetPhase::NothingToReset => fl!("codex-reset-nothing-to-reset"),
        ResetPhase::NoCredit => fl!("codex-reset-no-credit"),
        ResetPhase::Failed(_) => fl!("codex-reset-failed"),
    };
    content = content.push(widget::text(detail).size(14));
    if let ResetPhase::Failed(reason) = pending.phase {
        content = content.push(widget::text(reason).size(12));
    }
    if matches!(pending.phase, ResetPhase::Confirm) {
        content = content.push(widget::text(fl!("codex-reset-confirm-warning")).size(13));
    }
    let mut actions = row![].spacing(10);
    match pending.phase {
        ResetPhase::Confirm => {
            actions = actions.push(
                widget::button::suggested(fl!("codex-reset-confirm-action"))
                    .on_press(Message::ConfirmCodexReset),
            );
        }
        ResetPhase::Failed(_) if pending.option.is_some() => {
            actions = actions.push(
                widget::button::standard(fl!("codex-reset-retry"))
                    .on_press(Message::ConfirmCodexReset),
            );
        }
        ResetPhase::Failed(_) => {
            actions = actions.push(
                widget::button::standard(fl!("codex-reset-retry"))
                    .on_press(Message::PrepareCodexReset(pending.account_id.clone())),
            );
        }
        _ => {}
    }
    if matches!(pending.phase, ResetPhase::Success) {
        actions = actions.push(
            widget::button::suggested(fl!("codex-reset-done"))
                .on_press(Message::NavigateTo(PopupRoute::ProviderDetail)),
        );
    } else if !matches!(pending.phase, ResetPhase::Submitting) {
        let label = if matches!(pending.phase, ResetPhase::Confirm) {
            fl!("account-cancel")
        } else {
            fl!("codex-reset-done")
        };
        actions = actions.push(
            widget::button::text(label).on_press(Message::NavigateTo(PopupRoute::ProviderDetail)),
        );
    }
    content.push(actions).into()
}
