// SPDX-License-Identifier: MPL-2.0

mod badges;
mod detail;
mod settings;

use self::badges::{
    account_label_text, apply_alpha, badge_accent, badge_destructive, badge_destructive_soft,
    badge_neutral, badge_neutral_soft, badge_success, badge_success_soft, badge_warning,
    badge_warning_soft, badge_with_tooltip, disabled_account_label_text, plan_badge,
};
use self::detail::{empty_state_view, selected_provider_view};
use self::settings::{
    about_view, general_settings_view, manage_providers_view, provider_settings_view,
};
use super::provider_assets::{provider_icon_handle, provider_icon_variant};
use crate::app::{Message, PopupRoute};
use crate::config::{Config, PanelIconStyle, ResetTimeFormat, UsageAmountFormat};
use crate::detection::DetectionSnapshot;
use crate::fl;
use crate::model::{AppState, ProviderId, ProviderRuntimeState, UsageWindow};
use crate::providers::antigravity::{AntigravityLoginState, AntigravityLoginStatus};
use crate::providers::claude::{ClaudeLoginState, ClaudeLoginStatus};
use crate::providers::codex::{CodexLoginState, CodexLoginStatus};
use crate::providers::copilot::{CopilotLoginState, CopilotLoginStatus};
use crate::providers::cursor::CursorScanState;
use crate::providers::gemini::{GeminiLoginState, GeminiLoginStatus};
use crate::providers::kimi::login::KimiLoginState;
use crate::providers::minimax::MinimaxLoginState;
use crate::updates::UpdateStatus;
use crate::usage_display;
use cosmic::Element;
use cosmic::iced::widget::{column, container, progress_bar, row, scrollable};
use cosmic::iced::{Alignment, Background, Color, ContentFit, Length};
use cosmic::widget;

const POPUP_TAB_HEIGHT: f32 = 48.0;
pub(crate) const PROVIDER_VIEWPORT_SIZE: usize = 6;
const PROVIDER_CARD_SPACING: f32 = 8.0;
const PROVIDER_GROUP_PADDING: f32 = 24.0;
const PROVIDER_GROUP_SPACING: f32 = 12.0;
const HEADER_ICON_BUTTON_SIZE: f32 = 28.0;
const UPDATE_NOTIFICATION_DOT_COLOR: Color = Color::from_rgb(1.0, 0.31, 0.37);
const UPDATE_NOTIFICATION_DOT_SIZE: f32 = 12.0;
const ACCENT_SOFT_FILL_ALPHA: f32 = 0.14;
const COMPONENT_SURFACE_ALPHA: f32 = 0.40;

#[derive(Clone, Copy)]
pub struct ProviderLoginStates<'a> {
    pub codex: Option<&'a CodexLoginState>,
    pub claude: Option<&'a ClaudeLoginState>,
    pub cursor_scan: &'a CursorScanState,
    pub gemini: Option<&'a GeminiLoginState>,
    pub copilot: Option<&'a CopilotLoginState>,
    pub minimax: Option<&'a MinimaxLoginState>,
    pub kimi: Option<&'a KimiLoginState>,
    pub antigravity: Option<&'a AntigravityLoginState>,
    pub opencode_go: Option<&'a crate::providers::opencode_go::login::OpenCodeGoLoginState>,
    pub grok: Option<&'a crate::providers::grok::GrokLoginState>,
}

#[derive(Clone, Copy)]
pub struct DetailSelection {
    pub provider: ProviderId,
    pub account_page: usize,
    pub provider_viewport_offset: usize,
}

