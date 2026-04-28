use super::super::super::{
    ClaudeLoginState, ClaudeLoginStatus, CodexLoginState, CodexLoginStatus, CursorLoginState,
    CursorLoginStatus, Element, Length, Message, fl, row, widget,
};

pub(super) fn codex_login_controls(
    login: Option<&CodexLoginState>,
    enabled: bool,
) -> Element<'_, Message> {
    let Some(login) = login else {
        return widget::button::standard(fl!("account-add"))
            .on_press_maybe(enabled.then_some(Message::StartCodexLogin))
            .into();
    };

    let mut content =
        cosmic::iced::widget::column![widget::text(codex_login_status(login)).size(13)]
            .spacing(10)
            .width(Length::Fill);

    if login.status == CodexLoginStatus::Running
        && let Some(url) = &login.login_url
    {
        content = content.push(
            widget::button::standard(fl!("open-browser"))
                .on_press_maybe(enabled.then_some(Message::OpenUrl(url.clone()))),
        );
    }

    if login.status == CodexLoginStatus::Running {
        content = content.push(
            widget::button::text(fl!("account-cancel"))
                .on_press_maybe(enabled.then_some(Message::CancelCodexLogin)),
        );
    } else {
        content = content.push(
            row![
                widget::button::text(fl!("account-add-another"))
                    .on_press_maybe(enabled.then_some(Message::StartCodexLogin)),
                widget::button::text(fl!("account-dismiss"))
                    .on_press_maybe(enabled.then_some(Message::CancelCodexLogin)),
            ]
            .spacing(8),
        );
    }

    Element::from(content)
}

pub(super) fn claude_login_controls(
    login: Option<&ClaudeLoginState>,
    enabled: bool,
) -> Element<'_, Message> {
    let Some(login) = login else {
        return widget::button::standard(fl!("account-add"))
            .on_press_maybe(enabled.then_some(Message::StartClaudeLogin))
            .into();
    };

    let mut content =
        cosmic::iced::widget::column![widget::text(claude_login_status(login)).size(13)]
            .spacing(10)
            .width(Length::Fill);

    if login.status == ClaudeLoginStatus::Running
        && let Some(url) = &login.login_url
    {
        content = content.push(
            widget::button::standard(fl!("open-browser"))
                .on_press_maybe(enabled.then_some(Message::OpenUrl(url.clone()))),
        );
    }

    if login.status == ClaudeLoginStatus::Running {
        content = content.push(
            widget::button::text(fl!("account-cancel"))
                .on_press_maybe(enabled.then_some(Message::CancelClaudeLogin)),
        );
    } else {
        content = content.push(
            row![
                widget::button::text(fl!("account-add-another"))
                    .on_press_maybe(enabled.then_some(Message::StartClaudeLogin)),
                widget::button::text(fl!("account-dismiss"))
                    .on_press_maybe(enabled.then_some(Message::CancelClaudeLogin)),
            ]
            .spacing(8),
        );
    }

    Element::from(content)
}

pub(super) fn cursor_login_controls(
    login: Option<&CursorLoginState>,
    enabled: bool,
) -> Element<'_, Message> {
    let Some(login) = login else {
        return widget::button::standard(fl!("account-add"))
            .on_press_maybe(enabled.then_some(Message::StartCursorLogin))
            .into();
    };

    let mut content =
        cosmic::iced::widget::column![widget::text(cursor_login_status(login)).size(13)]
            .spacing(10)
            .width(Length::Fill);

    if login.status == CursorLoginStatus::Running {
        content = content.push(
            widget::button::standard(fl!("open-browser"))
                .on_press_maybe(enabled.then_some(Message::OpenUrl(login.login_url.clone()))),
        );
        content = content.push(
            widget::button::text(fl!("account-cancel"))
                .on_press_maybe(enabled.then_some(Message::CancelCursorLogin)),
        );
    } else {
        content = content.push(
            row![
                widget::button::text(fl!("account-add-another"))
                    .on_press_maybe(enabled.then_some(Message::StartCursorLogin)),
                widget::button::text(fl!("account-dismiss"))
                    .on_press_maybe(enabled.then_some(Message::CancelCursorLogin)),
            ]
            .spacing(8),
        );
    }

    Element::from(content)
}

fn codex_login_status(login: &CodexLoginState) -> String {
    match login.status {
        CodexLoginStatus::Running => fl!("codex-login-running"),
        CodexLoginStatus::Succeeded => fl!("codex-login-succeeded"),
        CodexLoginStatus::Failed => login
            .error
            .clone()
            .unwrap_or_else(|| fl!("codex-login-failed")),
    }
}

fn claude_login_status(login: &ClaudeLoginState) -> String {
    match login.status {
        ClaudeLoginStatus::Running => fl!("claude-login-running"),
        ClaudeLoginStatus::Succeeded => fl!("claude-login-succeeded"),
        ClaudeLoginStatus::Failed => login
            .error
            .clone()
            .unwrap_or_else(|| fl!("claude-login-failed")),
    }
}

fn cursor_login_status(login: &CursorLoginState) -> String {
    match login.status {
        CursorLoginStatus::Running => {
            fl!("cursor-login-running", browser = login.browser.label())
        }
        CursorLoginStatus::Succeeded => fl!("cursor-login-succeeded"),
        CursorLoginStatus::Failed => login
            .error
            .clone()
            .unwrap_or_else(|| fl!("cursor-login-failed")),
    }
}
