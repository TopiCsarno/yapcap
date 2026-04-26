// SPDX-License-Identifier: MPL-2.0

use crate::app::{Message, PopupRoute, SettingsRoute};
use crate::config::{Config, PanelIconStyle, ResetTimeFormat, UsageAmountFormat};
use crate::fl;
use crate::model::{
    AccountSelectionStatus, AppState, AuthState, ProviderAccountRuntimeState, ProviderCost,
    ProviderHealth, ProviderId, ProviderRuntimeState, STALE_THRESHOLD, UsageSnapshot, UsageWindow,
};
use crate::provider_assets::{provider_icon_handle, provider_icon_variant};
use crate::providers::claude::{ClaudeLoginState, ClaudeLoginStatus};
use crate::providers::codex::{CodexLoginState, CodexLoginStatus};
use crate::providers::cursor::{CursorLoginState, CursorLoginStatus};
use crate::providers::interface::ProviderAccountActionSupport;
use crate::providers::registry;
use crate::updates::UpdateStatus;
use crate::usage_display;
use chrono::{DateTime, Utc};
use cosmic::Element;
use cosmic::iced::widget::{column, container, progress_bar, row, scrollable};
use cosmic::iced::{Alignment, Background, Color, Length, Size};
use cosmic::widget;

const POPUP_WIDTH: f32 = 420.0;
const POPUP_MAX_HEIGHT: f32 = 1080.0;
const POPUP_PADDING: f32 = 32.0;
const POPUP_CHROME_SPACING: f32 = 42.0;
const POPUP_HEADER_HEIGHT: f32 = 36.0;
const POPUP_TAB_HEIGHT: f32 = 68.0;
const POPUP_FOOTER_HEIGHT: f32 = 28.0;
const PROVIDER_CARD_SPACING: f32 = 8.0;
const PROVIDER_HEIGHT_SECTION_SPACING: f32 = 14.0;
const PROVIDER_SUMMARY_HEIGHT: f32 = 104.0;
const PROVIDER_SECTION_HEIGHT: f32 = 96.0;
const PROVIDER_ACCOUNT_LIST_TITLE_HEIGHT: f32 = 32.0;
const PROVIDER_ACCOUNT_LIST_ROW_HEIGHT: f32 = 92.0;
const PROVIDER_ACCOUNT_LIST_SPACING: f32 = 10.0;
const SETTINGS_SECTION_HEIGHT: f32 = 104.0;
const SETTINGS_PROVIDER_ROW_HEIGHT: f32 = 44.0;
const ACCOUNT_LABEL_MAX_CHARS: usize = 30;
const UPDATE_NOTIFICATION_DOT_COLOR: Color = Color::from_rgb(0.93, 0.11, 0.15);
const ACCENT_SOFT_FILL_ALPHA: f32 = 0.14;

#[derive(Clone, Copy)]
pub struct ProviderLoginStates<'a> {
    pub codex: Option<&'a CodexLoginState>,
    pub claude: Option<&'a ClaudeLoginState>,
    pub cursor: Option<&'a CursorLoginState>,
}

pub fn popup_content<'a>(
    state: &'a AppState,
    config: &'a Config,
    logins: ProviderLoginStates<'a>,
    selected_provider: ProviderId,
    route: &'a PopupRoute,
    update_status: &'a UpdateStatus,
) -> Element<'a, Message> {
    let selected = selected_state(state, selected_provider);

    let header = popup_header(route);

    let nav_row: Element<'_, Message> = match route {
        PopupRoute::ProviderDetail => state
            .providers
            .iter()
            .filter(|provider| provider.enabled)
            .fold(row![].spacing(8), |row, provider| {
                row.push(provider_tab(
                    state,
                    provider,
                    provider.provider == selected_provider,
                ))
            })
            .into(),
        PopupRoute::Settings(settings_route) => {
            container(settings_category_row(settings_route, update_status))
                .height(Length::Fixed(POPUP_TAB_HEIGHT))
                .align_y(Alignment::Center)
                .width(Length::Fill)
                .into()
        }
    };

    let body: Element<'_, Message> = match route {
        PopupRoute::ProviderDetail => selected_provider_view(selected, state, config),
        PopupRoute::Settings(SettingsRoute::General) => {
            general_settings_view(config, update_status)
        }
        PopupRoute::Settings(SettingsRoute::Provider(id)) => {
            provider_settings_view(state, config, logins, *id)
        }
    };

    let footer_action: Element<'_, Message> = match route {
        PopupRoute::ProviderDetail => settings_footer_action(update_status),
        PopupRoute::Settings(_) => widget::button::text(fl!("done"))
            .on_press(Message::NavigateTo(PopupRoute::ProviderDetail))
            .into(),
    };

    let footer = row![
        widget::button::text(fl!("quit")).on_press(Message::Quit),
        cosmic::iced::widget::Space::new().width(Length::Fill),
        footer_action,
    ]
    .align_y(Alignment::Center);

    let body_panel = container(panel(scrollable(body).width(Length::Fill)))
        .width(Length::Fill)
        .height(Length::Fill);

    let content = column![header, nav_row, body_panel, footer]
        .spacing(14)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill);

    Element::from(content)
}

pub fn popup_session_size(state: &AppState) -> Size {
    let provider_height = state
        .providers
        .iter()
        .map(|provider| provider_body_height(state, Some(provider)))
        .fold(PROVIDER_SUMMARY_HEIGHT, f32::max);
    let body_height = provider_height.max(settings_body_height(state));
    let height = POPUP_PADDING
        + POPUP_CHROME_SPACING
        + POPUP_HEADER_HEIGHT
        + POPUP_TAB_HEIGHT
        + POPUP_FOOTER_HEIGHT
        + body_height;

    Size::new(POPUP_WIDTH, height.clamp(1.0, POPUP_MAX_HEIGHT))
}

fn panel<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Element::from(container(content).width(Length::Fill).padding(12))
}

fn popup_header(route: &PopupRoute) -> Element<'static, Message> {
    let mut header = row![
        widget::text(fl!("app-title")).size(22),
        cosmic::iced::widget::Space::new().width(Length::Fill),
    ]
    .align_y(Alignment::Center)
    .spacing(12);

    if matches!(route, PopupRoute::ProviderDetail) {
        header =
            header.push(widget::button::standard(fl!("refresh-now")).on_press(Message::RefreshNow));
    }

    header.into()
}

fn card<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Element::from(container(content).width(Length::Fill).padding(8))
}

fn accent_selection_fill(theme: &cosmic::Theme) -> Color {
    let cosmic = theme.cosmic();
    apply_alpha(cosmic.accent.base.into(), ACCENT_SOFT_FILL_ALPHA)
}

fn settings_block<'a>(
    title: Element<'a, Message>,
    body: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    settings_block_enabled(title, body, true)
}

fn settings_block_enabled<'a>(
    title: Element<'a, Message>,
    body: impl Into<Element<'a, Message>>,
    enabled: bool,
) -> Element<'a, Message> {
    let content = column![title, body.into()].spacing(10).width(Length::Fill);

    let outer = container(content).width(Length::Fill).padding(12);
    if enabled {
        return Element::from(outer);
    }

    Element::from(outer.style(|theme| {
        let cosmic = theme.cosmic();
        widget::container::Style {
            text_color: Some(apply_alpha(cosmic.background.on.into(), 0.45)),
            background: Some(Background::Color(apply_alpha(
                cosmic.background.component.base.into(),
                0.45,
            ))),
            border: cosmic::iced::Border {
                radius: cosmic.corner_radii.radius_s.into(),
                width: 1.0,
                color: apply_alpha(cosmic.background.divider.into(), 0.45),
            },
            shadow: cosmic::iced::Shadow::default(),
            icon_color: Some(apply_alpha(cosmic.background.on.into(), 0.45)),
            snap: true,
        }
    }))
}

fn settings_category_row(
    route: &SettingsRoute,
    update_status: &UpdateStatus,
) -> Element<'static, Message> {
    let row = row![settings_category_tab(
        fl!("settings-general-title"),
        settings_category_icon(&SettingsRoute::General),
        matches!(route, SettingsRoute::General),
        SettingsRoute::General,
        update_available(update_status),
    )]
    .spacing(8)
    .width(Length::Fill);
    let providers = [ProviderId::Codex, ProviderId::Claude, ProviderId::Cursor];
    providers
        .into_iter()
        .fold(row, |row, provider| {
            let target_route = SettingsRoute::Provider(provider);
            row.push(settings_category_tab(
                provider.label().to_string(),
                settings_category_icon(&target_route),
                matches!(route, SettingsRoute::Provider(id) if *id == provider),
                target_route,
                false,
            ))
        })
        .into()
}