pub fn popup_content<'a>(
    state: &'a AppState,
    config: &'a Config,
    detection: &'a DetectionSnapshot,
    logins: ProviderLoginStates<'a>,
    selection: DetailSelection,
    route: &'a PopupRoute,
    update_status: &'a UpdateStatus,
) -> Element<'a, Message> {
    let empty_state = popup_empty_state_active(state);

    let header = popup_header(route, empty_state, update_status);

    let nav_row: Option<Element<'_, Message>> = match route {
        PopupRoute::ProviderDetail if empty_state => None,
        PopupRoute::ProviderDetail => (enabled_provider_count(state) > 1).then(|| {
            provider_tab_rows(
                state,
                selection.provider,
                selection.provider_viewport_offset,
            )
        }),
        PopupRoute::Settings
        | PopupRoute::ManageProviders
        | PopupRoute::ManageAccounts(_)
        | PopupRoute::About => None,
    };

    let body = popup_body_view(
        state,
        config,
        detection,
        logins,
        selection,
        route,
        update_status,
    );

    let body = popup_body_container(route, body);
    let body_panel: Element<'_, Message> = container(panel(body)).width(Length::Fill).into();

    let mut chrome = column![narrow_chrome(header)].spacing(14);
    if let Some(nav_row) = nav_row {
        chrome = chrome.push(narrow_chrome(nav_row));
    }
    let body_spacing = if matches!(route, PopupRoute::ProviderDetail) {
        6
    } else {
        14
    };
    let content = column![chrome, body_panel]
        .spacing(body_spacing)
        .padding(16)
        .width(Length::Fill);

    Element::from(content)
}

fn popup_body_view<'a>(
    state: &'a AppState,
    config: &'a Config,
    detection: &'a DetectionSnapshot,
    logins: ProviderLoginStates<'a>,
    selection: DetailSelection,
    route: &'a PopupRoute,
    update_status: &'a UpdateStatus,
) -> Element<'a, Message> {
    match route {
        PopupRoute::ProviderDetail if popup_empty_state_active(state) => empty_state_view(),
        PopupRoute::ProviderDetail => selected_provider_view(
            selected_state(state, selection.provider),
            state,
            config,
            detection,
            selection.account_page,
        ),
        PopupRoute::Settings => general_settings_view(config),
        PopupRoute::ManageProviders => manage_providers_view(state),
        PopupRoute::ManageAccounts(id) => {
            provider_settings_view(state, config, detection, logins, *id)
        }
        PopupRoute::About => about_view(update_status),
    }
}

fn popup_body_container<'a>(
    route: &PopupRoute,
    body: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let body = body.into();
    if popup_body_is_scrollable(route) {
        scrollable(body)
            .width(Length::Fill)
            .height(Length::Shrink)
            .into()
    } else {
        body
    }
}

fn popup_body_is_scrollable(route: &PopupRoute) -> bool {
    !matches!(route, PopupRoute::Settings | PopupRoute::About)
}

pub(super) fn popup_empty_state_active(state: &AppState) -> bool {
    state.providers.iter().all(|provider| !provider.enabled)
}

pub(super) fn detected_without_accounts(
    state: &AppState,
    detection: &DetectionSnapshot,
    provider: ProviderId,
) -> bool {
    detection.detected(provider) && state.accounts_for(provider).is_empty()
}

pub(crate) fn account_page_next(page: usize, account_count: usize) -> usize {
    if account_count == 0 {
        0
    } else {
        (page + 1) % account_count
    }
}

pub(crate) fn account_page_previous(page: usize, account_count: usize) -> usize {
    if account_count == 0 {
        0
    } else if page == 0 {
        account_count - 1
    } else {
        page - 1
    }
}

pub(crate) fn clamp_account_page(page: usize, account_count: usize) -> usize {
    if account_count == 0 {
        0
    } else {
        page.min(account_count - 1)
    }
}

pub(crate) fn pager_account_label(page: usize, label: &str) -> String {
    if label.trim().is_empty() {
        fl!(
            "account-pager-fallback",
            n = i64::try_from(page + 1).unwrap_or(i64::MAX)
        )
    } else {
        label.to_string()
    }
}

