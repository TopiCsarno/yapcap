// SPDX-License-Identifier: MPL-2.0

use super::Message;
use crate::fl;
use cosmic::Element;
use cosmic::iced::widget::container;
use cosmic::iced::{Background, Color, Length};
use cosmic::widget;

const ACCOUNT_LABEL_MAX_CHARS: usize = 30;

pub(super) fn apply_alpha(mut color: Color, opacity: f32) -> Color {
    color.a *= opacity;
    color
}

pub(super) fn badge_success(label: impl Into<String>) -> Element<'static, Message> {
    let label = label.into();
    badge_container(label, move |theme| {
        let cosmic = theme.cosmic();
        let color = cosmic.success.base.into();
        badge_style(apply_alpha(color, 0.14), color, color, theme)
    })
}

pub(super) fn badge_success_soft(label: impl Into<String>) -> Element<'static, Message> {
    let label = label.into();
    badge_container(label, move |theme| {
        let cosmic = theme.cosmic();
        let color = cosmic.success.base.into();
        badge_style(
            apply_alpha(color, 0.06),
            apply_alpha(color, 0.52),
            apply_alpha(color, 0.28),
            theme,
        )
    })
}

pub(super) fn badge_warning(label: impl Into<String>) -> Element<'static, Message> {
    let label = label.into();
    badge_container(label, move |theme| {
        let cosmic = theme.cosmic();
        let color = cosmic.warning.base.into();
        badge_style(apply_alpha(color, 0.14), color, color, theme)
    })
}

pub(super) fn badge_warning_soft(label: impl Into<String>) -> Element<'static, Message> {
    let label = label.into();
    badge_container(label, move |theme| {
        let cosmic = theme.cosmic();
        let color = cosmic.warning.base.into();
        badge_style(
            apply_alpha(color, 0.06),
            apply_alpha(color, 0.52),
            apply_alpha(color, 0.28),
            theme,
        )
    })
}

pub(super) fn badge_destructive(label: impl Into<String>) -> Element<'static, Message> {
    let label = label.into();
    badge_container(label, move |theme| {
        let cosmic = theme.cosmic();
        let color = cosmic.destructive.base.into();
        badge_style(apply_alpha(color, 0.14), color, color, theme)
    })
}

pub(super) fn badge_destructive_soft(label: impl Into<String>) -> Element<'static, Message> {
    let label = label.into();
    badge_container(label, move |theme| {
        let cosmic = theme.cosmic();
        let color = cosmic.destructive.base.into();
        badge_style(
            apply_alpha(color, 0.06),
            apply_alpha(color, 0.52),
            apply_alpha(color, 0.28),
            theme,
        )
    })
}

pub(super) fn badge_neutral(label: impl Into<String>) -> Element<'static, Message> {
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

pub(super) fn badge_neutral_soft(label: impl Into<String>) -> Element<'static, Message> {
    let label = label.into();
    badge_container(label, move |theme| {
        let cosmic = theme.cosmic();
        let surface = &cosmic.background.component;
        badge_style(
            apply_alpha(surface.base.into(), 0.24),
            apply_alpha(surface.on.into(), 0.52),
            apply_alpha(surface.divider.into(), 0.45),
            theme,
        )
    })
}

pub(super) fn plan_badge(label: &str) -> Element<'static, Message> {
    badge_with_tooltip(
        badge_neutral(format_plan_label(label)),
        fl!("badge-plan-tooltip"),
    )
}

pub(super) fn badge_with_tooltip(
    badge: Element<'static, Message>,
    tooltip: impl Into<String>,
) -> Element<'static, Message> {
    widget::tooltip::tooltip(
        badge,
        widget::text(tooltip.into()).size(12),
        widget::tooltip::Position::Top,
    )
    .into()
}

pub(super) fn account_label_text(label: &str, size: u16) -> Element<'static, Message> {
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

fn format_plan_label(label: &str) -> String {
    let mut chars = label.trim().chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    first.to_uppercase().chain(chars).collect()
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