fn settings_category_tab(
    label: String,
    icon: widget::icon::Handle,
    selected: bool,
    route: SettingsRoute,
    notify: bool,
) -> Element<'static, Message> {
    let icon = widget::icon::icon(icon)
        .size(18)
        .width(Length::Fixed(18.0))
        .height(Length::Fixed(18.0));
    let mut label = row![widget::text(label).size(12)]
        .spacing(5)
        .align_y(Alignment::Center);
    if notify {
        label = label.push(update_notification_dot(6.0));
    }
    let content = container(
        column![icon, label]
            .spacing(5)
            .align_x(Alignment::Center)
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .padding([7, 9])
    .align_x(Alignment::Center);

    Element::from(
        widget::button::custom(content)
            .class(settings_category_tab_class(selected))
            .width(Length::FillPortion(1))
            .on_press(Message::NavigateTo(PopupRoute::Settings(route))),
    )
}

fn update_available(update_status: &UpdateStatus) -> bool {
    matches!(update_status, UpdateStatus::UpdateAvailable { .. })
}

fn settings_footer_action(update_status: &UpdateStatus) -> Element<'static, Message> {
    let target = Message::NavigateTo(PopupRoute::Settings(SettingsRoute::General));

    if !update_available(update_status) {
        return widget::button::text(fl!("settings"))
            .leading_icon(widget::icon::from_name("preferences-system-symbolic"))
            .on_press(target)
            .into();
    }

    let icon = row![
        notification_dot(6.0),
        widget::icon::icon(widget::icon::from_name("preferences-system-symbolic").into())
            .size(16)
            .width(Length::Fixed(16.0))
            .height(Length::Fixed(16.0)),
    ]
    .spacing(5)
    .align_y(Alignment::Center);
    let content = row![icon, widget::text(fl!("settings")).size(14)]
        .spacing(4)
        .align_y(Alignment::Center);

    widget::button::custom(content)
        .class(cosmic::theme::Button::Text)
        .padding([0, 8])
        .on_press(target)
        .into()
}

fn notification_dot(size: f32) -> Element<'static, Message> {
    Element::from(
        container(
            cosmic::iced::widget::Space::new()
                .width(Length::Fixed(size))
                .height(Length::Fixed(size)),
        )
        .style(move |_theme: &cosmic::Theme| widget::container::Style {
            text_color: None,
            background: Some(Background::Color(UPDATE_NOTIFICATION_DOT_COLOR)),
            border: cosmic::iced::Border {
                radius: (size / 2.0).into(),
                width: 0.0,
                color: Color::TRANSPARENT,
            },
            shadow: cosmic::iced::Shadow::default(),
            icon_color: None,
            snap: true,
        }),
    )
}

fn update_notification_dot(size: f32) -> Element<'static, Message> {
    widget::tooltip::tooltip(
        notification_dot(size),
        widget::text(fl!("update-dot-tooltip")).size(12),
        widget::tooltip::Position::Top,
    )
    .into()
}

fn settings_category_icon(route: &SettingsRoute) -> widget::icon::Handle {
    match route {
        SettingsRoute::General => widget::icon::from_name("preferences-system-symbolic").into(),
        SettingsRoute::Provider(provider) => {
            provider_icon_handle(*provider, provider_icon_variant())
        }
    }
}

fn settings_category_tab_class(selected: bool) -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(move |_focused, theme| {
            tab_button_style(theme, selected, ButtonInteraction::idle(false), 1.0)
        }),
        disabled: Box::new(move |theme| {
            tab_button_style(theme, selected, ButtonInteraction::idle(false), 0.45)
        }),
        hovered: Box::new(move |_focused, theme| {
            tab_button_style(theme, selected, ButtonInteraction::hover(false), 1.0)
        }),
        pressed: Box::new(move |_focused, theme| {
            tab_button_style(theme, selected, ButtonInteraction::press(false), 0.92)
        }),
    }
}

fn general_settings_view<'a>(
    config: &'a Config,
    update_status: &'a UpdateStatus,
) -> Element<'a, Message> {
    let refresh_section = refresh_section(config.refresh_interval_seconds);
    let panel_icon_section = panel_icon_section(config.panel_icon_style);
    let reset_time_section = reset_time_section(config.reset_time_format);
    let usage_amount_section = usage_amount_section(config.usage_amount_format);
    let about = about_section(update_status);

    Element::from(
        column![
            refresh_section,
            panel_icon_section,
            reset_time_section,
            usage_amount_section,
            about
        ]
        .spacing(14)
        .width(Length::Fill),
    )
}

fn provider_settings_view<'a>(
    state: &'a AppState,
    config: &'a Config,
    logins: ProviderLoginStates<'a>,
    provider_id: ProviderId,
) -> Element<'a, Message> {
    let enabled = state.provider(provider_id).is_some_and(|p| p.enabled);

    let enable_section = settings_block(
        widget::text(fl!("provider-enabled-title")).size(16).into(),
        widget::toggler(enabled)
            .width(Length::Fill)
            .on_toggle(move |e| Message::SetProviderEnabled(provider_id, e)),
    );

    let accounts_section = match provider_id {
        ProviderId::Codex => codex_accounts_section(state, config, logins.codex, enabled),
        ProviderId::Claude => claude_accounts_section(state, config, logins.claude, enabled),
        ProviderId::Cursor => cursor_accounts_section(state, config, logins.cursor, enabled),
    };

    Element::from(
        column![enable_section, accounts_section]
            .spacing(14)
            .width(Length::Fill),
    )
}

fn codex_accounts_section<'a>(
    state: &'a AppState,
    config: &'a Config,
    codex_login: Option<&'a CodexLoginState>,
    enabled: bool,
) -> Element<'a, Message> {
    let codex = state.provider(ProviderId::Codex);
    let active_id = codex.and_then(|provider| provider.active_account_id.as_deref());
    let accounts = state.accounts_for(ProviderId::Codex);
    let mut rows = column![].spacing(8).width(Length::Fill);

    if accounts.is_empty() {
        rows = rows.push(widget::text(fl!("codex-accounts-empty")).size(13));
    } else {
        let mut account_rows = column![].spacing(6).width(Length::Fill);
        for account in &accounts {
            account_rows = account_rows.push(codex_account_settings_row(
                account, active_id, config, enabled,
            ));
        }
        rows = rows.push(account_selector_list(account_rows));
    }

    if !accounts.is_empty() {
        rows = rows.push(codex_auto_detect_active_account_control(config, enabled));
    }

    if let Some(provider) = codex
        && provider.account_status == AccountSelectionStatus::SelectionRequired
    {
        rows = rows.push(widget::text(fl!("codex-account-select-required")).size(13));
    }

    rows = rows.push(codex_login_controls(codex_login, enabled));

    settings_block_enabled(
        widget::text(fl!("codex-accounts-title")).size(16).into(),
        rows,
        enabled,
    )
}

fn codex_auto_detect_active_account_control(
    config: &Config,
    enabled: bool,
) -> Element<'_, Message> {
    let on_toggle =
        enabled.then_some(Message::SetCodexAutoDetectActiveAccount as fn(bool) -> Message);
    Element::from(
        row![
            container(
                widget::checkbox(config.codex_auto_detect_active_account)
                    .width(Length::Shrink)
                    .on_toggle_maybe(on_toggle)
            )
            .padding([2, 0, 0, 0]),
            widget::text(fl!("codex-auto-active-account"))
                .size(14)
                .width(Length::Fill),
        ]
        .spacing(8)
        .align_y(Alignment::Start)
        .width(Length::Fill),
    )
}

fn claude_accounts_section<'a>(
    state: &'a AppState,
    config: &'a Config,
    claude_login: Option<&'a ClaudeLoginState>,
    enabled: bool,
) -> Element<'a, Message> {
    let active_id = state
        .provider(ProviderId::Claude)
        .and_then(|p| p.active_account_id.as_deref());
    let accounts = state.accounts_for(ProviderId::Claude);
    let mut rows = column![].spacing(8).width(Length::Fill);

    if accounts.is_empty() {
        rows = rows.push(widget::text(fl!("claude-accounts-empty")).size(13));
    } else {
        let mut account_rows = column![].spacing(6).width(Length::Fill);
        for account in accounts {
            account_rows = account_rows.push(claude_account_settings_row(
                account, active_id, config, enabled,
            ));
        }
        rows = rows.push(account_selector_list(account_rows));
    }

    rows = rows.push(claude_login_controls(claude_login, enabled));

    settings_block_enabled(
        widget::text(fl!("claude-accounts-title")).size(16).into(),
        rows,
        enabled,
    )
}