fn narrow_chrome<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(container(content.into()).width(Length::Fill))
        .width(Length::Fill)
        .align_x(Alignment::Center)
        .into()
}

fn panel<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Element::from(container(content).width(Length::Fill).padding(12))
}

fn popup_header(
    route: &PopupRoute,
    empty_state: bool,
    update_status: &UpdateStatus,
) -> Element<'static, Message> {
    if !matches!(route, PopupRoute::ProviderDetail) {
        return widget::button::text(fl!("back"))
            .leading_icon(widget::icon::from_name("go-previous-symbolic"))
            .class(back_button_class())
            .on_press(Message::NavigateTo(PopupRoute::ProviderDetail))
            .into();
    }

    let about = header_icon_button(
        "dialog-information-symbolic",
        Message::NavigateTo(PopupRoute::About),
        true,
    );
    let about: Element<'static, Message> = if update_available(update_status) {
        cosmic::iced::widget::stack![
            about,
            container(notification_dot())
                .width(Length::Fixed(HEADER_ICON_BUTTON_SIZE))
                .height(Length::Fixed(HEADER_ICON_BUTTON_SIZE))
                .align_x(cosmic::iced::alignment::Horizontal::Right)
                .align_y(cosmic::iced::alignment::Vertical::Top),
        ]
        .width(Length::Fixed(HEADER_ICON_BUTTON_SIZE))
        .height(Length::Fixed(HEADER_ICON_BUTTON_SIZE))
        .into()
    } else {
        about
    };
    let mut actions = row![
        widget::tooltip::tooltip(
            header_icon_button(
                "view-list-symbolic",
                Message::NavigateTo(PopupRoute::ManageProviders),
                false,
            ),
            widget::text(fl!("manage-providers-tooltip")).size(12),
            widget::tooltip::Position::Top,
        ),
        widget::tooltip::tooltip(
            header_icon_button(
                "preferences-system-symbolic",
                Message::NavigateTo(PopupRoute::Settings),
                false,
            ),
            widget::text(fl!("settings-tooltip")).size(12),
            widget::tooltip::Position::Top,
        ),
    ]
    .spacing(7)
    .align_y(Alignment::Center);
    if !empty_state {
        actions = actions.push(widget::tooltip::tooltip(
            header_icon_button("view-refresh-symbolic", Message::RefreshNow, false),
            widget::text(fl!("refresh-now")).size(12),
            widget::tooltip::Position::Top,
        ));
    }

    let header = row![
        widget::tooltip::tooltip(
            about,
            widget::text(fl!("about-tooltip")).size(12),
            widget::tooltip::Position::Top,
        ),
        cosmic::iced::widget::Space::new().width(Length::Fill),
        actions,
    ]
    .align_y(Alignment::Center)
    .spacing(12);

    header.into()
}

fn header_icon_button(
    icon_name: &'static str,
    message: Message,
    inverted: bool,
) -> Element<'static, Message> {
    widget::button::icon(widget::icon::from_name(icon_name))
        .extra_small()
        .width(Length::Fixed(HEADER_ICON_BUTTON_SIZE))
        .height(Length::Fixed(HEADER_ICON_BUTTON_SIZE))
        .padding(2)
        .class(header_icon_button_class(inverted))
        .on_press(message)
        .into()
}

fn header_icon_button_class(inverted: bool) -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(move |_focused, theme| header_icon_button_style(theme, false, inverted)),
        disabled: Box::new(move |theme| header_icon_button_style(theme, false, inverted)),
        hovered: Box::new(move |_focused, theme| header_icon_button_style(theme, true, inverted)),
        pressed: Box::new(move |_focused, theme| header_icon_button_style(theme, true, inverted)),
    }
}

