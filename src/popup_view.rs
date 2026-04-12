use crate::model::{AppState, ProviderCost, ProviderId, ProviderRuntimeState, UsageWindow};
use crate::provider_assets::{ProviderIconVariant, provider_icon_handle};
use cosmic::Element;
use cosmic::iced::widget::{column, container, progress_bar, row, scrollable, text};
use cosmic::iced::{Alignment, Background, Color, Length};
use cosmic::widget;

use crate::cosmic_app::Message;

pub fn popup_content(state: &AppState, selected_provider: ProviderId) -> Element<'_, Message> {
    let selected = selected_state(state, selected_provider);

    let header = row![
        text("YapCap").size(22),
        cosmic::iced::widget::Space::new().width(Length::Fill),
        widget::button::standard("Refresh now").on_press(Message::RefreshNow)
    ]
    .align_y(Alignment::Center)
    .spacing(12);

    let tab_row = state
        .providers
        .iter()
        .fold(row![].spacing(8), |row, provider| {
            row.push(provider_tab(
                provider,
                provider.provider == selected_provider,
            ))
        });

    let detail = selected_provider_view(selected);
    let footer = widget::button::text("Quit").on_press(Message::Quit);

    Element::from(
        column![
            header,
            tab_row,
            scrollable(detail).height(Length::Fill),
            footer
        ]
        .spacing(14)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill),
    )
}

fn provider_tab(provider: &ProviderRuntimeState, selected: bool) -> Element<'static, Message> {
    let weekly = tab_percent(provider);
    let icon_variant = provider_icon_variant();
    let badge = widget::icon::icon(provider_icon_handle(provider.provider, icon_variant))
        .size(18)
        .width(Length::Fixed(18.0))
        .height(Length::Fixed(18.0));
    let label = text(provider.provider.label()).size(12);
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
        active: Box::new(move |focused, theme| provider_tab_style(theme, selected, focused, 1.0)),
        disabled: Box::new(move |theme| provider_tab_style(theme, selected, false, 0.45)),
        hovered: Box::new(move |focused, theme| provider_tab_style(theme, selected, focused, 1.0)),
        pressed: Box::new(move |focused, theme| provider_tab_style(theme, selected, focused, 0.92)),
    }
}

fn provider_tab_style(
    theme: &cosmic::Theme,
    selected: bool,
    focused: bool,
    opacity: f32,
) -> widget::button::Style {
    let cosmic = theme.cosmic();
    let mut style = widget::button::Style::new();
    let surface = &cosmic.background.component;

    style.background = Some(Background::Color(apply_alpha(surface.base.into(), opacity)));
    style.border_radius = cosmic.corner_radii.radius_s.into();
    style.border_width = if selected { 2.0 } else { 1.0 };
    style.border_color = if selected {
        apply_alpha(cosmic.accent.base.into(), opacity)
    } else {
        apply_alpha(surface.divider.into(), opacity)
    };
    style.outline_width = if focused { 1.0 } else { 0.0 };
    style.outline_color = cosmic.accent.base.into();
    style.text_color = Some(apply_alpha(surface.on.into(), opacity));
    style.icon_color = Some(apply_alpha(surface.on.into(), opacity));

    style
}

fn apply_alpha(mut color: Color, opacity: f32) -> Color {
    color.a *= opacity;
    color
}

fn provider_icon_variant() -> ProviderIconVariant {
    if cosmic::theme::is_dark() {
        ProviderIconVariant::Reversed
    } else {
        ProviderIconVariant::Default
    }
}

fn selected_provider_view(provider: Option<&ProviderRuntimeState>) -> Element<'_, Message> {
    let Some(provider) = provider else {
        return Element::from(container(text("No providers available")).width(Length::Fill));
    };

    // Title: provider name (left) + plan (right)
    let plan_label = provider
        .snapshot
        .as_ref()
        .and_then(|s| s.identity.plan.as_deref())
        .unwrap_or("");
    let title = row![
        widget::icon::icon(provider_icon_handle(
            provider.provider,
            provider_icon_variant(),
        ))
        .size(24)
        .width(Length::Fixed(24.0))
        .height(Length::Fixed(24.0)),
        text(provider.provider.label()).size(28),
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    let title_row = row![
        title,
        cosmic::iced::widget::Space::new().width(Length::Fill),
        text(plan_label).size(14)
    ]
    .align_y(Alignment::Center);

    // Subtitle: "Updated Xm ago" (left) + status badge (right)
    let updated_label = provider
        .last_success_at
        .map(format_updated_label)
        .unwrap_or_else(|| provider.status_line());
    let subtitle = row![
        text(updated_label).size(14),
        cosmic::iced::widget::Space::new().width(Length::Fill),
        text(provider_status_badge(provider)).size(14)
    ]
    .align_y(Alignment::Center);

    let mut content = column![title_row, subtitle].spacing(6);

    if let Some(snapshot) = &provider.snapshot {
        if let Some(primary) = &snapshot.primary {
            content = content.push(usage_section(primary));
        }
        if let Some(secondary) = &snapshot.secondary {
            content = content.push(usage_section(secondary));
        }
        // Tertiary + cost: combined Extra usage section
        if let Some(tertiary) = &snapshot.tertiary {
            content = content.push(extra_section(tertiary, snapshot.provider_cost.as_ref()));
        } else if let Some(cost) = &snapshot.provider_cost {
            content = content.push(cost_section(cost));
        }
        if let Some(email) = &snapshot.identity.email {
            content = content.push(info_block("Account", email.clone(), None));
        }
    } else {
        content = content.push(info_block(
            "Status",
            provider.status_line(),
            provider.error.clone(),
        ));
    }

    Element::from(container(content.spacing(14).width(Length::Fill)).width(Length::Fill))
}

