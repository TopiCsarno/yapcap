// SPDX-License-Identifier: MPL-2.0

mod badges;
mod detail;
mod measure;
mod settings;

use self::badges::{
    account_label_text, apply_alpha, badge_destructive, badge_destructive_soft, badge_neutral,
    badge_neutral_soft, badge_success, badge_success_soft, badge_warning, badge_warning_soft,
    badge_with_tooltip, plan_badge,
};
use self::detail::{active_snapshot, provider_body_height_multi, selected_provider_view};
use self::measure::Measure;
use self::settings::{general_settings_view, provider_settings_view, settings_body_height};
use super::provider_assets::{provider_icon_handle, provider_icon_variant};
use crate::app::{Message, PopupRoute, SettingsRoute};
use crate::config::{Config, PanelIconStyle, ResetTimeFormat, UsageAmountFormat};
use crate::fl;
use crate::model::{
    AppState, AuthState, ProviderAccountRuntimeState, ProviderId, ProviderRuntimeState, UsageWindow,
};
use crate::providers::claude::{ClaudeLoginState, ClaudeLoginStatus};
use crate::providers::codex::{CodexLoginState, CodexLoginStatus};
use crate::providers::cursor::CursorScanState;
use crate::providers::interface::ProviderAccountActionSupport;
use crate::providers::registry;
use crate::updates::UpdateStatus;
use crate::usage_display;
use cosmic::Element;
use cosmic::iced::widget::{column, container, progress_bar, row, scrollable};
use cosmic::iced::{Alignment, Background, Color, Length, Size};
use cosmic::widget;

pub const POPUP_COLUMN_WIDTH: f32 = 420.0;
const POPUP_WIDTH: f32 = POPUP_COLUMN_WIDTH;
const POPUP_MAX_HEIGHT: f32 = 1080.0;
const POPUP_PADDING: f32 = 32.0;
const POPUP_CHROME_SPACING: f32 = 42.0;
const POPUP_HEADER_HEIGHT: f32 = 36.0;
const POPUP_TAB_HEIGHT: f32 = 68.0;
const POPUP_FOOTER_HEIGHT: f32 = 28.0;
const POPUP_BODY_PANEL_PADDING: f32 = 24.0;
const POPUP_BODY_BOTTOM_SLACK: f32 = 8.0;
const PROVIDER_CARD_SPACING: f32 = 8.0;
const PROVIDER_SUMMARY_HEIGHT: f32 = 58.0;
const PROVIDER_ACCOUNT_HEADER_HEIGHT: f32 = 96.0;
const PROVIDER_SECTION_HEIGHT: f32 = 84.0;
const PROVIDER_SECTION_WITH_ACTION_HEIGHT: f32 = 120.0;
const SETTINGS_SECTION_HEIGHT: f32 = 104.0;
const SETTINGS_PROVIDER_ROW_HEIGHT: f32 = 44.0;
const UPDATE_NOTIFICATION_DOT_COLOR: Color = Color::from_rgb(0.93, 0.11, 0.15);
const ACCENT_SOFT_FILL_ALPHA: f32 = 0.14;

#[derive(Clone, Copy)]
pub struct ProviderLoginStates<'a> {
    pub codex: Option<&'a CodexLoginState>,
    pub claude: Option<&'a ClaudeLoginState>,
    pub cursor_scan: &'a CursorScanState,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PopupBodyMeasureTarget {
    Provider(ProviderId),
    Settings(SettingsRoute),
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

    let body = popup_body_view(state, config, logins, selected, route, update_status);

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

    let body_panel: Element<'_, Message> = container(panel(scrollable(body).width(Length::Fill)))
        .width(Length::Fill)
        .height(Length::Fill)
        .into();

    let body_stack = popup_body_stack(state, config, logins, update_status, body_panel);

    let content = column![
        narrow_chrome(header),
        narrow_chrome(nav_row),
        body_stack,
        narrow_chrome(footer),
    ]
    .spacing(14)
    .padding(16)
    .width(Length::Fill)
    .height(Length::Fill);

    Element::from(content)
}

fn popup_body_view<'a>(
    state: &'a AppState,
    config: &'a Config,
    logins: ProviderLoginStates<'a>,
    selected: Option<&'a ProviderRuntimeState>,
    route: &'a PopupRoute,
    update_status: &'a UpdateStatus,
) -> Element<'a, Message> {
    match route {
        PopupRoute::ProviderDetail => selected_provider_view(selected, state, config),
        PopupRoute::Settings(SettingsRoute::General) => {
            general_settings_view(config, update_status)
        }
        PopupRoute::Settings(SettingsRoute::Provider(id)) => {
            provider_settings_view(state, config, logins, *id)
        }
    }
}

fn popup_body_stack<'a>(
    state: &'a AppState,
    config: &'a Config,
    logins: ProviderLoginStates<'a>,
    update_status: &'a UpdateStatus,
    body_panel: Element<'a, Message>,
) -> Element<'a, Message> {
    let mut stack = cosmic::iced::widget::Stack::new()
        .push(body_panel)
        .width(Length::Fill)
        .height(Length::Fill);

    for provider in state.providers.iter().filter(|provider| provider.enabled) {
        let provider_id = provider.provider;
        let width = selected_account_count(state, provider_id) * POPUP_WIDTH;
        let body = selected_provider_view(Some(provider), state, config);
        stack = stack.push(Measure::new(body, width, move |size| {
            Message::PopupBodyMeasured(PopupBodyMeasureTarget::Provider(provider_id), size)
        }));
    }

    let general = general_settings_view(config, update_status);
    stack = stack.push(Measure::new(general, POPUP_WIDTH, |size| {
        Message::PopupBodyMeasured(
            PopupBodyMeasureTarget::Settings(SettingsRoute::General),
            size,
        )
    }));

    for provider in ProviderId::ALL {
        let body = provider_settings_view(state, config, logins, provider);
        stack = stack.push(Measure::new(body, POPUP_WIDTH, move |size| {
            Message::PopupBodyMeasured(
                PopupBodyMeasureTarget::Settings(SettingsRoute::Provider(provider)),
                size,
            )
        }));
    }

    stack.into()
}