fn header_icon_button_style(
    theme: &cosmic::Theme,
    hovered: bool,
    inverted: bool,
) -> widget::button::Style {
    let cosmic = theme.cosmic();
    let mut style = widget::button::Style::new();
    let icon_color = if inverted || hovered {
        component_on_color(theme)
    } else {
        apply_alpha(component_on_color(theme), 0.45)
    };
    style.border_radius = cosmic.corner_radii.radius_s.into();
    style.icon_color = Some(icon_color);
    style.text_color = style.icon_color;
    style
}

fn back_button_class() -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(|_focused, theme| back_button_style(theme)),
        disabled: Box::new(back_button_style),
        hovered: Box::new(|_focused, theme| back_button_style(theme)),
        pressed: Box::new(|_focused, theme| back_button_style(theme)),
    }
}

fn back_button_style(theme: &cosmic::Theme) -> widget::button::Style {
    let cosmic = theme.cosmic();
    let mut style = widget::button::Style::new();
    style.text_color = Some(Color::WHITE);
    style.icon_color = Some(Color::WHITE);
    style.border_radius = cosmic.corner_radii.radius_s.into();
    style
}

fn card<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Element::from(container(content).width(Length::Fill).padding(8))
}

pub(super) fn component_container_style(theme: &cosmic::Theme) -> widget::container::Style {
    let cosmic = theme.cosmic();
    let surface = &cosmic.background(theme.transparent).component;
    widget::container::Style {
        text_color: Some(surface.on.into()),
        background: component_container_background(theme),
        border: cosmic::iced::Border {
            radius: cosmic.corner_radii.radius_s.into(),
            width: 1.0,
            color: surface.divider.into(),
        },
        shadow: cosmic::iced::Shadow::default(),
        icon_color: Some(surface.on.into()),
        snap: true,
    }
}

pub(super) fn component_container_background(theme: &cosmic::Theme) -> Option<Background> {
    Some(Background::Color(component_surface_color(theme)))
}

pub(super) fn component_card_background(theme: &cosmic::Theme) -> Background {
    Background::Color(component_surface_color(theme))
}

pub(super) fn component_surface_color(theme: &cosmic::Theme) -> Color {
    let mut color: Color = theme
        .cosmic()
        .background(theme.transparent)
        .component
        .base
        .into();
    if theme.transparent {
        color.a = COMPONENT_SURFACE_ALPHA;
    }
    color
}

pub(super) fn component_divider_color(theme: &cosmic::Theme) -> Color {
    theme
        .cosmic()
        .background(theme.transparent)
        .component
        .divider
        .into()
}

pub(super) fn component_hover_color(theme: &cosmic::Theme) -> Color {
    theme
        .cosmic()
        .background(theme.transparent)
        .component
        .hover
        .into()
}

pub(super) fn component_selected_color(theme: &cosmic::Theme) -> Color {
    theme
        .cosmic()
        .background(theme.transparent)
        .component
        .selected
        .into()
}

pub(super) fn component_on_color(theme: &cosmic::Theme) -> Color {
    theme
        .cosmic()
        .background(theme.transparent)
        .component
        .on
        .into()
}

pub(super) fn account_action_button(
    label: String,
    press: Option<Message>,
) -> Element<'static, Message> {
    widget::button::custom(
        row![
            widget::text(label),
            cosmic::iced::widget::Space::new().width(Length::Fill),
            widget::icon::from_name("go-next-symbolic").icon().size(16),
        ]
        .align_y(Alignment::Center)
        .width(Length::Fill),
    )
    .width(Length::Fill)
    .padding([4, 0])
    .class(account_action_button_class())
    .on_press_maybe(press)
    .into()
}

pub(super) fn account_add_button(
    label: String,
    press: Option<Message>,
) -> Element<'static, Message> {
    account_action_button_with_icon(
        account_action_icon(widget::text("+").size(22).into(), true, press.is_some()),
        label,
        press,
    )
}

pub(super) fn account_import_button(
    label: String,
    press: Option<Message>,
) -> Element<'static, Message> {
    account_action_button_with_icon(
        account_action_icon(
            widget::icon::from_name("go-up-symbolic")
                .icon()
                .size(18)
                .into(),
            false,
            press.is_some(),
        ),
        label,
        press,
    )
}

