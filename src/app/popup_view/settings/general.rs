use super::super::{
    Alignment, Background, ButtonInteraction, Element, Length, Message, PanelIconStyle,
    PanelValueDisplay, ProviderId, ResetTimeFormat, UsageAmountFormat, UsageWindow, apply_alpha,
    component_container_style, component_divider_color, component_hover_color, component_on_color,
    component_selected_color, component_surface_color, container, fl, progress_bar,
    provider_icon_handle, provider_icon_variant, row, settings_block, usage_display, widget,
};

pub(super) fn general_settings_view<'a>(config: &'a crate::config::Config) -> Element<'a, Message> {
    let refresh_section = refresh_section(config.refresh_interval_seconds);
    let panel_icon_section = panel_icon_section(config.panel_icon_style);
    let show_all_providers_section = show_all_providers_section(config.show_all_providers);
    let show_all_accounts_section = show_all_accounts_section(config.show_all_accounts);
    let panel_font_size_section = panel_font_size_section(config.effective_panel_font_size());
    let panel_value_section = panel_value_section(config.panel_value_display);
    let reset_time_section = reset_time_section(config.reset_time_format);
    let usage_amount_section = usage_amount_section(config.usage_amount_format);
    Element::from(
        cosmic::iced::widget::column![
            refresh_section,
            panel_icon_section,
            show_all_providers_section,
            show_all_accounts_section,
            panel_font_size_section,
            panel_value_section,
            reset_time_section,
            usage_amount_section
        ]
        .spacing(14)
        .width(Length::Fill),
    )
}

fn refresh_section(current_seconds: u64) -> Element<'static, Message> {
    let options: &[(u64, &str)] = &[(60, "1"), (300, "5"), (900, "15"), (1800, "30")];

    let buttons = options.iter().enumerate().fold(
        row![].width(Length::Fill),
        |row, (index, (secs, text))| {
            let is_selected = *secs == current_seconds;
            let content = segmented_option_content(
                index,
                is_selected,
                segmented_option_text((*text).to_string(), 12, is_selected),
                [9, 8],
                34.0,
            );
            let tooltip = match *secs {
                60 => fl!("refresh-interval-1-tooltip"),
                300 => fl!("refresh-interval-5-tooltip"),
                900 => fl!("refresh-interval-15-tooltip"),
                1800 => fl!("refresh-interval-30-tooltip"),
                _ => fl!("refresh-interval-tooltip"),
            };
            row.push(widget::tooltip::tooltip(
                widget::button::custom(content)
                    .class(segmented_option_class(
                        is_selected,
                        index == 0,
                        index + 1 == options.len(),
                    ))
                    .padding(0)
                    .on_press(Message::SetRefreshInterval(*secs))
                    .width(Length::FillPortion(1)),
                widget::text(tooltip).size(12),
                widget::tooltip::Position::Top,
            ))
        },
    );

    settings_block(
        widget::text(fl!("refresh-section-title")).size(16).into(),
        segmented_options(buttons.into()),
    )
}

fn panel_icon_section(current_style: PanelIconStyle) -> Element<'static, Message> {
    let options = [
        PanelIconStyle::LogoAndBars,
        PanelIconStyle::BarsOnly,
        PanelIconStyle::LogoAndPercent,
        PanelIconStyle::PercentOnly,
    ];

    let buttons =
        options
            .iter()
            .enumerate()
            .fold(row![].width(Length::Fill), |row, (index, style)| {
                let is_selected = *style == current_style;
                let content = segmented_option_content(
                    index,
                    is_selected,
                    panel_icon_preview(*style),
                    [9, 6],
                    36.0,
                );
                let button = widget::button::custom(content)
                    .class(segmented_option_class(
                        is_selected,
                        index == 0,
                        index + 1 == options.len(),
                    ))
                    .padding(0)
                    .on_press(Message::SetPanelIconStyle(*style))
                    .width(Length::FillPortion(1));
                let tooltip = match *style {
                    PanelIconStyle::LogoAndBars => fl!("panel-icon-logo-and-bars-tooltip"),
                    PanelIconStyle::BarsOnly => fl!("panel-icon-bars-only-tooltip"),
                    PanelIconStyle::LogoAndPercent => fl!("panel-icon-logo-and-percent-tooltip"),
                    PanelIconStyle::PercentOnly => fl!("panel-icon-percent-only-tooltip"),
                };

                row.push(widget::tooltip::tooltip(
                    button,
                    widget::text(tooltip).size(12),
                    widget::tooltip::Position::Top,
                ))
            });

    settings_block(
        widget::text(fl!("panel-icon-section-title"))
            .size(16)
            .into(),
        segmented_options(buttons.into()),
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

fn show_all_providers_section(show_all_providers: bool) -> Element<'static, Message> {
    let checkbox = widget::checkbox(show_all_providers)
        .label(fl!("show-all-providers"))
        .on_toggle(Message::SetShowAllProviders)
        .text_size(12);

    widget::tooltip::tooltip(
        checkbox,
        widget::text(fl!("show-all-providers-tooltip")).size(12),
        widget::tooltip::Position::Top,
    )
    .into()
}

