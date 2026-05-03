use super::super::{
    Alignment, Background, ButtonInteraction, Element, Length, Message, PanelIconStyle, ProviderId,
    ResetTimeFormat, UpdateStatus, UsageAmountFormat, UsageWindow, accent_selection_fill,
    apply_alpha, container, fl, progress_bar, provider_icon_handle, provider_icon_variant, row,
    settings_block, update_available, update_notification_dot, usage_display, widget,
};

pub(super) fn general_settings_view<'a>(
    config: &'a crate::config::Config,
    update_status: &'a UpdateStatus,
) -> Element<'a, Message> {
    let refresh_section = refresh_section(config.refresh_interval_seconds);
    let panel_icon_section = panel_icon_section(config.panel_icon_style);
    let reset_time_section = reset_time_section(config.reset_time_format);
    let usage_amount_section = usage_amount_section(config.usage_amount_format);
    let about = about_section(update_status);

    Element::from(
        cosmic::iced::widget::column![
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
    let bars = cosmic::iced::widget::column![
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
                cosmic::iced::widget::column![
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
    let dist = if std::env::var_os("FLATPAK_ID").is_some() {
        "Flatpak"
    } else {
        "Native"
    };
    let mut title = row![widget::text(fl!("about-section-title")).size(16)]
        .align_y(Alignment::Center)
        .spacing(8);
    if update_available(update_status) {
        title = title.push(update_notification_dot(7.0));
    }

    let update_line: Element<'_, Message> = match update_status {
        UpdateStatus::UpdateAvailable { version, url } => cosmic::iced::widget::column![
            widget::text(fl!("update-available", version = version.as_str())).size(12),
            widget::button::link(fl!("update-open-release"))
                .on_press(Message::OpenUrl(url.clone())),
        ]
        .spacing(4)
        .width(Length::Fill)
        .into(),
        UpdateStatus::Unchecked => widget::text(fl!("update-checking")).size(12).into(),
        UpdateStatus::NoUpdate => widget::text(fl!("update-up-to-date")).size(12).into(),
        UpdateStatus::Error(reason) => cosmic::iced::widget::column![
            widget::text(fl!("update-failed", reason = reason.as_str())).size(12),
            widget::button::text(fl!("update-check-again"))
                .padding([0, 0])
                .on_press(Message::CheckUpdates),
        ]
        .spacing(4)
        .width(Length::Fill)
        .into(),
    };

    let inner = cosmic::iced::widget::column![
        widget::text(fl!("app-version", version = current_version, dist = dist)).size(12),
        update_line,
    ]
    .spacing(6)
    .width(Length::Fill);

    settings_block(title.into(), inner)
}