fn account_action_button_with_icon(
    icon: Element<'static, Message>,
    label: String,
    press: Option<Message>,
) -> Element<'static, Message> {
    let copy = widget::text(label).size(14);

    let content = row![
        icon,
        copy,
        cosmic::iced::widget::Space::new().width(Length::Fill),
        widget::icon::from_name("go-next-symbolic").icon().size(18),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    widget::button::custom(content)
        .width(Length::Fill)
        .padding([8, 12])
        .class(account_action_card_class(press.is_some()))
        .on_press_maybe(press)
        .into()
}

fn account_action_icon(
    icon: Element<'static, Message>,
    accent: bool,
    enabled: bool,
) -> Element<'static, Message> {
    container(icon)
        .width(Length::Fixed(32.0))
        .height(Length::Fixed(32.0))
        .align_x(cosmic::iced::alignment::Horizontal::Center)
        .align_y(cosmic::iced::alignment::Vertical::Center)
        .style(move |theme| {
            let cosmic = theme.cosmic();
            let opacity = if enabled { 1.0 } else { 0.45 };
            let color = if accent {
                apply_alpha(cosmic.accent.base.into(), opacity)
            } else {
                apply_alpha(component_on_color(theme), opacity)
            };
            let background = if accent {
                apply_alpha(cosmic.accent.base.into(), 0.18 * opacity)
            } else {
                apply_alpha(component_on_color(theme), 0.12 * opacity)
            };
            widget::container::Style {
                text_color: Some(color),
                background: Some(Background::Color(background)),
                border: cosmic::iced::Border {
                    radius: 16.0.into(),
                    width: 0.0,
                    color,
                },
                shadow: cosmic::iced::Shadow::default(),
                icon_color: Some(color),
                snap: true,
            }
        })
        .into()
}

fn account_action_card_class(enabled: bool) -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(move |_focused, theme| account_action_card_style(theme, enabled, false)),
        disabled: Box::new(move |theme| account_action_card_style(theme, false, false)),
        hovered: Box::new(move |_focused, theme| account_action_card_style(theme, enabled, true)),
        pressed: Box::new(move |_focused, theme| account_action_card_style(theme, enabled, true)),
    }
}

fn account_action_card_style(
    theme: &cosmic::Theme,
    enabled: bool,
    hovered: bool,
) -> widget::button::Style {
    let cosmic = theme.cosmic();
    let opacity = if enabled { 1.0 } else { 0.45 };
    let mut style = widget::button::Style::new();
    style.background = if hovered {
        Some(Background::Color(component_hover_color(theme)))
    } else {
        component_container_background(theme)
    };
    style.border_radius = cosmic.corner_radii.radius_s.into();
    style.border_width = 1.0;
    style.border_color = apply_alpha(component_divider_color(theme), opacity);
    style.text_color = Some(apply_alpha(component_on_color(theme), opacity));
    style.icon_color = Some(apply_alpha(component_on_color(theme), opacity));
    style
}

fn account_action_button_class() -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(|_focused, theme| account_action_button_style(theme, false)),
        disabled: Box::new(|theme| account_action_button_style(theme, false)),
        hovered: Box::new(|_focused, theme| account_action_button_style(theme, true)),
        pressed: Box::new(|_focused, theme| account_action_button_style(theme, true)),
    }
}

fn account_action_button_style(theme: &cosmic::Theme, hovered: bool) -> widget::button::Style {
    let cosmic = theme.cosmic();
    let mut style = widget::button::Style::new();
    style.background = hovered.then(|| Background::Color(component_hover_color(theme)));
    style.border_radius = cosmic.corner_radii.radius_s.into();
    style.text_color = Some(component_on_color(theme));
    style.icon_color = Some(component_on_color(theme));
    style
}

