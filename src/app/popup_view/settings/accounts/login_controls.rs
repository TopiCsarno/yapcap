use super::super::super::{
    Alignment, AntigravityLoginState, AntigravityLoginStatus, ClaudeLoginState, ClaudeLoginStatus,
    CodexLoginState, CodexLoginStatus, CopilotLoginState, CopilotLoginStatus, CursorScanState,
    Element, GeminiLoginState, GeminiLoginStatus, Length, Message, account_add_button,
    account_import_button, fl, row, widget,
};
use crate::app::login::{KimiLoginFlow, LoginFlow, MinimaxLoginFlow, OpenCodeGoLoginFlow};
use crate::providers::grok::{GrokLoginState, GrokLoginStatus};
use crate::providers::kimi::login::{KimiLoginEvent, KimiLoginState, KimiLoginStatus};
use crate::providers::minimax::{MinimaxLoginEvent, MinimaxLoginState, MinimaxLoginStatus};
use crate::providers::opencode_go::login::{
    OpenCodeGoLoginEvent, OpenCodeGoLoginState, OpenCodeGoLoginStatus,
};

fn minimax_login_message(event: MinimaxLoginEvent) -> Message {
    MinimaxLoginFlow::wrap_event(event)
}

fn kimi_login_message(event: KimiLoginEvent) -> Message {
    KimiLoginFlow::wrap_event(event)
}

fn opencode_go_login_message(event: OpenCodeGoLoginEvent) -> Message {
    OpenCodeGoLoginFlow::wrap_event(event)
}