fn cursor_accounts_section<'a>(
    state: &'a AppState,
    config: &'a Config,
    cursor_login: Option<&'a CursorLoginState>,
    enabled: bool,
) -> Element<'a, Message> {
    let active_id = state
        .provider(ProviderId::Cursor)
        .and_then(|p| p.active_account_id.as_deref());
    let accounts = state.accounts_for(ProviderId::Cursor);
    let mut rows = column![].spacing(8).width(Length::Fill);

    if accounts.is_empty() {
        rows = rows.push(widget::text(fl!("cursor-accounts-empty")).size(13));
    } else {
        let mut account_rows = column![].spacing(6).width(Length::Fill);
        for account in accounts {
            account_rows = account_rows.push(cursor_account_settings_row(
                account, active_id, config, enabled,
            ));
        }
        rows = rows.push(account_selector_list(account_rows));
    }

    rows = rows.push(cursor_login_controls(cursor_login, enabled));

    settings_block_enabled(
        widget::text(fl!("cursor-accounts-title")).size(16).into(),
        rows,
        enabled,
    )
}

fn cursor_account_settings_row<'a>(
    account: &'a ProviderAccountRuntimeState,
    active_id: Option<&str>,
    config: &'a Config,
    enabled: bool,
) -> Element<'a, Message> {
    let is_active = active_id == Some(account.account_id.as_str());
    let requires_action = cursor_account_requires_action(account);
    let action_support =
        account_action_support(config, ProviderId::Cursor, account.account_id.as_str());
    let can_reauthenticate = enabled
        && requires_action
        && action_support.as_ref().is_some_and(|support| {
            support.can_reauthenticate && support.supports_background_status_refresh
        });
    let account_id = account.account_id.clone();

    let title_row = row![account_label_text(&account.label, 14)]
        .spacing(8)
        .align_y(Alignment::Center)
        .width(Length::Fill);
    let mut selector_body = column![title_row].spacing(6).width(Length::Fill);
    if requires_action {
        selector_body = selector_body.push(
            row![badge_warning(fl!("cursor-account-reauth-badge"))]
                .width(Length::Fill)
                .align_y(Alignment::Center),
        );
    }

    let selector_content = container(selector_body)
        .padding([10, 12])
        .width(Length::Fill);

    let selector = widget::button::custom(selector_content)
        .class(account_row_button_class(is_active))
        .width(Length::Fill)
        .on_press_maybe((enabled && !is_active).then_some(Message::SetActiveAccount(
            ProviderId::Cursor,
            account_id.clone(),
        )));

    let delete_press = cursor_delete_message(config, account, enabled);
    let mut actions = row![account_selected_marker(is_active, enabled)]
        .spacing(0)
        .align_y(Alignment::Center);
    if can_reauthenticate {
        actions = actions.push(
            widget::button::icon(widget::icon::from_name("view-refresh-symbolic"))
                .class(account_row_icon_button_class())
                .tooltip(fl!("cursor-account-reauth-tooltip"))
                .on_press(Message::ReauthenticateCursorAccount(
                    account.account_id.clone(),
                )),
        );
    }
    actions = actions.push(
        widget::button::icon(widget::icon::from_name("edit-delete-symbolic"))
            .class(account_row_icon_button_class())
            .tooltip(fl!("account-delete-tooltip"))
            .on_press_maybe(delete_press),
    );

    Element::from(account_row_container(
        selector.into(),
        actions.into(),
        is_active,
        enabled,
        requires_action,
    ))
}

fn codex_login_controls(login: Option<&CodexLoginState>, enabled: bool) -> Element<'_, Message> {
    let Some(login) = login else {
        return widget::button::standard(fl!("account-add"))
            .on_press_maybe(enabled.then_some(Message::StartCodexLogin))
            .into();
    };

    let mut content = column![widget::text(codex_login_status(login)).size(13)]
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

fn claude_login_controls(login: Option<&ClaudeLoginState>, enabled: bool) -> Element<'_, Message> {
    let Some(login) = login else {
        return widget::button::standard(fl!("account-add"))
            .on_press_maybe(enabled.then_some(Message::StartClaudeLogin))
            .into();
    };

    let mut content = column![widget::text(claude_login_status(login)).size(13)]
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

fn cursor_login_controls(login: Option<&CursorLoginState>, enabled: bool) -> Element<'_, Message> {
    let Some(login) = login else {
        return widget::button::standard(fl!("account-add"))
            .on_press_maybe(enabled.then_some(Message::StartCursorLogin))
            .into();
    };

    let mut content = column![widget::text(cursor_login_status(login)).size(13)]
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

fn cursor_delete_message(
    config: &Config,
    account: &ProviderAccountRuntimeState,
    enabled: bool,
) -> Option<Message> {
    let can_delete =
        account_action_support(config, ProviderId::Cursor, account.account_id.as_str())
            .is_some_and(|support| support.can_delete);
    if !enabled || !can_delete {
        return None;
    }
    Some(Message::DeleteCursorAccount(account.account_id.clone()))
}

fn codex_account_settings_row<'a>(
    account: &'a ProviderAccountRuntimeState,
    active_id: Option<&str>,
    config: &'a Config,
    enabled: bool,
) -> Element<'a, Message> {
    let is_active = active_id == Some(account.account_id.as_str());
    let account_id = account.account_id.clone();
    let selector_content = container(
        row![account_label_text(&account.label, 14),]
            .spacing(12)
            .align_y(Alignment::Center)
            .width(Length::Fill),
    )
    .padding([10, 12])
    .width(Length::Fill);

    let selector = widget::button::custom(selector_content)
        .class(account_row_button_class(is_active))
        .width(Length::Fill)
        .on_press_maybe((enabled && !is_active).then_some(Message::SetActiveAccount(
            ProviderId::Codex,
            account_id.clone(),
        )));

    let can_delete = account_action_support(config, ProviderId::Codex, account_id.as_str())
        .is_some_and(|support| support.can_delete);
    let delete_press = (enabled && can_delete).then_some(Message::DeleteCodexAccount(account_id));
    let actions = row![
        account_selected_marker(is_active, enabled),
        widget::button::icon(widget::icon::from_name("edit-delete-symbolic"))
            .class(account_row_icon_button_class())
            .tooltip(fl!("account-delete-tooltip"))
            .on_press_maybe(delete_press),
    ]
    .spacing(0)
    .align_y(Alignment::Center);

    Element::from(account_row_container(
        selector.into(),
        actions.into(),
        is_active,
        enabled,
        false,
    ))
}

fn claude_account_row_label(account: &ProviderAccountRuntimeState, config: &Config) -> String {
    let id = account.account_id.as_str();
    let managed = config.claude_managed_accounts.iter().find(|m| m.id == id);
    let config_email = managed
        .and_then(|m| m.email.as_deref())
        .filter(|e| !e.is_empty());
    let snap_email = account
        .snapshot
        .as_ref()
        .and_then(|s| s.identity.email.as_deref())
        .filter(|e| !e.is_empty());
    snap_email
        .or(config_email)
        .unwrap_or(account.label.as_str())
        .to_string()
}

fn claude_account_settings_row<'a>(
    account: &'a ProviderAccountRuntimeState,
    active_id: Option<&str>,
    config: &'a Config,
    enabled: bool,
) -> Element<'a, Message> {
    let is_active = active_id == Some(account.account_id.as_str());
    let account_id = account.account_id.clone();
    let row_label = claude_account_row_label(account, config);
    let selector_content = container(
        row![account_label_text(&row_label, 14),]
            .spacing(12)
            .align_y(Alignment::Center)
            .width(Length::Fill),
    )
    .padding([10, 12])
    .width(Length::Fill);

    let selector = widget::button::custom(selector_content)
        .class(account_row_button_class(is_active))
        .width(Length::Fill)
        .on_press_maybe((enabled && !is_active).then_some(Message::SetActiveAccount(
            ProviderId::Claude,
            account_id.clone(),
        )));

    let can_delete = account_action_support(config, ProviderId::Claude, account_id.as_str())
        .is_some_and(|support| support.can_delete);
    let delete_press = (enabled && can_delete).then_some(Message::DeleteClaudeAccount(account_id));
    let actions = row![
        account_selected_marker(is_active, enabled),
        widget::button::icon(widget::icon::from_name("edit-delete-symbolic"))
            .class(account_row_icon_button_class())
            .tooltip(fl!("account-delete-tooltip"))
            .on_press_maybe(delete_press),
    ]
    .spacing(0)
    .align_y(Alignment::Center);

    Element::from(account_row_container(
        selector.into(),
        actions.into(),
        is_active,
        enabled,
        false,
    ))
}

fn account_selected_marker(selected: bool, enabled: bool) -> Element<'static, Message> {
    if !selected {
        return cosmic::iced::widget::Space::new()
            .width(Length::Fixed(18.0))
            .into();
    }

    container(
        widget::icon::icon(widget::icon::from_name("object-select-symbolic").into())
            .size(18)
            .width(Length::Fixed(18.0))
            .height(Length::Fixed(18.0)),
    )
    .style(move |theme| {
        let cosmic = theme.cosmic();
        let color = if enabled {
            cosmic.accent.base.into()
        } else {
            apply_alpha(cosmic.background.component.on.into(), 0.45)
        };
        widget::container::Style {
            text_color: Some(color),
            background: None,
            border: cosmic::iced::Border::default(),
            shadow: cosmic::iced::Shadow::default(),
            icon_color: Some(color),
            snap: true,
        }
    })
    .into()
}