fn accent_selection_fill(theme: &cosmic::Theme) -> Color {
    let cosmic = theme.cosmic();
    apply_alpha(cosmic.accent.base.into(), ACCENT_SOFT_FILL_ALPHA)
}

fn provider_tab_selection_fill(theme: &cosmic::Theme) -> Color {
    apply_alpha(component_on_color(theme), 0.12)
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
        let mut style = component_container_style(theme);
        style.text_color = Some(apply_alpha(component_on_color(theme), 0.45));
        style.background = Some(Background::Color(apply_alpha(
            component_surface_color(theme),
            0.45,
        )));
        style.border.color = apply_alpha(component_divider_color(theme), 0.45);
        style.icon_color = Some(apply_alpha(component_on_color(theme), 0.45));
        style
    }))
}

fn update_available(update_status: &UpdateStatus) -> bool {
    matches!(update_status, UpdateStatus::UpdateAvailable { .. })
}

fn notification_dot() -> Element<'static, Message> {
    Element::from(
        container(
            cosmic::iced::widget::Space::new()
                .width(Length::Fixed(UPDATE_NOTIFICATION_DOT_SIZE))
                .height(Length::Fixed(UPDATE_NOTIFICATION_DOT_SIZE)),
        )
        .style(|theme: &cosmic::Theme| {
            let cosmic = theme.cosmic();
            widget::container::Style {
                text_color: None,
                background: Some(Background::Color(UPDATE_NOTIFICATION_DOT_COLOR)),
                border: cosmic::iced::Border {
                    radius: (UPDATE_NOTIFICATION_DOT_SIZE / 2.0).into(),
                    width: 2.0,
                    color: cosmic.background(theme.transparent).base.into(),
                },
                shadow: cosmic::iced::Shadow {
                    color: apply_alpha(UPDATE_NOTIFICATION_DOT_COLOR, 0.80),
                    offset: cosmic::iced::Vector::new(0.0, 0.0),
                    blur_radius: 7.0,
                },
                icon_color: None,
                snap: true,
            }
        }),
    )
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

fn provider_tab_rows(
    state: &AppState,
    selected_provider: ProviderId,
    offset: usize,
) -> Element<'static, Message> {
    let providers: Vec<_> = state
        .providers
        .iter()
        .filter(|provider| provider.enabled)
        .map(|provider| provider.provider)
        .collect();
    let offset = offset.min(provider_viewport_max_offset(providers.len()));
    let visible_providers = provider_viewport(&providers, offset);
    let (side_portion, provider_portion) = provider_viewport_portions(providers.len());
    let mut tabs = row![].width(Length::Fill).align_y(Alignment::Center);
    if side_portion > 0 {
        tabs = tabs.push(provider_viewport_spacer(side_portion));
    }
    for provider in visible_providers {
        tabs = tabs.push(provider_viewport_tab(
            *provider,
            *provider == selected_provider,
            provider_portion,
        ));
    }
    if side_portion > 0 {
        tabs = tabs.push(provider_viewport_spacer(side_portion));
    } else {
        for _ in visible_providers.len()..PROVIDER_VIEWPORT_SIZE {
            tabs = tabs.push(provider_viewport_spacer(1));
        }
    }

    if provider_viewport_navigation_visible(providers.len()) {
        let previous =
            widget::button::icon(widget::icon::from_name("go-previous-symbolic")).extra_small();
        let previous = if offset == 0 {
            previous
        } else {
            previous.on_press(Message::PageProviderViewport(
                crate::app::PagerDirection::Previous,
            ))
        };
        let next = widget::button::icon(widget::icon::from_name("go-next-symbolic")).extra_small();
        let next = if offset >= provider_viewport_max_offset(providers.len()) {
            next
        } else {
            next.on_press(Message::PageProviderViewport(
                crate::app::PagerDirection::Next,
            ))
        };

        row![previous, tabs, next]
            .spacing(6)
            .width(Length::Fill)
            .align_y(Alignment::Center)
            .into()
    } else {
        tabs.into()
    }
}

