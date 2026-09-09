use super::super::super::{
    Alignment, Background, Element, Length, Message, ProviderId, accent_selection_fill,
    account_label_text, apply_alpha, badge_destructive, badge_destructive_soft, badge_neutral,
    badge_neutral_soft, badge_success, badge_success_soft, badge_warning, badge_warning_soft,
    badge_with_tooltip, component_container_background, component_container_style,
    component_divider_color, component_on_color, container, disabled_account_label_text, fl, row,
    widget,
};
use crate::providers::interface::{
    ProviderAccountAction, ProviderAccountFacts, ProviderAccountStatus, ProviderAccountStatusKind,
};

#[derive(Clone, Copy)]
pub(super) struct AccountRowPosition {
    pub(super) first: bool,
    pub(super) last: bool,
}

fn status_badge(status: &ProviderAccountStatus, enabled: bool) -> Element<'static, Message> {
    let badge = match (status.kind, enabled) {
        (ProviderAccountStatusKind::Warning, true) => badge_warning(status.badge_text.clone()),
        (ProviderAccountStatusKind::Warning, false) => {
            badge_warning_soft(status.badge_text.clone())
        }
        (ProviderAccountStatusKind::Neutral, true) => badge_neutral(status.badge_text.clone()),
        (ProviderAccountStatusKind::Neutral, false) => {
            badge_neutral_soft(status.badge_text.clone())
        }
        (ProviderAccountStatusKind::Destructive, true) => {
            badge_destructive(status.badge_text.clone())
        }
        (ProviderAccountStatusKind::Destructive, false) => {
            badge_destructive_soft(status.badge_text.clone())
        }
    };
    badge_with_tooltip(badge, status.tooltip_text.clone())
}

pub(super) fn account_settings_row(
    provider: ProviderId,
    account: &ProviderAccountFacts,
    selected_ids: &[&str],
    active_id: Option<&str>,
    enabled: bool,
    position: AccountRowPosition,
) -> Element<'static, Message> {
    let is_selected = selected_ids.contains(&account.account_id.as_str());
    let is_active = active_id == Some(account.account_id.as_str());
    let can_reauthenticate = action_available(
        account,
        ProviderAccountAction::Reauthenticate,
        enabled,
        account.status.as_ref(),
    );
    let can_restore_from_opencode = action_available(
        account,
        ProviderAccountAction::RestoreFromOpenCode,
        enabled,
        account.status.as_ref(),
    );
    let can_restore_from_grok = action_available(
        account,
        ProviderAccountAction::RestoreFromGrok,
        enabled,
        account.status.as_ref(),
    );
    let can_rescan = action_available(
        account,
        ProviderAccountAction::Rescan,
        enabled,
        account.status.as_ref(),
    );
    let account_id = account.account_id.clone();

    let account_label = if enabled {
        account_label_text(&account.label, 14)
    } else {
        disabled_account_label_text(&account.label, 14)
    };
    let mut title_row = row![account_label]
        .spacing(8)
        .align_y(Alignment::Center)
        .height(Length::Fixed(22.0))
        .width(Length::Fill);
    if is_active {
        title_row = title_row.push(badge_with_tooltip(
            active_badge(enabled),
            fl!("badge-active-tooltip"),
        ));
    }
    let selector_content = match &account.status {
        Some(status) if status.reauth_eligible => {
            cosmic::iced::widget::column![title_row, status_badge(status, enabled),]
                .spacing(4)
                .width(Length::Fill)
        }
        Some(status) => {
            cosmic::iced::widget::column![title_row.push(status_badge(status, enabled))]
                .width(Length::Fill)
        }
        None => cosmic::iced::widget::column![title_row].width(Length::Fill),
    };
    let selector_content = container(selector_content)
        .padding([8, 12])
        .width(Length::Fill);

    let selector = widget::button::custom(selector_content)
        .class(account_row_button_class(is_selected))
        .width(Length::Fill)
        .on_press_maybe(enabled.then_some(Message::ToggleAccountSelection(
            provider,
            account_id.clone(),
        )));

    let can_delete = account.supports_action(ProviderAccountAction::Delete);
    let delete_press =
        (enabled && can_delete).then_some(Message::DeleteAccount(provider, account_id.clone()));
    let mut actions = row![account_selected_marker(is_selected, enabled)]
        .spacing(0)
        .align_y(Alignment::Center);
    if can_reauthenticate {
        actions = actions.push(account_action_icon_button(
            "view-refresh-symbolic",
            account.reauthenticate_tooltip.clone(),
            Some(Message::ReauthenticateAccount(provider, account_id.clone())),
        ));
    }
    if can_restore_from_opencode {
        actions = actions.push(account_action_icon_button(
            "document-import-symbolic",
            fl!("restore-from-opencode"),
            Some(Message::RestoreFromOpenCode(provider, account_id.clone())),
        ));
    }
    if can_restore_from_grok {
        actions = actions.push(account_action_icon_button(
            "document-import-symbolic",
            fl!("restore-from-grok"),
            Some(Message::RestoreFromGrok(account_id.clone())),
        ));
    }
    if can_rescan {
        actions = actions.push(account_action_icon_button(
            "view-refresh-symbolic",
            account.reauthenticate_tooltip.clone(),
            Some(Message::ReauthenticateAccount(provider, account_id.clone())),
        ));
    }
    actions = actions.push(account_action_icon_button(
        "edit-delete-symbolic",
        fl!("account-delete-tooltip"),
        delete_press,
    ));

    Element::from(account_row_container(
        selector.into(),
        actions.into(),
        is_selected,
        enabled,
        account
            .status
            .as_ref()
            .is_some_and(|status| status.style_as_action_required),
        position.first,
        position.last,
    ))
}