pub(super) fn codex_login_controls(
    login: Option<&CodexLoginState>,
    opencode_import_available: bool,
    enabled: bool,
) -> Element<'_, Message> {
    let Some(login) = login else {
        let mut controls = cosmic::iced::widget::column![account_add_button(
            fl!("account-add"),
            enabled.then_some(Message::StartLogin(crate::model::ProviderId::Codex)),
        )]
        .spacing(8)
        .width(Length::Fill);
        if opencode_import_available {
            controls = controls.push(account_import_button(
                fl!("import-from-opencode"),
                enabled.then_some(Message::ImportFromOpenCode(
                    crate::model::ProviderId::Codex,
                    None,
                )),
            ));
        }
        return controls.into();
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
        content = content.push(widget::button::text(fl!("account-cancel")).on_press_maybe(
            enabled.then_some(Message::CancelLogin(crate::model::ProviderId::Codex)),
        ));
    } else {
        let mut controls = cosmic::iced::widget::column![account_add_button(
            fl!("account-add"),
            enabled.then_some(Message::StartLogin(crate::model::ProviderId::Codex)),
        )]
        .spacing(8)
        .width(Length::Fill);
        if opencode_import_available {
            controls = controls.push(account_import_button(
                fl!("import-from-opencode"),
                enabled.then_some(Message::ImportFromOpenCode(
                    crate::model::ProviderId::Codex,
                    None,
                )),
            ));
        }
        content = content.push(
            controls
                .push(widget::button::text(fl!("account-dismiss")).on_press_maybe(
                    enabled.then_some(Message::CancelLogin(crate::model::ProviderId::Codex)),
                ))
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
        return account_add_button(
            fl!("account-add"),
            enabled.then_some(Message::StartLogin(crate::model::ProviderId::Claude)),
        );
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
        content = content.push(widget::button::text(fl!("account-cancel")).on_press_maybe(
            enabled.then_some(Message::CancelLogin(crate::model::ProviderId::Claude)),
        ));
    } else {
        content = content.push(
            row![
                widget::button::text(fl!("account-add-another")).on_press_maybe(
                    enabled.then_some(Message::StartLogin(crate::model::ProviderId::Claude))
                ),
                widget::button::text(fl!("account-dismiss")).on_press_maybe(
                    enabled.then_some(Message::CancelLogin(crate::model::ProviderId::Claude))
                ),
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
        return account_add_button(
            fl!("account-add"),
            enabled.then_some(Message::StartLogin(crate::model::ProviderId::Gemini)),
        );
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
        content = content.push(widget::button::text(fl!("account-cancel")).on_press_maybe(
            enabled.then_some(Message::CancelLogin(crate::model::ProviderId::Gemini)),
        ));
    } else {
        content = content.push(
            row![
                widget::button::text(fl!("account-add-another")).on_press_maybe(
                    enabled.then_some(Message::StartLogin(crate::model::ProviderId::Gemini))
                ),
                widget::button::text(fl!("account-dismiss")).on_press_maybe(
                    enabled.then_some(Message::CancelLogin(crate::model::ProviderId::Gemini))
                ),
            ]
            .spacing(8),
        );
    }

    Element::from(content)
}

pub(super) fn copilot_login_controls(
    login: Option<&CopilotLoginState>,
    opencode_import_available: bool,
    enabled: bool,
) -> Element<'_, Message> {
    let Some(login) = login else {
        let mut controls = cosmic::iced::widget::column![account_add_button(
            fl!("account-add"),
            enabled.then_some(Message::StartLogin(crate::model::ProviderId::Copilot)),
        )]
        .spacing(8)
        .width(Length::Fill);
        if opencode_import_available {
            controls = controls.push(account_import_button(
                fl!("import-from-opencode"),
                enabled.then_some(Message::ImportFromOpenCode(
                    crate::model::ProviderId::Copilot,
                    None,
                )),
            ));
        }
        return controls.into();
    };

    let mut content =
        cosmic::iced::widget::column![widget::text(copilot_login_status(login)).size(13)]
            .spacing(10)
            .width(Length::Fill);

    if login.status == CopilotLoginStatus::Running && !login.importing_from_opencode {
        content = content.push(widget::text(fl!("account-browser-login-hint")).size(12));
    }

    if login.status == CopilotLoginStatus::Running
        && let Some(code) = &login.user_code
    {
        content = content.push(copilot_user_code_row(code, login.code_copied, enabled));
    }
    if login.status == CopilotLoginStatus::Running
        && let Some(url) = &login.verification_uri
    {
        content = content.push(
            widget::button::standard(fl!("open-browser"))
                .on_press_maybe(enabled.then_some(Message::OpenUrl(url.clone()))),
        );
    }

    if login.status == CopilotLoginStatus::Running {
        content = content.push(widget::button::text(fl!("account-cancel")).on_press_maybe(
            enabled.then_some(Message::CancelLogin(crate::model::ProviderId::Copilot)),
        ));
    } else {
        let mut controls = cosmic::iced::widget::column![account_add_button(
            fl!("account-add-another"),
            enabled.then_some(Message::StartLogin(crate::model::ProviderId::Copilot)),
        )]
        .spacing(8)
        .width(Length::Fill);
        if opencode_import_available {
            controls = controls.push(account_import_button(
                fl!("import-from-opencode"),
                enabled.then_some(Message::ImportFromOpenCode(
                    crate::model::ProviderId::Copilot,
                    None,
                )),
            ));
        }
        content = content.push(controls.push(
            widget::button::text(fl!("account-dismiss")).on_press_maybe(
                enabled.then_some(Message::CancelLogin(crate::model::ProviderId::Copilot)),
            ),
        ));
    }

    Element::from(content)
}

fn copilot_user_code_row<'a>(code: &'a str, copied: bool, enabled: bool) -> Element<'a, Message> {
    let code_text = widget::text(fl!("copilot-login-user-code", code = code)).size(13);

    let copy_icon_handle = widget::icon::from_name("edit-copy-symbolic")
        .icon()
        .into_svg_handle()
        .unwrap_or_else(|| widget::svg::Handle::from_memory(Vec::new()));
    let copy_icon = widget::Svg::new(copy_icon_handle)
        .symbolic(true)
        .class(cosmic::theme::Svg::custom(|theme| widget::svg::Style {
            color: Some(
                theme
                    .cosmic()
                    .background(theme.transparent)
                    .component
                    .on
                    .into(),
            ),
        }))
        .width(Length::Fixed(16.0))
        .height(Length::Fixed(16.0));
    let copy_message = enabled.then(|| Message::CopyCopilotLoginCode(code.to_string()));
    let copy_button = widget::tooltip::tooltip(
        widget::button::custom(copy_icon)
            .padding(4)
            .on_press_maybe(copy_message),
        widget::text(fl!("copilot-login-copy-code-tooltip")).size(12),
        widget::tooltip::Position::Top,
    );

    let mut content = row![code_text, copy_button]
        .spacing(8)
        .align_y(Alignment::Center);
    if copied {
        content = content.push(widget::text(fl!("copilot-login-code-copied")).size(12));
    }
    content.into()
}