fn usage_section(window: &UsageWindow) -> Element<'_, Message> {
    let reset_text = window.reset_at.map(format_reset_label);
    usage_block(
        &window.label,
        window.used_percent as f32,
        format!("{:.1}% used", window.used_percent),
        reset_text,
    )
}

fn extra_section<'a>(
    window: &'a UsageWindow,
    cost: Option<&'a ProviderCost>,
) -> Element<'a, Message> {
    let cost_text = cost.map(|c| match c.limit {
        Some(limit) => format!("{}{:.2} / {}{:.2}", c.units, c.used, c.units, limit),
        None => format!("{}{:.2} spent", c.units, c.used),
    });
    usage_block(
        &window.label,
        window.used_percent as f32,
        format!("{:.1}% used", window.used_percent),
        cost_text,
    )
}

fn cost_section(cost: &ProviderCost) -> Element<'_, Message> {
    let text_str = match cost.limit {
        Some(limit) => format!(
            "{}{:.2} / {}{:.2}",
            cost.units, cost.used, cost.units, limit
        ),
        None => format!("{}{:.2} spent", cost.units, cost.used),
    };
    info_block("Extra", text_str, None)
}

fn usage_block(
    title: &str,
    percent: f32,
    primary: String,
    secondary: Option<String>,
) -> Element<'_, Message> {
    let pct_row = row![
        text(primary).size(14),
        cosmic::iced::widget::Space::new().width(Length::Fill),
        text(secondary.unwrap_or_default()).size(13),
    ]
    .align_y(Alignment::Center);

    let col = column![
        text(title).size(18),
        progress_bar(0.0..=100.0, percent)
            .length(Length::Fill)
            .girth(Length::Fixed(8.0)),
        pct_row,
    ]
    .spacing(6);

    Element::from(container(col).width(Length::Fill).padding([4, 0]))
}

fn info_block(title: &str, primary: String, secondary: Option<String>) -> Element<'_, Message> {
    let mut col = column![text(title).size(18), text(primary).size(14)].spacing(6);

    if let Some(secondary) = secondary {
        col = col.push(text(secondary).size(13));
    }

    Element::from(container(col).width(Length::Fill).padding([4, 0]))
}

fn selected_state(
    state: &AppState,
    selected_provider: ProviderId,
) -> Option<&ProviderRuntimeState> {
    state
        .providers
        .iter()
        .find(|provider| provider.provider == selected_provider)
        .or_else(|| state.providers.first())
}

fn tab_percent(provider: &ProviderRuntimeState) -> f32 {
    provider
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.headline_window())
        .map(|window| window.used_percent as f32)
        .unwrap_or(0.0)
}

fn provider_status_badge(provider: &ProviderRuntimeState) -> &'static str {
    if !provider.enabled {
        "Disabled"
    } else if provider.snapshot.is_some() {
        "Live"
    } else {
        "Error"
    }
}

fn format_updated_label(last_success_at: chrono::DateTime<chrono::Utc>) -> String {
    let age = chrono::Utc::now() - last_success_at;
    if age.num_seconds() < 10 {
        "Updated just now".to_string()
    } else if age.num_minutes() < 1 {
        format!("Updated {}s ago", age.num_seconds())
    } else if age.num_hours() < 1 {
        format!("Updated {}m ago", age.num_minutes())
    } else if age.num_days() < 1 {
        format!("Updated {}h ago", age.num_hours())
    } else {
        format!("Updated {}", last_success_at.format("%Y-%m-%d %H:%M"))
    }
}

fn format_reset_label(reset_at: chrono::DateTime<chrono::Utc>) -> String {
    let remaining = reset_at - chrono::Utc::now();
    if remaining.num_seconds() <= 0 {
        return "Resetting soon".to_string();
    }
    let days = remaining.num_days();
    let hours = remaining.num_hours() % 24;
    let mins = remaining.num_minutes() % 60;
    if days > 0 {
        format!("Resets in {}d {}h", days, hours)
    } else if hours > 0 {
        format!("Resets in {}h {}m", hours, mins)
    } else {
        format!("Resets in {}m", mins)
    }
}