fn account_action_support(
    config: &Config,
    provider: ProviderId,
    account_id: &str,
) -> Option<ProviderAccountActionSupport> {
    registry::discover_accounts(provider, config)
        .into_iter()
        .find(|account| account.provider == provider && account.account_id == account_id)
        .map(|account| account.action_support)
}

fn account_selector_list<'a>(rows: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(rows).width(Length::Fill).into()
}

fn account_row_container<'a>(
    selector: Element<'a, Message>,
    delete_button: Element<'a, Message>,
    selected: bool,
    enabled: bool,
    action_required: bool,
) -> Element<'a, Message> {
    container(
        row![selector, delete_button]
            .spacing(0)
            .align_y(Alignment::Center)
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .style(move |theme: &cosmic::Theme| {
        let cosmic = theme.cosmic();
        let surface = &cosmic.background.component;
        let warning = cosmic.warning.base;
        widget::container::Style {
            text_color: Some(surface.on.into()),
            background: Some(Background::Color(if action_required {
                apply_alpha(warning.into(), 0.08)
            } else if selected && enabled {
                accent_selection_fill(theme)
            } else {
                surface.base.into()
            })),
            border: cosmic::iced::Border {
                radius: cosmic.corner_radii.radius_s.into(),
                width: if selected { 2.0 } else { 1.0 },
                color: if selected {
                    if enabled {
                        cosmic.accent.base.into()
                    } else {
                        apply_alpha(surface.on.into(), 0.45)
                    }
                } else if action_required {
                    apply_alpha(warning.into(), 0.72)
                } else {
                    surface.divider.into()
                },
            },
            shadow: cosmic::iced::Shadow::default(),
            icon_color: Some(surface.on.into()),
            snap: true,
        }
    })
    .into()
}

fn account_row_button_class(selected: bool) -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(move |focused, theme| {
            account_row_button_style(theme, selected, focused, 1.0)
        }),
        disabled: Box::new(move |theme| {
            let opacity = if selected { 1.0 } else { 0.45 };
            account_row_button_style(theme, selected, false, opacity)
        }),
        hovered: Box::new(move |focused, theme| {
            account_row_button_style(theme, selected, focused, 1.0)
        }),
        pressed: Box::new(move |focused, theme| {
            account_row_button_style(theme, selected, focused, 0.92)
        }),
    }
}

fn account_row_button_style(
    theme: &cosmic::Theme,
    selected: bool,
    focused: bool,
    opacity: f32,
) -> widget::button::Style {
    let cosmic = theme.cosmic();
    let mut style = widget::button::Style::new();
    let foreground = cosmic.background.component.on.into();

    style.icon_color = Some(apply_alpha(foreground, opacity));
    style.text_color = Some(apply_alpha(foreground, opacity));
    style.border_radius = cosmic.corner_radii.radius_s.into();
    style.border_width = if focused && selected { 1.0 } else { 0.0 };
    style.border_color = cosmic.accent.base.into();

    style
}

fn account_row_icon_button_class() -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(move |_focused, theme| account_row_icon_button_style(theme, 1.0)),
        disabled: Box::new(move |theme| account_row_icon_button_style(theme, 0.45)),
        hovered: Box::new(move |_focused, theme| account_row_icon_button_style(theme, 1.0)),
        pressed: Box::new(move |_focused, theme| account_row_icon_button_style(theme, 0.85)),
    }
}

fn account_row_icon_button_style(theme: &cosmic::Theme, opacity: f32) -> widget::button::Style {
    let cosmic = theme.cosmic();
    let mut style = widget::button::Style::new();
    let foreground = cosmic.background.component.on.into();

    style.icon_color = Some(apply_alpha(foreground, opacity));
    style.text_color = Some(apply_alpha(foreground, opacity));
    style.border_radius = cosmic.corner_radii.radius_m.into();

    style
}
#[derive(Clone, Copy)]
struct ButtonInteraction {
    focused: bool,
    hovered: bool,
    pressed: bool,
}

impl ButtonInteraction {
    const fn idle(focused: bool) -> Self {
        Self {
            focused,
            hovered: false,
            pressed: false,
        }
    }

    const fn hover(focused: bool) -> Self {
        Self {
            focused,
            hovered: true,
            pressed: false,
        }
    }

    const fn press(focused: bool) -> Self {
        Self {
            focused,
            hovered: true,
            pressed: true,
        }
    }
}

fn refresh_section(current_seconds: u64) -> Element<'static, Message> {
    let options: &[(u64, &str)] = &[
        (60, "1 min"),
        (300, "5 min"),
        (900, "15 min"),
        (1800, "30 min"),
    ];

    let buttons = options.iter().fold(
        row![].spacing(8).width(Length::Fill),
        |row, (secs, text)| {
            let is_selected = *secs == current_seconds;
            let content = container(widget::text(*text).size(13))
                .width(Length::Fill)
                .padding([9, 8])
                .align_x(Alignment::Center);
            row.push(
                widget::button::custom(content)
                    .class(refresh_option_class(is_selected))
                    .on_press(Message::SetRefreshInterval(*secs))
                    .width(Length::FillPortion(1)),
            )
        },
    );

    settings_block(
        widget::text(fl!("refresh-section-title")).size(16).into(),
        buttons,
    )
}

fn panel_icon_section(current_style: PanelIconStyle) -> Element<'static, Message> {
    let options = [
        PanelIconStyle::LogoAndBars,
        PanelIconStyle::BarsOnly,
        PanelIconStyle::LogoAndPercent,
        PanelIconStyle::PercentOnly,
    ];

    let buttons = options
        .iter()
        .fold(row![].spacing(6).width(Length::Fill), |row, style| {
            let is_selected = *style == current_style;
            let content = container(panel_icon_preview(*style))
                .width(Length::Fill)
                .padding([9, 6])
                .align_x(Alignment::Center);
            let button = widget::button::custom(content)
                .class(refresh_option_class(is_selected))
                .on_press(Message::SetPanelIconStyle(*style))
                .width(Length::FillPortion(1));
            let content: Element<'static, Message> = if *style == PanelIconStyle::PercentOnly {
                widget::tooltip::tooltip(
                    button,
                    widget::text(fl!("panel-icon-percent-only-tooltip")).size(12),
                    widget::tooltip::Position::Top,
                )
                .into()
            } else {
                button.into()
            };

            row.push(content)
        });

    settings_block(
        widget::text(fl!("panel-icon-section-title"))
            .size(16)
            .into(),
        buttons,
    )
}

fn panel_icon_preview(style: PanelIconStyle) -> Element<'static, Message> {
    let logo = widget::icon::icon(provider_icon_handle(
        ProviderId::Codex,
        provider_icon_variant(),
    ))
    .size(16)
    .width(Length::Fixed(16.0))
    .height(Length::Fixed(16.0));
    let bars = column![
        progress_bar(0.0..=100.0, 86.5)
            .length(Length::Fixed(38.0))
            .girth(Length::Fixed(5.0)),
        progress_bar(0.0..=100.0, 42.0)
            .length(Length::Fixed(38.0))
            .girth(Length::Fixed(3.0)),
    ]
    .spacing(3)
    .width(Length::Fixed(38.0));

    let preview: Element<'static, Message> = match style {
        PanelIconStyle::LogoAndBars => row![logo, bars]
            .spacing(5)
            .align_y(Alignment::Center)
            .into(),
        PanelIconStyle::BarsOnly => bars.into(),
        PanelIconStyle::LogoAndPercent => row![logo, widget::text("86.5%").size(12)]
            .spacing(5)
            .align_y(Alignment::Center)
            .into(),
        PanelIconStyle::PercentOnly => widget::text("86.5%").size(12).into(),
    };

    container(preview)
        .height(Length::Fixed(22.0))
        .align_y(Alignment::Center)
        .into()
}

fn reset_time_section(current_format: ResetTimeFormat) -> Element<'static, Message> {
    let options = [
        (ResetTimeFormat::Relative, fl!("reset-time-relative")),
        (ResetTimeFormat::Absolute, fl!("reset-time-absolute")),
    ];
    let now = chrono::Utc::now();
    let example_window = UsageWindow {
        label: "Session".to_string(),
        used_percent: 50.0,
        reset_at: Some(now + chrono::Duration::hours(28)),
        window_seconds: None,
        reset_description: None,
    };

    let buttons = options.iter().fold(
        row![].spacing(8).width(Length::Fill),
        |row, (format, text)| {
            let is_selected = *format == current_format;
            let example = usage_display::reset_label(&example_window, now, *format)
                .unwrap_or_else(|| fl!("reset-now"));
            let example_size = if *format == ResetTimeFormat::Absolute {
                9
            } else {
                10
            };
            let content = container(
                column![
                    widget::text(text.clone()).size(13),
                    widget::text(example).size(example_size)
                ]
                .spacing(3)
                .align_x(Alignment::Center)
                .width(Length::Fill),
            )
            .width(Length::Fill)
            .padding([8, 8])
            .align_x(Alignment::Center);
            row.push(
                widget::button::custom(content)
                    .class(refresh_option_class(is_selected))
                    .on_press(Message::SetResetTimeFormat(*format))
                    .width(Length::FillPortion(1)),
            )
        },
    );

    settings_block(
        widget::text(fl!("reset-time-section-title"))
            .size(16)
            .into(),
        buttons,
    )
}

