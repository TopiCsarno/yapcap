use super::super::super::{
    ClaudeLoginState, ClaudeLoginStatus, CodexLoginState, CodexLoginStatus, CursorScanState,
    Element, GeminiLoginState, GeminiLoginStatus, Length, Message, fl, row, widget,
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
        content = content.push(
            widget::text_input(fl!("claude-login-code-placeholder"), &login.code_input)
                .on_input(Message::UpdateClaudeLoginCode)
                .on_submit(|_| Message::SubmitClaudeLoginCode)
                .width(Length::Fill),
        );
        content = content.push(
            widget::button::standard(fl!("claude-login-submit-code")).on_press_maybe(
                (enabled && !login.code_input.trim().is_empty())
                    .then_some(Message::SubmitClaudeLoginCode),
            ),
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

pub(super) fn gemini_login_controls(
    login: Option<&GeminiLoginState>,
    enabled: bool,
) -> Element<'_, Message> {
    let Some(login) = login else {
        return widget::button::standard(fl!("account-add"))
            .on_press_maybe(enabled.then_some(Message::StartGeminiLogin))
            .into();
    };

    let mut content =
        cosmic::iced::widget::column![widget::text(gemini_login_status(login)).size(13)]
            .spacing(10)
            .width(Length::Fill);

    if login.status == GeminiLoginStatus::Running
        && let Some(url) = &login.login_url
    {
        content = content.push(
            widget::button::standard(fl!("open-browser"))
                .on_press_maybe(enabled.then_some(Message::OpenUrl(url.clone()))),
        );
    }

    if login.status == GeminiLoginStatus::Running {
        content = content.push(
            widget::button::text(fl!("account-cancel"))
                .on_press_maybe(enabled.then_some(Message::CancelGeminiLogin)),
        );
    } else {
        content = content.push(
            row![
                widget::button::text(fl!("account-add-another"))
                    .on_press_maybe(enabled.then_some(Message::StartGeminiLogin)),
                widget::button::text(fl!("account-dismiss"))
                    .on_press_maybe(enabled.then_some(Message::CancelGeminiLogin)),
            ]
            .spacing(8),
        );
    }

    Element::from(content)
}

fn gemini_login_status(login: &GeminiLoginState) -> String {
    match login.status {
        GeminiLoginStatus::Running => fl!("gemini-login-running"),
        GeminiLoginStatus::Succeeded => fl!("gemini-login-succeeded"),
        GeminiLoginStatus::Failed => login
            .error
            .clone()
            .unwrap_or_else(|| fl!("gemini-login-failed")),
    }
}

pub(super) fn cursor_scan_controls(scan: &CursorScanState, enabled: bool) -> Element<'_, Message> {
    match scan {
        CursorScanState::Idle => {
            let mut content = cosmic::iced::widget::column![]
                .spacing(6)
                .width(Length::Fill);
            content = content.push(
                widget::button::standard(fl!("cursor-scan-button"))
                    .on_press_maybe(enabled.then_some(Message::StartCursorScan)),
            );
            content = content.push(
                widget::text(fl!("cursor-scan-subtitle"))
                    .size(12)
                    .width(Length::Fill),
            );
            Element::from(content)
        }
        CursorScanState::Scanning => Element::from(
            cosmic::iced::widget::column![widget::text(fl!("cursor-scanning")).size(13)]
                .spacing(10)
                .width(Length::Fill),
        ),
        CursorScanState::Found { email, plan } => {
            let status_text = match plan.as_deref() {
                Some(plan) => fl!(
                    "cursor-scan-found-plan",
                    email = email.as_str(),
                    plan = plan
                ),
                None => fl!("cursor-scan-found", email = email.as_str()),
            };
            let mut content = cosmic::iced::widget::column![widget::text(status_text).size(13)]
                .spacing(10)
                .width(Length::Fill);
            content = content.push(
                row![
                    widget::button::standard(fl!("cursor-scan-connect"))
                        .on_press_maybe(enabled.then_some(Message::ConfirmCursorScan)),
                    widget::button::text(fl!("account-cancel"))
                        .on_press_maybe(enabled.then_some(Message::DismissCursorScan)),
                ]
                .spacing(8),
            );
            Element::from(content)
        }
        CursorScanState::AlreadyConnected { email } => {
            let status_text = fl!("cursor-scan-already-connected", email = email.as_str());
            let mut content = cosmic::iced::widget::column![widget::text(status_text).size(13)]
                .spacing(10)
                .width(Length::Fill);
            content = content.push(
                row![
                    widget::button::standard(fl!("cursor-scan-reconnect"))
                        .on_press_maybe(enabled.then_some(Message::ConfirmCursorScan)),
                    widget::button::text(fl!("account-cancel"))
                        .on_press_maybe(enabled.then_some(Message::DismissCursorScan)),
                ]
                .spacing(8),
            );
            Element::from(content)
        }
        CursorScanState::Error(message) => {
            let mut content = cosmic::iced::widget::column![widget::text(message).size(13)]
                .spacing(10)
                .width(Length::Fill);
            content = content.push(
                widget::button::standard(fl!("cursor-scan-try-again"))
                    .on_press_maybe(enabled.then_some(Message::DismissCursorScan)),
            );
            Element::from(content)
        }
    }
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
        ClaudeLoginStatus::Failed => match login.error.as_deref() {
            Some("invalid-code") => fl!("claude-login-code-invalid"),
            Some(msg) => msg.to_string(),
            None => fl!("claude-login-failed"),
        },
    }
}