fn show_all_accounts_section(show_all_accounts: bool) -> Element<'static, Message> {
    let checkbox = widget::checkbox(show_all_accounts)
        .label(fl!("show-all-accounts"))
        .on_toggle(Message::SetShowAllAccounts)
        .text_size(12);

    widget::tooltip::tooltip(
        checkbox,
        widget::text(fl!("show-all-accounts-tooltip")).size(12),
        widget::tooltip::Position::Top,
    )
    .into()
}

fn panel_font_size_section(current: u16) -> Element<'static, Message> {
    let value_label = widget::text(format!("{current}")).size(14);
    let slider = cosmic::iced::widget::slider(
        crate::config::PANEL_FONT_SIZE_MIN..=crate::config::PANEL_FONT_SIZE_MAX,
        current,
        Message::SetPanelFontSize,
    )
    .step(1u16)
    .width(Length::Fill);

    let content = row![
        widget::tooltip::tooltip(
            value_label,
            widget::text(fl!("panel-font-size-tooltip")).size(12),
            widget::tooltip::Position::Top,
        ),
        container(slider).width(Length::Fill),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    settings_block(
        widget::text(fl!("panel-font-size-section-title"))
            .size(16)
            .into(),
        container(content).padding([4, 0]),
    )
}

fn panel_value_section(current_display: PanelValueDisplay) -> Element<'static, Message> {
    let options = [
        (PanelValueDisplay::Percent, fl!("panel-value-percent")),
        (PanelValueDisplay::Amount, fl!("panel-value-amount")),
        (PanelValueDisplay::Both, fl!("panel-value-both")),
    ];

    let buttons = options.iter().enumerate().fold(
        row![].width(Length::Fill),
        |row, (index, (display, text))| {
            let is_selected = *display == current_display;
            let content = segmented_option_content(
                index,
                is_selected,
                segmented_option_text(text.clone(), 12, is_selected),
                [9, 8],
                34.0,
            );
            let tooltip = match *display {
                PanelValueDisplay::Percent => fl!("panel-value-percent-tooltip"),
                PanelValueDisplay::Amount => fl!("panel-value-amount-tooltip"),
                PanelValueDisplay::Both => fl!("panel-value-both-tooltip"),
            };
            row.push(widget::tooltip::tooltip(
                widget::button::custom(content)
                    .class(segmented_option_class(
                        is_selected,
                        index == 0,
                        index + 1 == options.len(),
                    ))
                    .padding(0)
                    .on_press(Message::SetPanelValueDisplay(*display))
                    .width(Length::FillPortion(1)),
                widget::text(tooltip).size(12),
                widget::tooltip::Position::Top,
            ))
        },
    );

    settings_block(
        widget::text(fl!("panel-value-section-title"))
            .size(16)
            .into(),
        segmented_options(buttons.into()),
    )
}