fn usage_amount_section(current_format: UsageAmountFormat) -> Element<'static, Message> {
    let options = [
        (UsageAmountFormat::Used, fl!("usage-amount-used")),
        (UsageAmountFormat::Left, fl!("usage-amount-left")),
    ];

    let buttons = options.iter().fold(
        row![].spacing(8).width(Length::Fill),
        |row, (format, text)| {
            let is_selected = *format == current_format;
            let content = container(widget::text(text.clone()).size(13))
                .width(Length::Fill)
                .padding([9, 8])
                .align_x(Alignment::Center);
            row.push(
                widget::button::custom(content)
                    .class(refresh_option_class(is_selected))
                    .on_press(Message::SetUsageAmountFormat(*format))
                    .width(Length::FillPortion(1)),
            )
        },
    );

    settings_block(
        widget::text(fl!("usage-amount-section-title"))
            .size(16)
            .into(),
        buttons,
    )
}

fn refresh_option_class(selected: bool) -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(move |focused, theme| refresh_option_style(theme, selected, focused, 1.0)),
        disabled: Box::new(move |theme| refresh_option_style(theme, selected, false, 0.45)),
        hovered: Box::new(move |focused, theme| {
            refresh_option_interaction_style(
                theme,
                selected,
                ButtonInteraction::hover(focused),
                1.0,
            )
        }),
        pressed: Box::new(move |focused, theme| {
            refresh_option_interaction_style(
                theme,
                selected,
                ButtonInteraction::press(focused),
                0.92,
            )
        }),
    }
}

fn refresh_option_style(
    theme: &cosmic::Theme,
    selected: bool,
    focused: bool,
    opacity: f32,
) -> widget::button::Style {
    refresh_option_interaction_style(theme, selected, ButtonInteraction::idle(focused), opacity)
}

fn refresh_option_interaction_style(
    theme: &cosmic::Theme,
    selected: bool,
    interaction: ButtonInteraction,
    opacity: f32,
) -> widget::button::Style {
    let cosmic = theme.cosmic();
    let mut style = widget::button::Style::new();
    let surface = &cosmic.background.component;

    let (background, foreground, border_color) = if selected {
        (
            accent_selection_fill(theme),
            surface.on.into(),
            cosmic.accent.base.into(),
        )
    } else if interaction.pressed {
        (
            surface.divider.into(),
            surface.on.into(),
            surface.divider.into(),
        )
    } else if interaction.hovered {
        (
            cosmic.background.component.hover.into(),
            surface.on.into(),
            surface.divider.into(),
        )
    } else {
        (
            surface.base.into(),
            surface.on.into(),
            surface.divider.into(),
        )
    };

    style.background = Some(Background::Color(apply_alpha(background, opacity)));
    style.border_radius = cosmic.corner_radii.radius_s.into();
    style.border_width = if selected { 2.0 } else { 1.0 };
    style.border_color = apply_alpha(border_color, opacity);
    style.outline_width = if interaction.focused { 1.0 } else { 0.0 };
    style.outline_color = cosmic.accent.base.into();
    style.text_color = Some(apply_alpha(foreground, opacity));
    style.icon_color = Some(apply_alpha(foreground, opacity));

    style
}

fn about_section(update_status: &UpdateStatus) -> Element<'_, Message> {
    let current_version = env!("CARGO_PKG_VERSION");
    let mut title = row![widget::text(fl!("about-section-title")).size(16)]
        .align_y(Alignment::Center)
        .spacing(8);
    if update_available(update_status) {
        title = title.push(update_notification_dot(7.0));
    }

    let update_line: Element<'_, Message> = match update_status {
        UpdateStatus::UpdateAvailable { version, url } => column![
            widget::text(fl!("update-available", version = version.as_str())).size(12),
            widget::button::link(fl!("update-open-release"))
                .on_press(Message::OpenUrl(url.clone())),
        ]
        .spacing(4)
        .width(Length::Fill)
        .into(),
        UpdateStatus::Unchecked => widget::text(fl!("update-checking")).size(12).into(),
        UpdateStatus::NoUpdate => widget::text(fl!("update-up-to-date")).size(12).into(),
        UpdateStatus::Error(reason) => column![
            widget::text(fl!("update-failed", reason = reason.as_str())).size(12),
            widget::button::text(fl!("update-check-again"))
                .padding([0, 0])
                .on_press(Message::CheckUpdates),
        ]
        .spacing(4)
        .width(Length::Fill)
        .into(),
    };

    let inner = column![
        widget::text(fl!("app-version", version = current_version)).size(12),
        update_line,
    ]
    .spacing(6)
    .width(Length::Fill);

    settings_block(title.into(), inner)
}

fn provider_tab(
    state: &AppState,
    provider: &ProviderRuntimeState,
    selected: bool,
) -> Element<'static, Message> {
    let weekly = tab_percent(state, provider);
    let icon_variant = provider_icon_variant();
    let badge = widget::icon::icon(provider_icon_handle(provider.provider, icon_variant))
        .size(18)
        .width(Length::Fixed(18.0))
        .height(Length::Fixed(18.0));
    let label = widget::text(provider.provider.label()).size(12);
    let bar = progress_bar(0.0..=100.0, weekly)
        .length(Length::Fill)
        .girth(Length::Fixed(4.0));

    let content = container(
        column![badge, label, bar]
            .spacing(5)
            .align_x(Alignment::Center)
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .padding([7, 9]);

    Element::from(
        widget::button::custom(content)
            .class(provider_tab_class(selected))
            .width(Length::FillPortion(1))
            .on_press(Message::SelectProvider(provider.provider)),
    )
}

fn provider_tab_class(selected: bool) -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(move |focused, theme| {
            tab_button_style(theme, selected, ButtonInteraction::idle(focused), 1.0)
        }),
        disabled: Box::new(move |theme| {
            tab_button_style(theme, selected, ButtonInteraction::idle(false), 0.45)
        }),
        hovered: Box::new(move |focused, theme| {
            tab_button_style(theme, selected, ButtonInteraction::hover(focused), 1.0)
        }),
        pressed: Box::new(move |focused, theme| {
            tab_button_style(theme, selected, ButtonInteraction::press(focused), 0.92)
        }),
    }
}

fn tab_button_style(
    theme: &cosmic::Theme,
    selected: bool,
    interaction: ButtonInteraction,
    opacity: f32,
) -> widget::button::Style {
    let cosmic = theme.cosmic();
    let mut style = widget::button::Style::new();
    let surface = &cosmic.background.component;

    let background = if selected {
        if interaction.pressed {
            surface.divider.into()
        } else {
            accent_selection_fill(theme)
        }
    } else if interaction.pressed {
        surface.divider.into()
    } else if interaction.hovered {
        cosmic.background.component.hover.into()
    } else {
        surface.base.into()
    };

    style.background = Some(Background::Color(apply_alpha(background, opacity)));
    style.border_radius = cosmic.corner_radii.radius_s.into();
    style.border_width = if selected { 2.0 } else { 1.0 };
    style.border_color = if selected {
        apply_alpha(cosmic.accent.base.into(), opacity)
    } else {
        apply_alpha(surface.divider.into(), opacity)
    };
    style.outline_width = if interaction.focused && selected {
        1.0
    } else {
        0.0
    };
    style.outline_color = cosmic.accent.base.into();
    style.text_color = Some(apply_alpha(surface.on.into(), opacity));
    style.icon_color = Some(apply_alpha(surface.on.into(), opacity));

    style
}

fn apply_alpha(mut color: Color, opacity: f32) -> Color {
    color.a *= opacity;
    color
}

fn provider_summary(
    provider: &ProviderRuntimeState,
    active_account: Option<&ProviderAccountRuntimeState>,
    state: &AppState,
) -> Element<'static, Message> {
    let title = row![
        widget::icon::icon(provider_icon_handle(
            provider.provider,
            provider_icon_variant(),
        ))
        .size(24)
        .width(Length::Fixed(24.0))
        .height(Length::Fixed(24.0)),
        widget::text(provider.provider.label()).size(28),
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    let title_row = row![
        title,
        cosmic::iced::widget::Space::new().width(Length::Fill),
        provider_status_badge(state, provider)
    ]
    .align_y(Alignment::Center);
    let updated_label = active_account
        .and_then(|account| account.last_success_at)
        .map(format_updated_label);

    let mut col = column![title_row].spacing(6);
    if let Some(label) = updated_label {
        col = col.push(widget::text(label).size(14));
    }

    card(col)
}

