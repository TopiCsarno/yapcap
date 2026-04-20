// SPDX-License-Identifier: MPL-2.0

use crate::app::Message;
use crate::config::Config;
use crate::fl;
use crate::model::{
    AppState, ProviderCost, ProviderHealth, ProviderId, ProviderRuntimeState, STALE_THRESHOLD,
    UsageWindow,
};
use crate::provider_assets::{provider_icon_handle, provider_icon_variant};
use crate::updates::UpdateStatus;
use crate::usage_display;
use cosmic::Element;
use cosmic::iced::widget::{column, container, progress_bar, row, scrollable};
use cosmic::iced::{Alignment, Background, Color, Length};
use cosmic::widget;

pub fn popup_content<'a>(
    state: &'a AppState,
    config: &'a Config,
    selected_provider: ProviderId,
    show_settings: bool,
    update_status: &'a UpdateStatus,
) -> Element<'a, Message> {
    let selected = selected_state(state, selected_provider);

    let header = row![
        widget::text(fl!("app-title")).size(22),
        cosmic::iced::widget::Space::new().width(Length::Fill),
        widget::button::standard(fl!("refresh-now")).on_press(Message::RefreshNow)
    ]
    .align_y(Alignment::Center)
    .spacing(12);

    let tab_row = state
        .providers
        .iter()
        .filter(|provider| provider.enabled)
        .fold(row![].spacing(8), |row, provider| {
            row.push(provider_tab(
                provider,
                provider.provider == selected_provider,
            ))
        });

    let body: Element<'_, Message> = if show_settings {
        settings_view(state, config, update_status)
    } else {
        selected_provider_view(selected)
    };

    let settings_label = if show_settings {
        fl!("done")
    } else {
        fl!("settings")
    };
    let footer = row![
        widget::button::text(fl!("quit")).on_press(Message::Quit),
        cosmic::iced::widget::Space::new().width(Length::Fill),
        widget::button::text(settings_label).on_press(Message::ToggleSettings),
    ]
    .align_y(Alignment::Center);

    Element::from(
        column![
            header,
            tab_row,
            scrollable(body).height(Length::Fill),
            footer
        ]
        .spacing(14)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill),
    )
}

const SETTINGS_INDENT: u16 = 24;

fn settings_block<'a>(
    title: Element<'a, Message>,
    body: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    Element::from(
        column![title, container(body).padding([0, 0, 0, SETTINGS_INDENT]),]
            .spacing(10)
            .width(Length::Fill),
    )
}

fn settings_view<'a>(
    state: &'a AppState,
    config: &'a Config,
    update_status: &'a UpdateStatus,
) -> Element<'a, Message> {
    let provider_rows = state.providers.iter().fold(
        column![].spacing(10).width(Length::Fill),
        |col, provider| {
            let id = provider.provider;
            col.push(
                widget::toggler(provider.enabled)
                    .label(id.label().to_string())
                    .width(Length::Fill)
                    .on_toggle(move |enabled| Message::SetProviderEnabled(id, enabled)),
            )
        },
    );
    let providers_section = settings_block(
        widget::text(fl!("providers-section-title")).size(16).into(),
        provider_rows,
    );

    let refresh_section = refresh_section(config.refresh_interval_seconds);

    let about = about_section(update_status);

    Element::from(
        column![providers_section, refresh_section, about]
            .spacing(22)
            .padding([0, 0, 8, 0])
            .width(Length::Fill),
    )
}

fn refresh_section(current_seconds: u64) -> Element<'static, Message> {
    let label = widget::text(fl!("refresh-interval-label")).size(13);

    let options: &[(u64, &str)] = &[(60, "1m"), (300, "5m"), (900, "15m"), (1800, "30m")];

    let buttons = options.iter().fold(
        row![].spacing(8).width(Length::Fill),
        |row, (secs, text)| {
            let is_selected = *secs == current_seconds;
            let caption = if is_selected {
                format!("• {text}")
            } else {
                text.to_string()
            };
            row.push(
                widget::button::text(caption)
                    .on_press(Message::SetRefreshInterval(*secs))
                    .width(Length::Shrink),
            )
        },
    );

    settings_block(
        widget::text(fl!("refresh-section-title")).size(16).into(),
        column![label, buttons].spacing(10),
    )
}

fn about_section(update_status: &UpdateStatus) -> Element<'_, Message> {
    let current_version = env!("CARGO_PKG_VERSION");

    let update_line: Element<'_, Message> = match update_status {
        UpdateStatus::UpdateAvailable { version, url } => row![
            widget::text(fl!("update-available", version = version.as_str())).size(12),
            cosmic::iced::widget::Space::new().width(Length::Fixed(8.0)),
            widget::button::link(fl!("update-open-release"))
                .on_press(Message::OpenUrl(url.clone())),
        ]
        .align_y(Alignment::Center)
        .into(),
        UpdateStatus::Unchecked => widget::text(fl!("update-checking")).size(12).into(),
        UpdateStatus::NoUpdate => widget::text(fl!("update-up-to-date")).size(12).into(),
        UpdateStatus::Error(reason) => widget::text(fl!("update-failed", reason = reason.as_str()))
            .size(12)
            .into(),
    };

    let inner = column![
        widget::text(fl!("app-version", version = current_version)).size(12),
        update_line,
    ]
    .spacing(6)
    .width(Length::Fill);

    settings_block(
        widget::text(fl!("about-section-title")).size(16).into(),
        inner,
    )
}

