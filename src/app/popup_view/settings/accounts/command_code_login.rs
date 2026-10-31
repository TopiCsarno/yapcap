// SPDX-License-Identifier: MPL-2.0

use crate::app::Message;
use crate::app::login::{CommandCodeLoginFlow, LoginFlow};
use crate::fl;
use crate::model::ProviderId;
use crate::providers::command_code::login::{
    CommandCodeLoginEvent as Event, CommandCodeLoginState as State,
    CommandCodeLoginStatus as Status,
};
use cosmic::{
    Element,
    iced::{
        Length,
        widget::{column, row},
    },
    widget,
};

fn message(event: Event) -> Message {
    CommandCodeLoginFlow::wrap_event(event)
}

pub(super) fn controls(login: Option<&State>, enabled: bool) -> Element<'_, Message> {
    let Some(login) = login else {
        return super::super::super::account_add_button(
            fl!("account-add"),
            enabled.then_some(Message::StartLogin(ProviderId::CommandCode)),
        );
    };
    let mut content = column![].spacing(10).width(Length::Fill);
    if let Some(error) = &login.error {
        content = content.push(widget::text(error).size(13));
    }
    if login.status == Status::Running {
        content = content.push(widget::text(fl!("command-code-login-running")).size(13));
        if let Some(url) = &login.login_url {
            content = content.push(
                widget::button::standard(fl!("open-browser"))
                    .on_press_maybe(enabled.then_some(Message::OpenUrl(url.clone()))),
            );
        }
    } else {
        content = content.push(
            widget::button::suggested(fl!("command-code-browser-login"))
                .on_press_maybe(enabled.then_some(message(Event::BrowserRequested))),
        );
        content = content.push(
            widget::button::standard(fl!("command-code-import-cli"))
                .on_press_maybe(enabled.then_some(message(Event::ImportCliRequested))),
        );
        content = content.push(widget::text(fl!("command-code-api-key-hint")).size(12));
        content = content.push(
            widget::text_input::secure_input(
                fl!("command-code-api-key"),
                &login.api_key,
                Some(message(Event::ApiKeyVisibilityToggled)),
                !login.api_key_visible,
            )
            .on_input(|key| message(Event::ApiKeyChanged(key)))
            .on_submit(|_| message(Event::Saved))
            .width(Length::Fill),
        );
        content = content.push(
            widget::text_input(fl!("account-label"), &login.label)
                .on_input(|label| message(Event::LabelChanged(label)))
                .width(Length::Fill),
        );
        content = content.push(
            widget::button::standard(fl!("command-code-save-key")).on_press_maybe(
                (enabled && !login.api_key.trim().is_empty()).then_some(message(Event::Saved)),
            ),
        );
    }
    content
        .push(row![
            widget::button::text(fl!("account-cancel"))
                .on_press_maybe(enabled.then_some(Message::CancelLogin(ProviderId::CommandCode)))
        ])
        .into()
}