pub(super) fn account_selector_list<'a>(
    rows: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    container(rows)
        .width(Length::Fill)
        .style(component_container_style)
        .into()
}

pub(super) fn account_action_container<'a>(
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    container(content)
        .padding([16, 0, 0, 0])
        .width(Length::Fill)
        .into()
}

fn active_badge(enabled: bool) -> Element<'static, Message> {
    if enabled {
        badge_success(fl!("badge-active"))
    } else {
        badge_success_soft(fl!("badge-active"))
    }
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
            apply_alpha(
                cosmic.background(theme.transparent).component.on.into(),
                0.45,
            )
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

fn action_available(
    account: &ProviderAccountFacts,
    action: ProviderAccountAction,
    enabled: bool,
    status: Option<&ProviderAccountStatus>,
) -> bool {
    if !account.supports_action(action) {
        return false;
    }
    match action {
        ProviderAccountAction::Delete => true,
        ProviderAccountAction::RestoreFromOpenCode
        | ProviderAccountAction::RestoreFromGrok
        | ProviderAccountAction::Reauthenticate
        | ProviderAccountAction::Rescan => {
            enabled && status.is_some_and(|status| status.reauth_eligible)
        }
    }
}

fn account_row_container<'a>(
    selector: Element<'a, Message>,
    delete_button: Element<'a, Message>,
    selected: bool,
    enabled: bool,
    action_required: bool,
    first: bool,
    last: bool,
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
        let warning = cosmic.warning.base;
        let mut style = component_container_style(theme);
        style.text_color = Some(component_on_color(theme));
        style.background = if action_required {
            Some(Background::Color(apply_alpha(warning.into(), 0.08)))
        } else if selected && enabled {
            Some(Background::Color(accent_selection_fill(theme)))
        } else {
            component_container_background(theme)
        };
        style.border.radius = account_row_radius(cosmic.corner_radii.radius_s, first, last);
        style.border.width = if selected { 2.0 } else { 1.0 };
        style.border.color = if selected {
            if enabled {
                cosmic.accent.base.into()
            } else {
                apply_alpha(component_on_color(theme), 0.45)
            }
        } else if action_required {
            apply_alpha(warning.into(), 0.72)
        } else {
            component_divider_color(theme)
        };
        style.icon_color = Some(if enabled {
            component_on_color(theme)
        } else {
            apply_alpha(component_on_color(theme), 0.45)
        });
        style
    })
    .into()
}

fn account_row_radius(radius: [f32; 4], first: bool, last: bool) -> cosmic::iced::border::Radius {
    cosmic::iced::border::Radius {
        top_left: if first { radius[0] } else { 0.0 },
        top_right: if first { radius[1] } else { 0.0 },
        bottom_right: if last { radius[2] } else { 0.0 },
        bottom_left: if last { radius[3] } else { 0.0 },
    }
}

fn account_row_button_class(selected: bool) -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(move |focused, theme| {
            account_row_button_style(theme, selected, focused, 1.0)
        }),
        disabled: Box::new(move |theme| account_row_button_style(theme, selected, false, 0.45)),
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
    let foreground = cosmic.background(theme.transparent).component.on.into();

    style.icon_color = Some(apply_alpha(foreground, opacity));
    style.text_color = Some(apply_alpha(foreground, opacity));
    style.border_radius = cosmic.corner_radii.radius_s.into();
    style.border_width = if focused && selected { 1.0 } else { 0.0 };
    style.border_color = cosmic.accent.base.into();

    style
}

fn account_row_icon_button_class(available: bool) -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(move |_focused, theme| {
            account_row_icon_button_style(theme, if available { 1.0 } else { 0.45 })
        }),
        disabled: Box::new(move |theme| account_row_icon_button_style(theme, 0.45)),
        hovered: Box::new(move |_focused, theme| {
            account_row_icon_button_style(theme, if available { 1.0 } else { 0.45 })
        }),
        pressed: Box::new(move |_focused, theme| {
            account_row_icon_button_style(theme, if available { 0.85 } else { 0.45 })
        }),
    }
}

fn account_action_icon_button(
    icon_name: &'static str,
    tooltip: String,
    press: Option<Message>,
) -> Element<'static, Message> {
    let available = press.is_some();
    let handle = widget::icon::from_name(icon_name)
        .icon()
        .into_svg_handle()
        .unwrap_or_else(|| widget::svg::Handle::from_memory(Vec::new()));
    let icon = widget::Svg::new(handle)
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
        .opacity(if available { 1.0_f32 } else { 0.45_f32 })
        .width(Length::Fixed(16.0))
        .height(Length::Fixed(16.0));

    widget::tooltip::tooltip(
        widget::button::custom(icon)
            .class(account_row_icon_button_class(available))
            .padding(4)
            .on_press_maybe(press),
        widget::text(tooltip).size(12),
        widget::tooltip::Position::Top,
    )
    .into()
}

fn account_row_icon_button_style(theme: &cosmic::Theme, opacity: f32) -> widget::button::Style {
    let cosmic = theme.cosmic();
    let mut style = widget::button::Style::new();
    let foreground = cosmic.background(theme.transparent).component.on.into();

    style.icon_color = Some(apply_alpha(foreground, opacity));
    style.text_color = Some(apply_alpha(foreground, opacity));
    style.border_radius = cosmic.corner_radii.radius_m.into();

    style
}