pub(crate) fn provider_viewport(providers: &[ProviderId], offset: usize) -> &[ProviderId] {
    let start = offset.min(provider_viewport_max_offset(providers.len()));
    let end = (start + PROVIDER_VIEWPORT_SIZE).min(providers.len());
    &providers[start..end]
}

fn provider_viewport_max_offset(provider_count: usize) -> usize {
    provider_count.saturating_sub(PROVIDER_VIEWPORT_SIZE)
}

pub(crate) fn provider_viewport_navigation_visible(provider_count: usize) -> bool {
    provider_viewport_max_offset(provider_count) > 0
}

fn provider_viewport_portions(provider_count: usize) -> (u16, u16) {
    if provider_count < PROVIDER_VIEWPORT_SIZE {
        ((PROVIDER_VIEWPORT_SIZE - provider_count) as u16, 2)
    } else {
        (0, 1)
    }
}

pub(crate) fn provider_viewport_max_offset_for(state: &AppState) -> usize {
    provider_viewport_max_offset(enabled_provider_count(state))
}

fn enabled_provider_count(state: &AppState) -> usize {
    state
        .providers
        .iter()
        .filter(|provider| provider.enabled)
        .count()
}

fn provider_viewport_tab(
    provider: ProviderId,
    selected: bool,
    portion: u16,
) -> Element<'static, Message> {
    let icon_variant = provider_icon_variant();
    let icon = widget::icon::icon(provider_icon_handle(provider, icon_variant))
        .size(22)
        .width(Length::Fixed(22.0))
        .height(Length::Fixed(22.0))
        .content_fit(ContentFit::Contain);
    let icon = container(icon)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);
    let baseline_height = provider_viewport_baseline_height(selected);

    column![
        widget::button::custom(icon)
            .class(provider_tab_class(selected))
            .width(Length::FillPortion(portion))
            .height(Length::Fixed(POPUP_TAB_HEIGHT - baseline_height))
            .on_press(Message::SelectProvider(provider)),
        provider_viewport_baseline(selected),
    ]
    .width(Length::FillPortion(portion))
    .into()
}

fn provider_viewport_spacer(portion: u16) -> Element<'static, Message> {
    let baseline_height = provider_viewport_baseline_height(false);
    column![
        cosmic::iced::widget::Space::new()
            .height(Length::Fixed(POPUP_TAB_HEIGHT - baseline_height)),
        provider_viewport_baseline(false),
    ]
    .width(Length::FillPortion(portion))
    .into()
}

fn provider_viewport_baseline_height(selected: bool) -> f32 {
    if selected { 4.0 } else { 2.0 }
}

fn provider_viewport_baseline(selected: bool) -> Element<'static, Message> {
    container(
        cosmic::iced::widget::Space::new()
            .height(Length::Fixed(provider_viewport_baseline_height(selected))),
    )
    .width(Length::Fill)
    .style(move |theme: &cosmic::Theme| {
        let cosmic = theme.cosmic();
        let color = if selected {
            component_on_color(theme)
        } else {
            component_divider_color(theme)
        };
        widget::container::Style {
            text_color: None,
            background: Some(Background::Color(color)),
            border: cosmic::iced::Border {
                radius: cosmic.corner_radii.radius_xl.into(),
                ..Default::default()
            },
            shadow: cosmic::iced::Shadow::default(),
            icon_color: None,
            snap: true,
        }
    })
    .into()
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

    let background = if selected {
        if interaction.pressed {
            Some(component_divider_color(theme))
        } else {
            Some(provider_tab_selection_fill(theme))
        }
    } else if interaction.pressed {
        Some(component_divider_color(theme))
    } else if interaction.hovered {
        Some(component_hover_color(theme))
    } else {
        None
    };

    style.background = background.map(|color| Background::Color(apply_alpha(color, opacity)));
    let radius = cosmic.corner_radii.radius_s;
    style.border_radius = cosmic::iced::border::Radius {
        top_left: radius[0],
        top_right: radius[1],
        bottom_right: 0.0,
        bottom_left: 0.0,
    };
    style.border_width = 0.0;
    style.border_color = Color::TRANSPARENT;
    style.outline_width = if interaction.focused && selected {
        1.0
    } else {
        0.0
    };
    style.outline_color = cosmic.accent.base.into();
    style.text_color = Some(apply_alpha(component_on_color(theme), opacity));
    style.icon_color = Some(apply_alpha(component_on_color(theme), opacity));

    style
}