fn copilot_login_status(login: &CopilotLoginState) -> String {
    match login.status {
        CopilotLoginStatus::Running if login.importing_from_opencode => fl!("opencode-importing"),
        CopilotLoginStatus::Running => fl!("copilot-login-running"),
        CopilotLoginStatus::Failed => login
            .error
            .clone()
            .unwrap_or_else(|| fl!("copilot-login-failed")),
    }
}

fn gemini_login_status(login: &GeminiLoginState) -> String {
    match login.status {
        GeminiLoginStatus::Running => fl!("gemini-login-running"),
        GeminiLoginStatus::Failed => login
            .error
            .clone()
            .unwrap_or_else(|| fl!("gemini-login-failed")),
    }
}

pub(super) fn antigravity_login_controls(
    login: Option<&AntigravityLoginState>,
    enabled: bool,
) -> Element<'_, Message> {
    let Some(login) = login else {
        return account_add_button(
            fl!("account-add"),
            enabled.then_some(Message::StartLogin(crate::model::ProviderId::Antigravity)),
        );
    };

    let mut content =
        cosmic::iced::widget::column![widget::text(antigravity_login_status(login)).size(13)]
            .spacing(10)
            .width(Length::Fill);

    if login.status == AntigravityLoginStatus::Running
        && let Some(url) = &login.login_url
    {
        content = content.push(
            widget::button::standard(fl!("open-browser"))
                .on_press_maybe(enabled.then_some(Message::OpenUrl(url.clone()))),
        );
    }

    if login.status == AntigravityLoginStatus::Running {
        content = content.push(widget::button::text(fl!("account-cancel")).on_press_maybe(
            enabled.then_some(Message::CancelLogin(crate::model::ProviderId::Antigravity)),
        ));
    } else {
        content = content.push(
            row![
                widget::button::text(fl!("account-add-another")).on_press_maybe(
                    enabled.then_some(Message::StartLogin(crate::model::ProviderId::Antigravity))
                ),
                widget::button::text(fl!("account-dismiss")).on_press_maybe(
                    enabled.then_some(Message::CancelLogin(crate::model::ProviderId::Antigravity))
                ),
            ]
            .spacing(8),
        );
    }

    Element::from(content)
}

fn antigravity_login_status(login: &AntigravityLoginState) -> String {
    match login.status {
        AntigravityLoginStatus::Running => fl!("antigravity-login-running"),
        AntigravityLoginStatus::Failed => login
            .error
            .clone()
            .unwrap_or_else(|| fl!("antigravity-login-failed")),
    }
}

pub(super) fn cursor_scan_controls(scan: &CursorScanState, enabled: bool) -> Element<'_, Message> {
    match scan {
        CursorScanState::Idle => account_add_button(
            fl!("account-add"),
            enabled.then_some(Message::StartCursorScan),
        ),
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
        CodexLoginStatus::Running if login.importing_from_opencode => fl!("opencode-importing"),
        CodexLoginStatus::Running => fl!("codex-login-running"),
        CodexLoginStatus::Failed => login
            .error
            .clone()
            .unwrap_or_else(|| fl!("codex-login-failed")),
    }
}