fn provider_tab(provider: &ProviderRuntimeState, selected: bool) -> Element<'static, Message> {
    let weekly = tab_percent(provider);
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
    style.border_width = 2.0;
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

fn selected_provider_view(provider: Option<&ProviderRuntimeState>) -> Element<'_, Message> {
    let Some(provider) = provider else {
        return Element::from(container(widget::text(fl!("no-providers"))).width(Length::Fill));
    };

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
        widget::text(provider.provider.label()).size(28),
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    let title_row = row![
        title,
        cosmic::iced::widget::Space::new().width(Length::Fill),
        widget::text(plan_label).size(14)
    ]
    .align_y(Alignment::Center);

    let updated_label = provider
        .last_success_at
        .map_or_else(|| provider.status_line(), format_updated_label);
    let subtitle = row![
        widget::text(updated_label).size(14),
        cosmic::iced::widget::Space::new().width(Length::Fill),
        widget::text(provider_status_badge(provider)).size(14)
    ]
    .align_y(Alignment::Center);

    let mut content = column![title_row, subtitle].spacing(6);

    if let Some(snapshot) = &provider.snapshot {
        if provider.health == ProviderHealth::Error {
            content = content.push(info_block(
                fl!("status-label"),
                provider.status_line(),
                provider.error.clone(),
            ));
            if let Some(source) = provider.source_label.as_ref().or(Some(&snapshot.source)) {
                content = content.push(info_block(fl!("source-label"), source.clone(), None));
            }
        }
        let mut cost_shown = false;
        for window in &snapshot.windows {
            if window.label == "Extra" && snapshot.provider_cost.is_some() {
                content = content.push(extra_section(window, snapshot.provider_cost.as_ref()));
                cost_shown = true;
            } else {
                content = content.push(usage_section(window));
            }
        }
        if !cost_shown
            && let Some(cost) = &snapshot.provider_cost
        {
            content = content.push(cost_section(provider.provider, cost));
        }
        if let Some(email) = &snapshot.identity.email {
            content = content.push(info_block(fl!("account-label"), email.clone(), None));
        }
    } else {
        content = content.push(info_block(
            fl!("status-label"),
            provider.status_line(),
            provider.error.clone(),
        ));
    }

    Element::from(container(content.spacing(14).width(Length::Fill)).width(Length::Fill))
}

fn usage_section(window: &UsageWindow) -> Element<'static, Message> {
    let now = chrono::Utc::now();
    let pct = usage_display::displayed_percent(window, now);
    usage_block(
        window.label.clone(),
        pct,
        format!("{pct:.1}% used"),
        usage_display::reset_label(window, now),
    )
}

fn extra_section(window: &UsageWindow, cost: Option<&ProviderCost>) -> Element<'static, Message> {
    let cost_text = cost.map(format_cost);
    usage_block(
        window.label.clone(),
        window.used_percent,
        format!("{:.1}% used", window.used_percent),
        cost_text,
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

    Element::from(
        container(
            column![
                widget::text(fl!("credits-label")).size(18),
                widget::text(fl!("credits-available", balance = balance.as_str())).size(14),
            ]
            .spacing(6),
        )
        .width(Length::Fill)
        .padding([4, 0]),
    )
}

fn usage_block(
    title: String,
    percent: f32,
    primary: String,
    secondary: Option<String>,
) -> Element<'static, Message> {
    let pct_row = row![
        widget::text(primary).size(14),
        cosmic::iced::widget::Space::new().width(Length::Fill),
        widget::text(secondary.unwrap_or_default()).size(13),
    ]
    .align_y(Alignment::Center);

    let col = column![
        widget::text(title).size(18),
        progress_bar(0.0..=100.0, percent)
            .length(Length::Fill)
            .girth(Length::Fixed(8.0)),
        pct_row,
    ]
    .spacing(6);

    Element::from(container(col).width(Length::Fill).padding([4, 0]))
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

    Element::from(container(col).width(Length::Fill).padding([4, 0]))
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

fn tab_percent(provider: &ProviderRuntimeState) -> f32 {
    provider
        .snapshot
        .as_ref()
        .and_then(|s| s.headline_window())
        .map_or(0.0, |w| {
            usage_display::displayed_percent(w, chrono::Utc::now())
        })
}

fn provider_status_badge(provider: &ProviderRuntimeState) -> String {
    if !provider.enabled {
        return fl!("badge-disabled");
    }
    if provider.is_refreshing {
        return fl!("badge-refreshing");
    }
    let now = chrono::Utc::now();
    let recent = provider
        .last_success_at
        .is_some_and(|t| now - t < STALE_THRESHOLD);
    match (&provider.health, provider.snapshot.is_some(), recent) {
        (ProviderHealth::Ok, true, true) => fl!("badge-live"),
        (_, true, _) => fl!("badge-stale"),
        (ProviderHealth::Error, false, _) => fl!("badge-error"),
        (ProviderHealth::Ok, false, _) => fl!("badge-loading"),
    }
}

fn format_updated_label(last_success_at: chrono::DateTime<chrono::Utc>) -> String {
    let age = chrono::Utc::now() - last_success_at;
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