fn provider_summary(
    provider: &ProviderRuntimeState,
    detected_without_accounts: bool,
) -> Element<'static, Message> {
    let mut title = row![
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

    if detected_without_accounts {
        title = title.push(badge_accent(fl!("provider-detected-chip")));
    }

    card(title)
}

fn info_block(
    title: String,
    primary: String,
    secondary: Option<String>,
    action: Option<Element<'static, Message>>,
) -> Element<'static, Message> {
    let mut col = column![widget::text(title).size(15), widget::text(primary).size(14)].spacing(6);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ProviderAccountRuntimeState;

    #[test]
    fn partial_provider_viewport_is_centered_with_equal_side_space() {
        for provider_count in 2..=5 {
            let (side_portion, provider_portion) = provider_viewport_portions(provider_count);

            assert_eq!(
                side_portion * 2 + provider_portion * provider_count as u16,
                12
            );
        }
    }

    #[test]
    fn empty_state_is_active_without_enabled_provider_tabs() {
        let mut state = AppState::empty();
        for provider in ProviderId::ALL {
            state.provider_mut(provider).unwrap().enabled = false;
        }

        assert!(popup_empty_state_active(&state));
        state.provider_mut(ProviderId::Codex).unwrap().enabled = true;

        assert!(!popup_empty_state_active(&state));
    }

    #[test]
    fn detected_settings_hint_ignores_explicit_disablement_but_hides_after_account_added() {
        let home = tempfile::tempdir().expect("create temporary home");
        std::fs::create_dir(home.path().join(".codex")).expect("create Codex marker");
        let detection = crate::detection::detect(home.path());
        let mut state = AppState::empty();
        state.provider_mut(ProviderId::Codex).unwrap().enabled = false;

        assert!(detected_without_accounts(
            &state,
            &detection,
            ProviderId::Codex
        ));

        state
            .provider_accounts
            .push(ProviderAccountRuntimeState::empty(
                ProviderId::Codex,
                "codex-test",
                "test@example.com",
            ));
        assert!(!detected_without_accounts(
            &state,
            &detection,
            ProviderId::Codex
        ));
    }

    #[test]
    fn component_containers_follow_popup_transparency() {
        let opaque = cosmic::Theme::dark();
        let mut transparent = opaque.clone();
        transparent.transparent = true;

        assert!(component_container_style(&opaque).background.is_some());
        let Some(Background::Color(background)) =
            component_container_style(&transparent).background
        else {
            panic!("component container background must be a solid color")
        };
        assert_eq!(background.a, COMPONENT_SURFACE_ALPHA);
    }

    #[test]
    fn component_cards_use_a_transparent_surface_layer() {
        let mut theme = cosmic::Theme::dark();
        theme.transparent = true;

        let Background::Color(card) = component_card_background(&theme) else {
            panic!("component card background must be a solid color")
        };
        let popup = theme.cosmic().background(true).base;

        let composite_alpha = popup.alpha + card.a * (1.0 - popup.alpha);

        assert_eq!(card.a, COMPONENT_SURFACE_ALPHA);
        assert!(composite_alpha > popup.alpha);
        assert!(composite_alpha < 1.0);
    }
}