fn claude_login_status(login: &ClaudeLoginState) -> String {
    match login.status {
        ClaudeLoginStatus::Running => fl!("claude-login-running"),
        ClaudeLoginStatus::Failed => match login.error.as_deref() {
            Some("invalid-code") => fl!("claude-login-code-invalid"),
            Some(msg) => msg.to_string(),
            None => fl!("claude-login-failed"),
        },
    }
}

pub(super) fn minimax_login_controls(
    login: Option<&MinimaxLoginState>,
    enabled: bool,
) -> Element<'_, Message> {
    let Some(login) = login else {
        return account_add_button(
            fl!("account-add"),
            enabled.then_some(Message::StartLogin(crate::model::ProviderId::Minimax)),
        );
    };

    let mut content = if let Some(error) = &login.error {
        cosmic::iced::widget::column![
            widget::text(minimax_login_status(login)).size(13),
            widget::text(error).size(13)
        ]
        .spacing(10)
    } else {
        cosmic::iced::widget::column![widget::text(minimax_login_status(login)).size(13)]
            .spacing(10)
    };

    content = content.width(Length::Fill);

    if login.status == MinimaxLoginStatus::Editing {
        content = content.push(widget::text(fl!("minimax-api-key-placeholder")).size(12));
        content = content.push(
            widget::text_input::secure_input(
                fl!("minimax-api-key-placeholder"),
                &login.api_key,
                Some(minimax_login_message(
                    MinimaxLoginEvent::ApiKeyVisibilityToggled,
                )),
                !login.api_key_visible,
            )
            .on_input(|api_key| minimax_login_message(MinimaxLoginEvent::ApiKeyChanged(api_key)))
            .on_submit(|_| minimax_login_message(MinimaxLoginEvent::Saved))
            .width(Length::Fill),
        );
        if login.api_key_from_opencode {
            content =
                content.push(widget::text(fl!("minimax-api-key-imported-from-opencode")).size(12));
        }
        content = content.push(widget::text(fl!("account-label")).size(12));
        content = content.push(
            widget::text_input(fl!("account-label"), &login.label)
                .on_input(|label| minimax_login_message(MinimaxLoginEvent::LabelChanged(label)))
                .width(Length::Fill),
        );
        content = content.push(
            row![
                widget::button::standard(fl!("account-add")).on_press_maybe(
                    enabled.then_some(minimax_login_message(MinimaxLoginEvent::Saved))
                ),
                widget::button::text(fl!("account-cancel")).on_press_maybe(
                    enabled.then_some(Message::CancelLogin(crate::model::ProviderId::Minimax))
                ),
            ]
            .spacing(8),
        );
    } else {
        content = content.push(
            row![
                widget::button::text(fl!("account-add-another")).on_press_maybe(
                    enabled.then_some(Message::StartLogin(crate::model::ProviderId::Minimax))
                ),
                widget::button::text(fl!("account-dismiss")).on_press_maybe(
                    enabled.then_some(Message::CancelLogin(crate::model::ProviderId::Minimax))
                ),
            ]
            .spacing(8),
        );
    }

    Element::from(content)
}

fn minimax_login_status(login: &MinimaxLoginState) -> String {
    match login.status {
        MinimaxLoginStatus::Editing => fl!("minimax-login-editing"),
        MinimaxLoginStatus::Failed => login
            .error
            .clone()
            .unwrap_or_else(|| fl!("minimax-login-failed")),
    }
}