pub fn popup_max_width(state: &AppState) -> f32 {
    ProviderId::ALL
        .iter()
        .map(|&p| selected_account_count(state, p))
        .fold(1.0_f32, f32::max)
        * POPUP_WIDTH
}

pub fn popup_session_size(state: &AppState, selected_provider: ProviderId) -> Size {
    let n_cols = selected_account_count(state, selected_provider);
    let width = POPUP_WIDTH * n_cols;
    let provider_height = state
        .providers
        .iter()
        .filter(|provider| provider.enabled)
        .map(|provider| provider_body_height_multi(state, Some(provider)))
        .fold(PROVIDER_SUMMARY_HEIGHT, f32::max);
    let height = POPUP_PADDING
        + POPUP_CHROME_SPACING
        + POPUP_HEADER_HEIGHT
        + POPUP_TAB_HEIGHT
        + POPUP_FOOTER_HEIGHT
        + POPUP_BODY_PANEL_PADDING
        + POPUP_BODY_BOTTOM_SLACK
        + provider_height;

    Size::new(width, height.clamp(1.0, POPUP_MAX_HEIGHT))
}

pub fn popup_session_size_with_body_height(
    state: &AppState,
    selected_provider: ProviderId,
    body_height: f32,
) -> Size {
    let n_cols = selected_account_count(state, selected_provider);
    let width = POPUP_WIDTH * n_cols;
    Size::new(width, popup_total_height(body_height))
}

pub fn popup_settings_size(state: &AppState) -> Size {
    let height = POPUP_PADDING
        + POPUP_CHROME_SPACING
        + POPUP_HEADER_HEIGHT
        + POPUP_TAB_HEIGHT
        + POPUP_FOOTER_HEIGHT
        + POPUP_BODY_PANEL_PADDING
        + POPUP_BODY_BOTTOM_SLACK
        + settings_body_height(state);
    Size::new(POPUP_WIDTH, height.clamp(1.0, POPUP_MAX_HEIGHT))
}

pub fn popup_settings_size_with_body_height(body_height: f32) -> Size {
    Size::new(POPUP_WIDTH, popup_total_height(body_height))
}

fn popup_total_height(body_height: f32) -> f32 {
    let height = POPUP_PADDING
        + POPUP_CHROME_SPACING
        + POPUP_HEADER_HEIGHT
        + POPUP_TAB_HEIGHT
        + POPUP_FOOTER_HEIGHT
        + POPUP_BODY_PANEL_PADDING
        + POPUP_BODY_BOTTOM_SLACK
        + body_height;
    height.clamp(1.0, POPUP_MAX_HEIGHT)
}

fn selected_account_count(state: &AppState, provider: ProviderId) -> f32 {
    let n = state.display_selected_account_count(provider);
    f32::from(u8::try_from(n).unwrap_or(u8::MAX))
}

fn narrow_chrome<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(container(content.into()).width(Length::Fixed(POPUP_WIDTH)))
        .width(Length::Fill)
        .align_x(Alignment::Center)
        .into()
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

fn provider_tab(
    state: &AppState,
    provider: &ProviderRuntimeState,
    selected: bool,
) -> Element<'static, Message> {
    let percents = tab_percents(state, provider);
    let icon_variant = provider_icon_variant();
    let badge = widget::icon::icon(provider_icon_handle(provider.provider, icon_variant))
        .size(18)
        .width(Length::Fixed(18.0))
        .height(Length::Fixed(18.0));
    let label = widget::text(provider.provider.label()).size(12);
    let bars = percents.into_iter().fold(column![].spacing(4), |col, pct| {
        col.push(
            progress_bar(0.0..=100.0, pct)
                .length(Length::Fill)
                .girth(Length::Fixed(4.0)),
        )
    });

    let content = container(
        column![badge, label, bars]
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

fn provider_summary(provider: &ProviderRuntimeState) -> Element<'static, Message> {
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

    card(title)
}

fn info_block(
    title: String,
    primary: String,
    secondary: Option<String>,
    action: Option<Element<'static, Message>>,
) -> Element<'static, Message> {
    let mut col = column![widget::text(title).size(18), widget::text(primary).size(14)].spacing(6);

    if let Some(secondary) = secondary {
        col = col.push(widget::text(secondary).size(13));
    }

    if let Some(action) = action {
        col = col.push(action);
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

fn tab_percents(state: &AppState, provider: &ProviderRuntimeState) -> Vec<f32> {
    let now = chrono::Utc::now();
    let accounts = state.display_selected_accounts(provider.provider);
    if accounts.is_empty() {
        let pct = active_snapshot(state, provider)
            .and_then(|s| s.headline_window())
            .map_or(0.0, |w| usage_display::displayed_percent(w, now));
        return vec![pct];
    }
    accounts
        .into_iter()
        .map(|account| {
            account
                .snapshot
                .as_ref()
                .and_then(|s| s.headline_window())
                .map_or(0.0, |w| usage_display::displayed_percent(w, now))
        })
        .collect()
}

pub(super) fn cursor_account_requires_action(account: &ProviderAccountRuntimeState) -> bool {
    account.provider == ProviderId::Cursor && account.auth_state == AuthState::ActionRequired
}