fn reset_time_section(current_format: ResetTimeFormat) -> Element<'static, Message> {
    let options = [
        (ResetTimeFormat::Relative, fl!("reset-time-relative")),
        (ResetTimeFormat::Absolute, fl!("reset-time-absolute")),
    ];
    let buttons = options.iter().enumerate().fold(
        row![].width(Length::Fill),
        |row, (index, (format, text))| {
            let is_selected = *format == current_format;
            let now = chrono::Utc::now();
            let example_window = UsageWindow {
                label: "Session".to_string(),
                used_percent: 50.0,
                reset_at: Some(now + chrono::Duration::hours(4)),
                window_seconds: None,
                reset_description: None,
                group: None,
            };
            let example = usage_display::reset_label(&example_window, now, *format)
                .unwrap_or_else(|| fl!("reset-now"));
            let content = segmented_option_content(
                index,
                is_selected,
                cosmic::iced::widget::column![
                    segmented_option_text(text.clone(), 12, is_selected),
                    segmented_option_text(example, 9, is_selected),
                ]
                .spacing(2)
                .align_x(Alignment::Center)
                .into(),
                [9, 8],
                48.0,
            );
            let tooltip = match *format {
                ResetTimeFormat::Relative => fl!("reset-time-relative-tooltip"),
                ResetTimeFormat::Absolute => fl!("reset-time-absolute-tooltip"),
            };
            row.push(widget::tooltip::tooltip(
                widget::button::custom(content)
                    .class(segmented_option_class(
                        is_selected,
                        index == 0,
                        index + 1 == options.len(),
                    ))
                    .padding(0)
                    .on_press(Message::SetResetTimeFormat(*format))
                    .width(Length::FillPortion(1)),
                widget::text(tooltip).size(12),
                widget::tooltip::Position::Top,
            ))
        },
    );

    settings_block(
        widget::text(fl!("reset-time-section-title"))
            .size(16)
            .into(),
        segmented_options(buttons.into()),
    )
}

fn usage_amount_section(current_format: UsageAmountFormat) -> Element<'static, Message> {
    let options = [
        (UsageAmountFormat::Used, fl!("usage-amount-used")),
        (UsageAmountFormat::Left, fl!("usage-amount-left")),
    ];

    let buttons = options.iter().enumerate().fold(
        row![].width(Length::Fill),
        |row, (index, (format, text))| {
            let is_selected = *format == current_format;
            let content = segmented_option_content(
                index,
                is_selected,
                segmented_option_text(text.clone(), 12, is_selected),
                [9, 8],
                34.0,
            );
            let tooltip = match *format {
                UsageAmountFormat::Used => fl!("usage-amount-used-tooltip"),
                UsageAmountFormat::Left => fl!("usage-amount-left-tooltip"),
            };
            row.push(widget::tooltip::tooltip(
                widget::button::custom(content)
                    .class(segmented_option_class(
                        is_selected,
                        index == 0,
                        index + 1 == options.len(),
                    ))
                    .padding(0)
                    .on_press(Message::SetUsageAmountFormat(*format))
                    .width(Length::FillPortion(1)),
                widget::text(tooltip).size(12),
                widget::tooltip::Position::Top,
            ))
        },
    );

    settings_block(
        widget::text(fl!("usage-amount-section-title"))
            .size(16)
            .into(),
        segmented_options(buttons.into()),
    )
}

fn segmented_option_text(text: String, size: u16, selected: bool) -> Element<'static, Message> {
    let text = widget::text(text).size(size);
    if selected {
        text.class(cosmic::theme::Text::Accent).into()
    } else {
        text.into()
    }
}

fn segmented_option_content(
    index: usize,
    selected: bool,
    content: Element<'static, Message>,
    padding: [u16; 2],
    divider_height: f32,
) -> Element<'static, Message> {
    let content: Element<'static, Message> = if selected {
        let checkmark: Element<'static, Message> = container(
            widget::icon::icon(widget::icon::from_name("object-select-symbolic").into()).size(12),
        )
        .style(|theme| segmented_option_content_style(theme, true))
        .into();
        container(
            row![checkmark, content]
                .spacing(5)
                .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .align_x(Alignment::Center)
        .into()
    } else {
        content
    };
    let content = container(content)
        .width(Length::Fill)
        .padding(padding)
        .align_x(Alignment::Center);
    let content: Element<'static, Message> = if index == 0 {
        content.into()
    } else {
        row![segmented_divider(divider_height), content]
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .into()
    };

    container(content)
        .width(Length::Fill)
        .align_x(Alignment::Center)
        .style(move |theme| segmented_option_content_style(theme, selected))
        .into()
}