pub(super) fn kimi_login_controls(
    login: Option<&KimiLoginState>,
    enabled: bool,
) -> Element<'_, Message> {
    let Some(login) = login else {
        return account_add_button(
            fl!("account-add"),
            enabled.then_some(Message::StartLogin(crate::model::ProviderId::Kimi)),
        );
    };

    let mut content = if let Some(error) = &login.error {
        cosmic::iced::widget::column![
            widget::text(kimi_login_status(login)).size(13),
            widget::text(error).size(13)
        ]
        .spacing(10)
    } else {
        cosmic::iced::widget::column![widget::text(kimi_login_status(login)).size(13)].spacing(10)
    };

    content = content.width(Length::Fill);

    if login.status == KimiLoginStatus::Editing {
        content = content.push(widget::text(fl!("kimi-api-key-placeholder")).size(12));
        content = content.push(
            widget::text_input::secure_input(
                fl!("kimi-api-key-placeholder"),
                &login.api_key,
                Some(kimi_login_message(KimiLoginEvent::ApiKeyVisibilityToggled)),
                !login.api_key_visible,
            )
            .on_input(|api_key| kimi_login_message(KimiLoginEvent::ApiKeyChanged(api_key)))
            .on_submit(|_| kimi_login_message(KimiLoginEvent::Saved))
            .width(Length::Fill),
        );
        if login.api_key_from_opencode {
            content =
                content.push(widget::text(fl!("kimi-api-key-imported-from-opencode")).size(12));
        }
        content = content.push(widget::text(fl!("account-label")).size(12));
        content = content.push(
            widget::text_input(fl!("account-label"), &login.label)
                .on_input(|label| kimi_login_message(KimiLoginEvent::LabelChanged(label)))
                .width(Length::Fill),
        );
        content = content.push(
            row![
                widget::button::standard(fl!("account-add"))
                    .on_press_maybe(enabled.then_some(kimi_login_message(KimiLoginEvent::Saved))),
                widget::button::text(fl!("account-cancel")).on_press_maybe(
                    enabled.then_some(Message::CancelLogin(crate::model::ProviderId::Kimi))
                ),
            ]
            .spacing(8),
        );
    } else {
        content = content.push(
            row![
                widget::button::text(fl!("account-add-another")).on_press_maybe(
                    enabled.then_some(Message::StartLogin(crate::model::ProviderId::Kimi))
                ),
                widget::button::text(fl!("account-dismiss")).on_press_maybe(
                    enabled.then_some(Message::CancelLogin(crate::model::ProviderId::Kimi))
                ),
            ]
            .spacing(8),
        );
    }

    Element::from(content)
}

fn kimi_login_status(login: &KimiLoginState) -> String {
    match login.status {
        KimiLoginStatus::Editing => fl!("kimi-login-editing"),
        KimiLoginStatus::Failed => login
            .error
            .clone()
            .unwrap_or_else(|| fl!("kimi-login-failed")),
    }
}

pub(super) fn opencode_go_login_controls(
    login: Option<&OpenCodeGoLoginState>,
    enabled: bool,
) -> Element<'_, Message> {
    let Some(login) = login else {
        return account_add_button(
            fl!("account-add"),
            enabled.then_some(Message::StartLogin(crate::model::ProviderId::OpenCodeGo)),
        );
    };

    let mut content = if let Some(error) = &login.error {
        cosmic::iced::widget::column![
            widget::text(opencode_go_login_status(login)).size(13),
            widget::text(error).size(13)
        ]
        .spacing(10)
    } else {
        cosmic::iced::widget::column![widget::text(opencode_go_login_status(login)).size(13)]
            .spacing(10)
    };

    content = content.width(Length::Fill);

    if login.status == OpenCodeGoLoginStatus::Editing {
        content = content.push(widget::text(fl!("opencode-go-api-key-placeholder")).size(12));
        content = content.push(
            widget::text_input::secure_input(
                fl!("opencode-go-api-key-placeholder"),
                &login.api_key,
                Some(opencode_go_login_message(
                    OpenCodeGoLoginEvent::ApiKeyVisibilityToggled,
                )),
                !login.api_key_visible,
            )
            .on_input(|api_key| {
                opencode_go_login_message(OpenCodeGoLoginEvent::ApiKeyChanged(api_key))
            })
            .on_submit(|_| opencode_go_login_message(OpenCodeGoLoginEvent::Saved))
            .width(Length::Fill),
        );
        if login.api_key_from_opencode {
            content = content
                .push(widget::text(fl!("opencode-go-api-key-imported-from-opencode")).size(12));
        }
        content = content.push(widget::text(fl!("account-label")).size(12));
        content = content.push(
            widget::text_input(fl!("account-label"), &login.label)
                .on_input(|label| {
                    opencode_go_login_message(OpenCodeGoLoginEvent::LabelChanged(label))
                })
                .width(Length::Fill),
        );
        content = content.push(
            row![
                widget::button::standard(fl!("account-add")).on_press_maybe(
                    enabled.then_some(opencode_go_login_message(OpenCodeGoLoginEvent::Saved))
                ),
                widget::button::text(fl!("account-cancel")).on_press_maybe(
                    enabled.then_some(Message::CancelLogin(crate::model::ProviderId::OpenCodeGo))
                ),
            ]
            .spacing(8),
        );
    } else {
        content = content.push(
            row![
                widget::button::text(fl!("account-add-another")).on_press_maybe(
                    enabled.then_some(Message::StartLogin(crate::model::ProviderId::OpenCodeGo))
                ),
                widget::button::text(fl!("account-dismiss")).on_press_maybe(
                    enabled.then_some(Message::CancelLogin(crate::model::ProviderId::OpenCodeGo))
                ),
            ]
            .spacing(8),
        );
    }

    Element::from(content)
}