fn badge_success(label: impl Into<String>) -> Element<'static, Message> {
    let label = label.into();
    badge_container(label, move |theme| {
        let cosmic = theme.cosmic();
        let color = cosmic.success.base.into();
        badge_style(apply_alpha(color, 0.14), color, color, theme)
    })
}

fn badge_warning(label: impl Into<String>) -> Element<'static, Message> {
    let label = label.into();
    badge_container(label, move |theme| {
        let cosmic = theme.cosmic();
        let color = cosmic.warning.base.into();
        badge_style(apply_alpha(color, 0.14), color, color, theme)
    })
}

fn badge_destructive(label: impl Into<String>) -> Element<'static, Message> {
    let label = label.into();
    badge_container(label, move |theme| {
        let cosmic = theme.cosmic();
        let color = cosmic.destructive.base.into();
        badge_style(apply_alpha(color, 0.14), color, color, theme)
    })
}

fn badge_neutral(label: impl Into<String>) -> Element<'static, Message> {
    let label = label.into();
    badge_container(label, move |theme| {
        let cosmic = theme.cosmic();
        let surface = &cosmic.background.component;
        badge_style(
            apply_alpha(surface.base.into(), 0.42),
            surface.on.into(),
            surface.divider.into(),
            theme,
        )
    })
}

fn badge_container(
    label: String,
    style: impl Fn(&cosmic::Theme) -> widget::container::Style + 'static,
) -> Element<'static, Message> {
    Element::from(
        container(widget::text(label).size(12))
            .padding([3, 7])
            .style(style),
    )
}

fn badge_style(
    bg: Color,
    text_color: Color,
    border_color: Color,
    theme: &cosmic::Theme,
) -> widget::container::Style {
    let cosmic = theme.cosmic();
    widget::container::Style {
        text_color: Some(text_color),
        background: Some(Background::Color(bg)),
        border: cosmic::iced::Border {
            radius: cosmic.corner_radii.radius_s.into(),
            width: 1.0,
            color: border_color,
        },
        shadow: cosmic::iced::Shadow::default(),
        icon_color: None,
        snap: true,
    }
}

fn plan_badge(label: &str) -> Element<'static, Message> {
    badge_neutral(format_plan_label(label))
}

fn format_plan_label(label: &str) -> String {
    let mut chars = label.trim().chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    first.to_uppercase().chain(chars).collect()
}

#[derive(Clone)]
struct UsageDisplayData {
    percent: f32,
    primary: String,
    secondary: Option<String>,
    pace: Option<usage_display::UsagePace>,
}

fn selected_provider_view<'a>(
    provider: Option<&'a ProviderRuntimeState>,
    state: &'a AppState,
    config: &'a Config,
) -> Element<'a, Message> {
    let Some(provider) = provider else {
        return no_providers_view();
    };
    let active_account = state.active_account(provider.provider);
    let snapshot = active_account
        .and_then(|account| account.snapshot.as_ref())
        .or(provider.legacy_display_snapshot.as_ref());

    let summary = provider_summary(provider, active_account, state);
    let mut content = column![summary]
        .spacing(PROVIDER_CARD_SPACING)
        .width(Length::Fill);
    let inactive_accounts = provider_detail_accounts(state, provider);

    if let Some(snapshot) = snapshot {
        if active_account.is_some_and(|account| account.health == ProviderHealth::Error) {
            content = content.push(provider_status_info(provider, state, active_account));
        }
        let mut cost_shown = false;
        for window in &snapshot.windows {
            if window.label == "Extra" && snapshot.provider_cost.is_some() {
                content = content.push(extra_section(
                    window,
                    snapshot.provider_cost.as_ref(),
                    config.usage_amount_format,
                ));
                cost_shown = true;
            } else {
                content = content.push(usage_section(
                    window,
                    config.reset_time_format,
                    config.usage_amount_format,
                ));
            }
        }
        if !cost_shown && let Some(cost) = &snapshot.provider_cost {
            content = content.push(cost_section(provider.provider, cost));
        }
        let account_label = snapshot
            .identity
            .email
            .as_deref()
            .or_else(|| active_account.map(|a| a.label.as_str()));
        if let Some(account_label) = account_label {
            content = content.push(account_section(
                account_label,
                snapshot.identity.plan.as_deref(),
            ));
        }
    } else {
        content = content.push(provider_status_info(provider, state, active_account));
    }

    if !inactive_accounts.is_empty() {
        content = content.push(inactive_accounts_section(
            inactive_accounts,
            active_account.is_some(),
            config,
        ));
    }

    Element::from(content)
}

fn no_providers_view<'a>() -> Element<'a, Message> {
    card(
        column![
            widget::text(fl!("no-providers")).size(16),
            widget::text(fl!("no-providers-detail")).size(13),
            widget::button::standard(fl!("no-providers-open-settings")).on_press(
                Message::NavigateTo(PopupRoute::Settings(SettingsRoute::General))
            ),
        ]
        .spacing(10)
        .width(Length::Fill),
    )
}

fn provider_status_info(
    provider: &ProviderRuntimeState,
    state: &AppState,
    active_account: Option<&ProviderAccountRuntimeState>,
) -> Element<'static, Message> {
    info_block(
        fl!("status-label"),
        provider_status_message(provider, state, active_account),
        None,
    )
}

fn provider_status_message(
    provider: &ProviderRuntimeState,
    state: &AppState,
    active_account: Option<&ProviderAccountRuntimeState>,
) -> String {
    let mut messages = Vec::new();

    if provider.provider == ProviderId::Codex
        && provider.account_status == AccountSelectionStatus::LoginRequired
    {
        messages.push(fl!("codex-no-accounts-status"));
        messages.push(fl!("codex-no-accounts-action"));
    } else {
        if provider.provider == ProviderId::Cursor
            && state
                .accounts_for(ProviderId::Cursor)
                .into_iter()
                .any(cursor_account_requires_action)
        {
            messages.push(fl!("cursor-accounts-reauth-summary"));
            messages.push(fl!("cursor-accounts-reauth-action"));
        }

        if let Some(account) = active_account
            && account.health == ProviderHealth::Error
            && let Some(error) = &account.error
        {
            if provider.provider != ProviderId::Cursor
                || !state
                    .accounts_for(ProviderId::Cursor)
                    .into_iter()
                    .any(cursor_account_requires_action)
            {
                messages.push(error.clone());
            }
        } else {
            let status = provider.status_line(active_account);
            if !(provider.provider == ProviderId::Cursor
                && status == "Login required"
                && state
                    .accounts_for(ProviderId::Cursor)
                    .into_iter()
                    .any(cursor_account_requires_action))
            {
                messages.push(status);
            }
        }
    }

    dedup_status_messages(messages).join(" ")
}

fn dedup_status_messages(messages: Vec<String>) -> Vec<String> {
    let mut deduped = Vec::new();
    for message in messages {
        if !message.is_empty() && !deduped.contains(&message) {
            deduped.push(message);
        }
    }
    deduped
}

fn settings_body_height(state: &AppState) -> f32 {
    let max_accounts = [ProviderId::Codex, ProviderId::Claude, ProviderId::Cursor]
        .into_iter()
        .map(|provider| state.accounts_for(provider).len().max(1))
        .max()
        .unwrap_or(1);
    let account_rows = f32::from(u16::try_from(max_accounts).unwrap_or(u16::MAX));
    let general_height = {
        let refresh = SETTINGS_SECTION_HEIGHT;
        let panel_icon = 128.0;
        let reset_time = SETTINGS_SECTION_HEIGHT;
        let usage_amount = SETTINGS_SECTION_HEIGHT;
        let about = SETTINGS_SECTION_HEIGHT;
        refresh + panel_icon + reset_time + usage_amount + about + 70.0
    };
    let provider_settings_height = {
        let enable_section = 40.0 + SETTINGS_PROVIDER_ROW_HEIGHT;
        let accounts_section = 40.0 + account_rows * SETTINGS_PROVIDER_ROW_HEIGHT + 40.0;
        enable_section + accounts_section + 28.0 + 8.0
    };
    let placeholder_height = SETTINGS_SECTION_HEIGHT * 2.0 + 28.0;
    general_height
        .max(provider_settings_height)
        .max(placeholder_height)
}