fn segmented_option_content_style(
    theme: &cosmic::Theme,
    selected: bool,
) -> widget::container::Style {
    let color = selected.then(|| theme.cosmic().accent_text_color().into());
    widget::container::Style {
        text_color: color,
        background: None,
        border: cosmic::iced::Border::default(),
        shadow: cosmic::iced::Shadow::default(),
        icon_color: color,
        snap: true,
    }
}

fn segmented_options(content: Element<'static, Message>) -> Element<'static, Message> {
    container(content)
        .width(Length::Fill)
        .style(segmented_options_style)
        .into()
}

fn segmented_divider(height: f32) -> Element<'static, Message> {
    container(cosmic::iced::widget::Space::new().width(Length::Fixed(1.0)))
        .height(Length::Fixed(height))
        .style(|theme: &cosmic::Theme| widget::container::Style {
            text_color: None,
            background: Some(Background::Color(apply_alpha(
                component_on_color(theme),
                0.28,
            ))),
            border: cosmic::iced::Border::default(),
            shadow: cosmic::iced::Shadow::default(),
            icon_color: None,
            snap: true,
        })
        .into()
}

fn segmented_options_style(theme: &cosmic::Theme) -> widget::container::Style {
    component_container_style(theme)
}

fn segmented_option_class(selected: bool, first: bool, last: bool) -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(move |focused, theme| {
            segmented_option_style(theme, selected, focused, 1.0, first, last)
        }),
        disabled: Box::new(move |theme| {
            segmented_option_style(theme, selected, false, 0.45, first, last)
        }),
        hovered: Box::new(move |focused, theme| {
            segmented_option_interaction_style(
                theme,
                selected,
                ButtonInteraction::hover(focused),
                1.0,
                first,
                last,
            )
        }),
        pressed: Box::new(move |focused, theme| {
            segmented_option_interaction_style(
                theme,
                selected,
                ButtonInteraction::press(focused),
                0.92,
                first,
                last,
            )
        }),
    }
}

fn segmented_option_style(
    theme: &cosmic::Theme,
    selected: bool,
    focused: bool,
    opacity: f32,
    first: bool,
    last: bool,
) -> widget::button::Style {
    segmented_option_interaction_style(
        theme,
        selected,
        ButtonInteraction::idle(focused),
        opacity,
        first,
        last,
    )
}

fn segmented_option_interaction_style(
    theme: &cosmic::Theme,
    selected: bool,
    interaction: ButtonInteraction,
    opacity: f32,
    first: bool,
    last: bool,
) -> widget::button::Style {
    let cosmic = theme.cosmic();
    let mut style = widget::button::Style::new();

    let (background, foreground) = if selected {
        (
            Some(component_selected_color(theme)),
            cosmic.accent_text_color().into(),
        )
    } else if interaction.pressed {
        (
            Some(component_divider_color(theme)),
            component_on_color(theme),
        )
    } else if interaction.hovered {
        (
            Some(component_hover_color(theme)),
            component_on_color(theme),
        )
    } else {
        (
            Some(component_surface_color(theme)),
            component_on_color(theme),
        )
    };

    style.background =
        background.map(|background| Background::Color(apply_alpha(background, opacity)));
    let radius = cosmic.corner_radii.radius_s;
    style.border_radius = cosmic::iced::border::Radius {
        top_left: if first { radius[0] } else { 0.0 },
        top_right: if last { radius[1] } else { 0.0 },
        bottom_right: if last { radius[2] } else { 0.0 },
        bottom_left: if first { radius[3] } else { 0.0 },
    };
    style.border_width = 0.0;
    style.border_color = apply_alpha(component_divider_color(theme), opacity);
    style.outline_width = if interaction.focused { 1.0 } else { 0.0 };
    style.outline_color = cosmic.accent.base.into();
    style.text_color = Some(apply_alpha(foreground, opacity));
    style.icon_color = Some(apply_alpha(foreground, opacity));

    style
}