fn opencode_go_login_status(login: &OpenCodeGoLoginState) -> String {
    match login.status {
        OpenCodeGoLoginStatus::Editing => fl!("opencode-go-login-editing"),
        OpenCodeGoLoginStatus::Failed => login
            .error
            .clone()
            .unwrap_or_else(|| fl!("opencode-go-login-failed")),
    }
}

pub(super) fn grok_login_controls(
    login: Option<&GrokLoginState>,
    host_import_available: bool,
    enabled: bool,
) -> Element<'_, Message> {
    let Some(login) = login else {
        let mut controls = cosmic::iced::widget::column![account_add_button(
            fl!("account-add"),
            enabled.then_some(Message::StartLogin(crate::model::ProviderId::Grok)),
        )]
        .spacing(8)
        .width(Length::Fill);
        if host_import_available {
            controls = controls.push(account_import_button(
                fl!("import-from-grok"),
                enabled.then_some(Message::ImportFromGrok(None)),
            ));
        }
        return controls.into();
    };

    let mut content =
        cosmic::iced::widget::column![widget::text(grok_login_status(login)).size(13)]
            .spacing(10)
            .width(Length::Fill);

    if login.status == GrokLoginStatus::Running && !login.importing_from_host_cli {
        content = content.push(widget::text(fl!("account-browser-login-hint")).size(12));
    }

    if login.status == GrokLoginStatus::Running
        && let Some(url) = &login.login_url
    {
        content = content.push(
            widget::button::standard(fl!("open-browser"))
                .on_press_maybe(enabled.then_some(Message::OpenUrl(url.clone()))),
        );
    }

    if login.status == GrokLoginStatus::Running {
        content = content.push(widget::button::text(fl!("account-cancel")).on_press_maybe(
            enabled.then_some(Message::CancelLogin(crate::model::ProviderId::Grok)),
        ));
    } else {
        content = content.push(
            row![
                widget::button::text(fl!("account-add-another")).on_press_maybe(
                    enabled.then_some(Message::StartLogin(crate::model::ProviderId::Grok))
                ),
                widget::button::text(fl!("account-dismiss")).on_press_maybe(
                    enabled.then_some(Message::CancelLogin(crate::model::ProviderId::Grok))
                ),
            ]
            .spacing(8),
        );
    }

    Element::from(content)
}

fn grok_login_status(login: &GrokLoginState) -> String {
    match login.status {
        GrokLoginStatus::Running => fl!("grok-login-running"),
        GrokLoginStatus::Failed => login
            .error
            .clone()
            .unwrap_or_else(|| fl!("grok-login-failed")),
    }
}