fn provider_body_height(state: &AppState, provider: Option<&ProviderRuntimeState>) -> f32 {
    let Some(provider) = provider else {
        return PROVIDER_SUMMARY_HEIGHT;
    };

    let mut sections = 1usize;
    let snapshot = active_snapshot(state, provider);
    if let Some(snapshot) = snapshot {
        if active_health(state, provider) == Some(ProviderHealth::Error) {
            sections += 1;
        }

        let mut cost_shown = false;
        for window in &snapshot.windows {
            sections += 1;
            if window.label == "Extra" && snapshot.provider_cost.is_some() {
                cost_shown = true;
            }
        }

        if !cost_shown && snapshot.provider_cost.is_some() {
            sections += 1;
        }
        let active_account = state.active_account(provider.provider);
        if snapshot.identity.email.is_some() || active_account.is_some() {
            sections += 1;
        }
    } else {
        sections += 1;
    }

    let extra_sections = f32::from(u16::try_from(sections.saturating_sub(1)).unwrap_or(u16::MAX));
    let mut height =
        PROVIDER_SUMMARY_HEIGHT + extra_sections * (PROVIDER_SECTION_HEIGHT + PROVIDER_HEIGHT_SECTION_SPACING);
    let account_rows = provider_detail_accounts(state, provider).len();
    if account_rows > 0 {
        let account_rows = f32::from(u16::try_from(account_rows).unwrap_or(u16::MAX));
        height += PROVIDER_HEIGHT_SECTION_SPACING
            + PROVIDER_ACCOUNT_LIST_TITLE_HEIGHT
            + account_rows * PROVIDER_ACCOUNT_LIST_ROW_HEIGHT;
        if account_rows > 1.0 {
            height += (account_rows - 1.0) * PROVIDER_ACCOUNT_LIST_SPACING;
        }
    }
    height
}

fn provider_detail_accounts<'a>(
    state: &'a AppState,
    provider: &'a ProviderRuntimeState,
) -> Vec<&'a ProviderAccountRuntimeState> {
    let active_id = provider.active_account_id.as_deref();
    state
        .accounts_for(provider.provider)
        .into_iter()
        .filter(|account| Some(account.account_id.as_str()) != active_id)
        .collect()
}

fn inactive_accounts_section<'a>(
    accounts: Vec<&'a ProviderAccountRuntimeState>,
    has_active_account: bool,
    config: &'a Config,
) -> Element<'a, Message> {
    let title = if has_active_account {
        fl!("other-accounts-label")
    } else {
        fl!("accounts-label")
    };
    let mut list = column![].spacing(PROVIDER_ACCOUNT_LIST_SPACING).width(Length::Fill);
    for account in accounts {
        list = list.push(inactive_account_row(account, config));
    }

    Element::from(
        column![widget::text(title).size(18), list]
            .spacing(8)
            .width(Length::Fill),
    )
}

fn inactive_account_row<'a>(
    account: &'a ProviderAccountRuntimeState,
    config: &'a Config,
) -> Element<'a, Message> {
    let snapshot = account.snapshot.as_ref();
    let label = account_display_label(account, snapshot);
    let mut header = row![
        account_label_text(label, 14),
        cosmic::iced::widget::Space::new().width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    if let Some(updated) = account.last_success_at.map(format_updated_label) {
        header = header.push(widget::text(updated).size(12));
    }

    let mut content = column![header].spacing(6).width(Length::Fill);

    if let Some(snapshot) = snapshot {
        if let Some(window) = snapshot
            .session_window()
            .or_else(|| snapshot.headline_window())
            .or_else(|| snapshot.windows.first())
        {
            let display = usage_display_data(
                window,
                config.reset_time_format,
                config.usage_amount_format,
                inactive_account_snapshot_time(account, snapshot),
            );
            content = content.push(widget::text(window.label.clone()).size(14));
            content = content.push(muted_progress_bar(display.percent));
            content = content.push(
                row![
                    widget::text(display.primary).size(13),
                    cosmic::iced::widget::Space::new().width(Length::Fill),
                    widget::text(display.secondary.unwrap_or_default()).size(12),
                ]
                .align_y(Alignment::Center),
            );
        } else {
            content = content.push(widget::text(account.status_line()).size(13));
        }
    } else {
        content = content.push(widget::text(account.status_line()).size(13));
    }

    Element::from(
        container(content)
            .width(Length::Fill)
            .padding([10, 12])
            .style(|theme| {
                let cosmic = theme.cosmic();
                let surface = &cosmic.background.component;
                widget::container::Style {
                    text_color: Some(apply_alpha(surface.on.into(), 0.68)),
                    background: Some(Background::Color(apply_alpha(surface.base.into(), 0.6))),
                    border: cosmic::iced::Border {
                        radius: cosmic.corner_radii.radius_s.into(),
                        width: 1.0,
                        color: apply_alpha(surface.divider.into(), 0.85),
                    },
                    shadow: cosmic::iced::Shadow::default(),
                    icon_color: Some(apply_alpha(surface.on.into(), 0.68)),
                    snap: true,
                }
            }),
    )
}

fn account_display_label<'a>(
    account: &'a ProviderAccountRuntimeState,
    snapshot: Option<&'a UsageSnapshot>,
) -> &'a str {
    snapshot
        .and_then(|snapshot| snapshot.identity.email.as_deref())
        .filter(|label| !label.trim().is_empty())
        .unwrap_or(account.label.as_str())
}

fn inactive_account_snapshot_time(
    account: &ProviderAccountRuntimeState,
    snapshot: &UsageSnapshot,
) -> DateTime<Utc> {
    account.last_success_at.unwrap_or(snapshot.updated_at)
}

fn usage_section(
    window: &UsageWindow,
    reset_time_format: ResetTimeFormat,
    usage_amount_format: UsageAmountFormat,
) -> Element<'static, Message> {
    let display = usage_display_data(
        window,
        reset_time_format,
        usage_amount_format,
        Utc::now(),
    );
    usage_block(
        window.label.clone(),
        display.percent,
        display.primary,
        display.secondary,
        display.pace,
        pace_marker_percent(display.pace, usage_amount_format),
    )
}

fn extra_section(
    window: &UsageWindow,
    cost: Option<&ProviderCost>,
    usage_amount_format: UsageAmountFormat,
) -> Element<'static, Message> {
    let cost_text = cost.map(format_cost);
    let display = usage_display_data(
        window,
        ResetTimeFormat::Relative,
        usage_amount_format,
        Utc::now(),
    );
    usage_block(
        window.label.clone(),
        display.percent,
        display.primary,
        cost_text,
        display.pace,
        pace_marker_percent(display.pace, usage_amount_format),
    )
}

fn cost_section(provider: ProviderId, cost: &ProviderCost) -> Element<'static, Message> {
    if provider == ProviderId::Codex {
        return credit_section(cost);
    }
    info_block(fl!("extra-label"), format_cost(cost), None)
}

fn format_cost(cost: &ProviderCost) -> String {
    match cost.limit {
        Some(limit) => format!(
            "{}{:.2} / {}{:.2}",
            cost.units, cost.used, cost.units, limit
        ),
        None => format!("{}{:.2} spent", cost.units, cost.used),
    }
}

fn credit_section(cost: &ProviderCost) -> Element<'static, Message> {
    let balance = if cost.used.fract() == 0.0 {
        format!("{:.0}", cost.used)
    } else {
        format!("{:.2}", cost.used)
    };

    card(
        column![
            widget::text(fl!("credits-label")).size(18),
            widget::text(fl!("credits-available", balance = balance.as_str())).size(14),
        ]
        .spacing(6),
    )
}

fn account_section(account_label: &str, plan_label: Option<&str>) -> Element<'static, Message> {
    let mut heading = row![
        widget::text(fl!("active-account-label")).size(18),
        cosmic::iced::widget::Space::new().width(Length::Fill),
    ]
    .align_y(Alignment::Center);

    if let Some(plan_label) = plan_label
        && !plan_label.trim().is_empty()
    {
        heading = heading.push(plan_badge(plan_label));
    }

    card(
        column![heading, account_label_text(account_label, 14)]
            .spacing(6)
            .width(Length::Fill),
    )
}

fn account_label_text(label: &str, size: u16) -> Element<'static, Message> {
    let truncated = truncate_account_label(label);
    let text = widget::text(truncated.clone())
        .size(size)
        .width(Length::Fill);
    if truncated == label {
        return text.into();
    }
    widget::tooltip::tooltip(
        text,
        widget::text(label.to_string()).size(12),
        widget::tooltip::Position::Top,
    )
    .into()
}

fn truncate_account_label(label: &str) -> String {
    let mut chars = label.chars();
    let truncated = chars
        .by_ref()
        .take(ACCOUNT_LABEL_MAX_CHARS)
        .collect::<String>();
    if chars.next().is_some() {
        return format!("{truncated}...");
    }
    truncated
}

fn usage_block(
    title: String,
    percent: f32,
    primary: String,
    secondary: Option<String>,
    pace: Option<usage_display::UsagePace>,
    pace_marker_percent: Option<f32>,
) -> Element<'static, Message> {
    let secondary = secondary.unwrap_or_default();
    let pct_row = row![
        widget::text(primary).size(14),
        cosmic::iced::widget::Space::new().width(Length::Fill),
        widget::text(secondary).size(13),
    ]
    .align_y(Alignment::Center);

    let col = column![
        widget::text(title).size(18),
        paced_progress_bar(
            percent,
            pace_marker_percent,
            pace.map(usage_display::pace_label)
        ),
        pct_row,
    ]
    .spacing(6);

    card(col)
}

