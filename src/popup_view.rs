use crate::model::{AppState, ProviderId, ProviderRuntimeState, UsageWindow};
use cosmic::Element;
use cosmic::iced::widget::{column, container, progress_bar, row, scrollable, text};
use cosmic::iced::{Alignment, Length};
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
    let badge = container(text(provider_badge(provider.provider)).size(14))
        .width(Length::Fixed(28.0))
        .center_x(Length::Fixed(28.0))
        .center_y(Length::Fixed(28.0));
    let label = text(provider.provider.label()).size(13);
    let bar = progress_bar(0.0..=100.0, weekly)
        .length(Length::Fill)
        .girth(Length::Fixed(6.0));
    let pct = text(format!("{weekly:.0}%")).size(12);

    let content = container(
        column![badge, label, bar, pct]
            .spacing(6)
            .align_x(Alignment::Center)
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .padding(10);

    let class = if selected {
        cosmic::theme::Button::Suggested
    } else {
        cosmic::theme::Button::Standard
    };

    Element::from(
        widget::button::custom(content)
            .class(class)
            .width(Length::FillPortion(1))
            .on_press(Message::SelectProvider(provider.provider)),
    )
}

fn selected_provider_view(provider: Option<&ProviderRuntimeState>) -> Element<'_, Message> {
    let Some(provider) = provider else {
        return Element::from(container(text("No providers available")).width(Length::Fill));
    };

    let title_row = row![
        text(provider.provider.label()).size(28),
        cosmic::iced::widget::Space::new().width(Length::Fill),
        text(provider_status_badge(provider)).size(14)
    ]
    .align_y(Alignment::Center);

    let subtitle = text(
        provider
            .last_success_at
            .map(format_updated_label)
            .unwrap_or_else(|| provider.status_line()),
    )
    .size(14);

    let mut content = column![title_row, subtitle].spacing(10);

    if let Some(snapshot) = &provider.snapshot {
        if let Some(primary) = &snapshot.primary {
            content = content.push(usage_section(primary));
        }
        if let Some(secondary) = &snapshot.secondary {
            content = content.push(usage_section(secondary));
        }
        if let Some(tertiary) = &snapshot.tertiary {
            content = content.push(usage_section(tertiary));
        }
        if let Some(cost) = &snapshot.provider_cost {
            let cost_text = match cost.limit {
                Some(limit) => format!("{:.1} / {:.1} {}", cost.used, limit, cost.units),
                None => format!("{:.1} {}", cost.used, cost.units),
            };
            content = content.push(info_block("Cost", cost_text, None));
        }
        if let Some(email) = &snapshot.identity.email {
            content = content.push(info_block("Account", email.clone(), None));
        }
        if let Some(source) = &provider.source_label {
            content = content.push(info_block("Source", source.clone(), None));
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
    let reset = window
        .reset_description
        .clone()
        .unwrap_or_else(|| "Reset time unavailable".to_string());

    usage_block(
        &window.label,
        window.used_percent as f32,
        format!("{:.1}% used", window.used_percent),
        Some(reset),
    )
}

fn usage_block(
    title: &str,
    percent: f32,
    primary: String,
    secondary: Option<String>,
) -> Element<'_, Message> {
    let mut col = column![
        text(title).size(21),
        progress_bar(0.0..=100.0, percent)
            .length(Length::Fill)
            .girth(Length::Fixed(8.0)),
        text(primary).size(14)
    ]
    .spacing(8);

    if let Some(secondary) = secondary {
        col = col.push(text(secondary).size(13));
    }

    Element::from(container(col).width(Length::Fill).padding([4, 0]))
}

fn info_block(title: &str, primary: String, secondary: Option<String>) -> Element<'_, Message> {
    let mut col = column![text(title).size(21), text(primary).size(14)].spacing(8);

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

fn provider_badge(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Codex => "O",
        ProviderId::Claude => "A",
        ProviderId::Cursor => "Cu",
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