fn usage_display_data(
    window: &UsageWindow,
    reset_time_format: ResetTimeFormat,
    usage_amount_format: UsageAmountFormat,
    now: DateTime<Utc>,
) -> UsageDisplayData {
    UsageDisplayData {
        percent: usage_display::displayed_amount_percent(window, now, usage_amount_format),
        primary: usage_display::usage_amount_label(window, now, usage_amount_format),
        secondary: usage_display::reset_label(window, now, reset_time_format),
        pace: usage_display::pace(window, now),
    }
}

fn muted_progress_bar(percent: f32) -> Element<'static, Message> {
    progress_bar(0.0..=100.0, percent)
        .length(Length::Fill)
        .girth(Length::Fixed(8.0))
        .class(cosmic::theme::ProgressBar::custom(|theme: &cosmic::Theme| {
            let cosmic = theme.cosmic();
            let surface = &cosmic.background.component;
            cosmic::iced::widget::progress_bar::Style {
                background: Background::Color(apply_alpha(surface.divider.into(), 0.35)),
                bar: Background::Color(apply_alpha(surface.on.into(), 0.28)),
                border: cosmic::iced::Border {
                    radius: 4.0.into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
            }
        }))
        .into()
}

fn paced_progress_bar(
    percent: f32,
    pace_marker_percent: Option<f32>,
    pace_label: Option<String>,
) -> Element<'static, Message> {
    let progress: Element<'static, Message> = progress_bar(0.0..=100.0, percent)
        .length(Length::Fill)
        .girth(Length::Fixed(8.0))
        .into();

    let bar = if let Some(marker_percent) = pace_marker_percent {
        cosmic::iced::widget::Stack::new()
            .push(progress)
            .push(pace_marker(marker_percent))
            .width(Length::Fill)
            .height(Length::Fixed(8.0))
            .into()
    } else {
        progress
    };

    if let Some(label) = pace_label {
        widget::tooltip::tooltip(
            bar,
            widget::text(label).size(12),
            widget::tooltip::Position::Top,
        )
        .into()
    } else {
        bar
    }
}

fn pace_marker_percent(
    pace: Option<usage_display::UsagePace>,
    usage_amount_format: UsageAmountFormat,
) -> Option<f32> {
    pace.map(|pace| match usage_amount_format {
        UsageAmountFormat::Used => pace.expected_percent,
        UsageAmountFormat::Left => 100.0 - pace.expected_percent,
    })
}

fn pace_marker(expected_percent: f32) -> Element<'static, Message> {
    let left = pace_marker_portion(expected_percent);
    let right = 1000 - left;
    row![
        cosmic::iced::widget::Space::new().width(Length::FillPortion(left)),
        container(cosmic::iced::widget::Space::new())
            .width(Length::Fixed(3.0))
            .height(Length::Fixed(8.0))
            .style(|theme: &cosmic::Theme| {
                let cosmic = theme.cosmic();
                widget::container::Style {
                    text_color: None,
                    background: Some(Background::Color(cosmic.accent.pressed.into())),
                    border: cosmic::iced::Border {
                        radius: 0.0.into(),
                        width: 0.0,
                        color: Color::TRANSPARENT,
                    },
                    shadow: cosmic::iced::Shadow::default(),
                    icon_color: None,
                    snap: true,
                }
            }),
        cosmic::iced::widget::Space::new().width(Length::FillPortion(right)),
    ]
    .width(Length::Fill)
    .height(Length::Fixed(8.0))
    .into()
}

fn pace_marker_portion(expected_percent: f32) -> u16 {
    let scaled = (expected_percent * 10.0).clamp(1.0, 999.0);
    let mut portion = 1u16;
    while portion < 999 && f32::from(portion) + 0.5 <= scaled {
        portion += 1;
    }
    portion
}

fn info_block(
    title: String,
    primary: String,
    secondary: Option<String>,
) -> Element<'static, Message> {
    let mut col = column![widget::text(title).size(18), widget::text(primary).size(14)].spacing(6);

    if let Some(secondary) = secondary {
        col = col.push(widget::text(secondary).size(13));
    }

    card(col)
}

fn selected_state(
    state: &AppState,
    selected_provider: ProviderId,
) -> Option<&ProviderRuntimeState> {
    state
        .providers
        .iter()
        .find(|p| p.provider == selected_provider && p.enabled)
        .or_else(|| state.providers.iter().find(|p| p.enabled))
}

fn tab_percent(state: &AppState, provider: &ProviderRuntimeState) -> f32 {
    active_snapshot(state, provider)
        .and_then(|s| s.headline_window())
        .map_or(0.0, |w| {
            usage_display::displayed_percent(w, chrono::Utc::now())
        })
}

fn provider_status_badge(
    state: &AppState,
    provider: &ProviderRuntimeState,
) -> Element<'static, Message> {
    if !provider.enabled {
        return badge_neutral(fl!("badge-disabled"));
    }
    if provider.is_refreshing {
        return badge_neutral(fl!("badge-refreshing"));
    }
    if provider.account_status == AccountSelectionStatus::LoginRequired
        || provider.account_status == AccountSelectionStatus::Unavailable
    {
        return badge_warning(fl!("badge-login-required"));
    }
    if provider.account_status == AccountSelectionStatus::SelectionRequired {
        return badge_warning(fl!("badge-select-required"));
    }
    let has_snapshot = active_snapshot(state, provider).is_some();
    match (
        active_health(state, provider),
        has_snapshot,
        provider_is_live(state, provider),
    ) {
        (Some(ProviderHealth::Ok), true, true) => badge_success(fl!("badge-live")),
        (_, true, _) => badge_warning(fl!("badge-stale")),
        (Some(ProviderHealth::Error), false, _) => badge_destructive(fl!("badge-error")),
        _ => badge_neutral(fl!("badge-loading")),
    }
}

fn cursor_account_requires_action(account: &ProviderAccountRuntimeState) -> bool {
    account.provider == ProviderId::Cursor && account.auth_state == AuthState::ActionRequired
}

fn provider_is_live(state: &AppState, provider: &ProviderRuntimeState) -> bool {
    if !provider.enabled
        || provider.is_refreshing
        || provider.account_status != AccountSelectionStatus::Ready
        || active_health(state, provider) != Some(ProviderHealth::Ok)
        || active_snapshot(state, provider).is_none()
    {
        return false;
    }
    let now = chrono::Utc::now();
    active_last_success_at(state, provider).is_some_and(|t| now - t < STALE_THRESHOLD)
}

fn active_snapshot<'a>(
    state: &'a AppState,
    provider: &'a ProviderRuntimeState,
) -> Option<&'a UsageSnapshot> {
    state
        .active_account(provider.provider)
        .and_then(|account| account.snapshot.as_ref())
        .or(provider.legacy_display_snapshot.as_ref())
}

fn active_health(state: &AppState, provider: &ProviderRuntimeState) -> Option<ProviderHealth> {
    state
        .active_account(provider.provider)
        .map(|account| account.health.clone())
}

fn active_last_success_at(
    state: &AppState,
    provider: &ProviderRuntimeState,
) -> Option<chrono::DateTime<chrono::Utc>> {
    state
        .active_account(provider.provider)
        .and_then(|account| account.last_success_at)
}

fn format_updated_label(last_success_at: chrono::DateTime<chrono::Utc>) -> String {
    let age = Utc::now() - last_success_at;
    if age.num_seconds() < 10 {
        fl!("updated-just-now")
    } else if age.num_minutes() < 1 {
        fl!("updated-seconds-ago", n = age.num_seconds())
    } else if age.num_hours() < 1 {
        fl!("updated-minutes-ago", n = age.num_minutes())
    } else if age.num_days() < 1 {
        fl!("updated-hours-ago", n = age.num_hours())
    } else {
        let date = last_success_at.format("%Y-%m-%d %H:%M").to_string();
        fl!("updated-at", date = date.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    fn strip_isolation_marks(s: &str) -> String {
        s.replace(['\u{2068}', '\u{2069}'], "")
    }

    #[test]
    fn usage_display_data_keeps_last_known_snapshot_value() {
        let snapshot_time = Utc.with_ymd_and_hms(2026, 4, 26, 11, 0, 0).unwrap();
        let window = UsageWindow {
            label: "Session".to_string(),
            used_percent: 61.2,
            reset_at: Some(snapshot_time + Duration::minutes(30)),
            window_seconds: None,
            reset_description: None,
        };

        let display = usage_display_data(
            &window,
            ResetTimeFormat::Relative,
            UsageAmountFormat::Used,
            snapshot_time,
        );

        assert!((display.percent - 61.2).abs() < 0.001);
        assert_eq!(
            display.secondary.as_deref().map(strip_isolation_marks),
            Some("Resets in 30m".to_string())
        );
    }
}
