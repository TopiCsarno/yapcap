use super::{
    Message, PROVIDER_CARD_SPACING, PROVIDER_HEIGHT_SECTION_SPACING, PROVIDER_SECTION_HEIGHT,
    PROVIDER_SUMMARY_HEIGHT, PopupRoute, SettingsRoute, account_label_text, badge_destructive,
    badge_neutral, badge_success, badge_warning, badge_with_tooltip, card,
    cursor_account_requires_action, info_block, plan_badge, provider_summary,
};
use crate::config::{Config, ResetTimeFormat, UsageAmountFormat};
use crate::fl;
use crate::model::{
    AccountSelectionStatus, AppState, AuthState, ProviderAccountRuntimeState, ProviderCost,
    ProviderHealth, ProviderId, ProviderRuntimeState, STALE_THRESHOLD, UsageSnapshot, UsageWindow,
};
use crate::usage_display;
use cosmic::Element;
use cosmic::iced::widget::{column, container, progress_bar, row};
use cosmic::iced::{Alignment, Background, Color, Length};
use cosmic::widget;

pub(super) fn selected_provider_view<'a>(
    provider: Option<&'a ProviderRuntimeState>,
    state: &'a AppState,
    config: &'a Config,
) -> Element<'a, Message> {
    let Some(provider) = provider else {
        return no_providers_view();
    };
    let accounts = state.selected_accounts(provider.provider);
    let summary = provider_summary(provider);

    if accounts.len() <= 1 {
        let account = accounts.first().copied();
        let items = account_column_items(account, provider, state, config);
        let mut content = column![summary]
            .spacing(PROVIDER_CARD_SPACING)
            .width(Length::Fill);
        for item in items {
            content = content.push(item);
        }
        Element::from(content)
    } else {
        let mut content = column![summary]
            .spacing(PROVIDER_CARD_SPACING)
            .width(Length::Fill);
        let mut cols_row = row![].spacing(8).height(Length::Fill);
        for account in &accounts {
            cols_row = cols_row.push(account_column_view(account, provider, state, config));
        }
        content = content.push(cols_row);
        Element::from(content)
    }
}

pub(super) fn provider_body_height_multi(
    state: &AppState,
    provider: Option<&ProviderRuntimeState>,
) -> f32 {
    let Some(provider) = provider else {
        return PROVIDER_SUMMARY_HEIGHT;
    };
    let accounts = state.selected_accounts(provider.provider);
    if accounts.is_empty() {
        return provider_body_height_for_account(provider, None);
    }
    accounts
        .iter()
        .map(|account| provider_body_height_for_account(provider, Some(account)))
        .fold(PROVIDER_SUMMARY_HEIGHT, f32::max)
}

pub(super) fn active_snapshot<'a>(
    state: &'a AppState,
    provider: &'a ProviderRuntimeState,
) -> Option<&'a UsageSnapshot> {
    state
        .active_account(provider.provider)
        .and_then(|account| account.snapshot.as_ref())
        .or(provider.legacy_display_snapshot.as_ref())
}

fn account_column_items<'a>(
    account: Option<&'a ProviderAccountRuntimeState>,
    provider: &'a ProviderRuntimeState,
    state: &'a AppState,
    config: &'a Config,
) -> Vec<Element<'a, Message>> {
    let mut items = Vec::new();
    if let Some(account) = account {
        items.push(account_column_header(account, provider));
    }
    items.extend(account_column_body_items(account, provider, state, config));
    items
}

fn account_column_body_items<'a>(
    account: Option<&'a ProviderAccountRuntimeState>,
    provider: &'a ProviderRuntimeState,
    state: &'a AppState,
    config: &'a Config,
) -> Vec<Element<'a, Message>> {
    let mut items = Vec::new();
    let snapshot = active_snapshot_for_account(account, provider);
    if let Some(snapshot) = snapshot {
        if account.is_some_and(|account| account.health == ProviderHealth::Error) {
            items.push(provider_status_info(provider, state, account));
        }
        let mut cost_shown = false;
        for window in &snapshot.windows {
            if window.label == "Extra" && snapshot.provider_cost.is_some() {
                items.push(extra_section(
                    window,
                    snapshot.provider_cost.as_ref(),
                    config.usage_amount_format,
                ));
                cost_shown = true;
            } else {
                items.push(usage_section(
                    window,
                    config.reset_time_format,
                    config.usage_amount_format,
                ));
            }
        }
        if !cost_shown && let Some(cost) = &snapshot.provider_cost {
            items.push(cost_section(provider.provider, cost));
        }
    } else {
        items.push(provider_status_info(provider, state, account));
    }
    items
}

fn account_column_header<'a>(
    account: &'a ProviderAccountRuntimeState,
    provider: &'a ProviderRuntimeState,
) -> Element<'a, Message> {
    let snapshot = account.snapshot.as_ref();
    let account_label = snapshot
        .and_then(|snapshot| snapshot.identity.email.as_deref())
        .filter(|email| !email.is_empty())
        .unwrap_or(account.label.as_str());
    let plan_label = snapshot.and_then(|snapshot| snapshot.identity.plan.as_deref());

    let mut label_row = row![account_label_text(account_label, 14)]
        .spacing(8)
        .align_y(Alignment::Center)
        .width(Length::Fill);
    label_row = label_row.push(cosmic::iced::widget::Space::new().width(Length::Fill));
    if let Some(plan) = plan_label.filter(|plan| !plan.trim().is_empty()) {
        label_row = label_row.push(plan_badge(plan));
    }

    let status = account_status_badge(account, provider);
    let mut status_row = row![status].spacing(8).align_y(Alignment::Center);
    if provider.active_account_id.as_deref() == Some(account.account_id.as_str()) {
        status_row = status_row.push(badge_with_tooltip(
            badge_success(fl!("badge-active")),
            fl!("badge-active-tooltip"),
        ));
    }
    if let Some(updated) = account.last_success_at.map(format_updated_label) {
        status_row = status_row.push(cosmic::iced::widget::Space::new().width(Length::Fill));
        status_row = status_row.push(widget::text(updated).size(12));
    }

    card(
        column![
            widget::text(fl!("account-label")).size(18),
            label_row,
            status_row,
        ]
        .spacing(6)
        .width(Length::Fill),
    )
}

fn account_column_view<'a>(
    account: &'a ProviderAccountRuntimeState,
    provider: &'a ProviderRuntimeState,
    state: &'a AppState,
    config: &'a Config,
) -> Element<'a, Message> {
    let header = account_column_header(account, provider);
    let body = account_column_body_items(Some(account), provider, state, config);
    let mut content = column![header]
        .spacing(PROVIDER_CARD_SPACING)
        .width(Length::Fill);
    for item in body {
        content = content.push(item);
    }
    container(content)
        .width(Length::FillPortion(1))
        .padding([0, 8])
        .style(|theme: &cosmic::Theme| {
            let cosmic = theme.cosmic();
            widget::container::Style {
                text_color: None,
                background: Some(Background::Color(cosmic.background.component.base.into())),
                border: cosmic::iced::Border {
                    radius: cosmic.corner_radii.radius_m.into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                shadow: cosmic::iced::Shadow::default(),
                icon_color: None,
                snap: false,
            }
        })
        .into()
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

fn provider_body_height_for_account(
    provider: &ProviderRuntimeState,
    account: Option<&ProviderAccountRuntimeState>,
) -> f32 {
    let mut sections = 1usize;
    let snapshot = active_snapshot_for_account(account, provider);
    if let Some(snapshot) = snapshot {
        if account.map(|account| &account.health) == Some(&ProviderHealth::Error) {
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
        if snapshot.identity.email.is_some() || account.is_some() {
            sections += 1;
        }
    } else {
        sections += 1;
    }

    let extra_sections = f32::from(u16::try_from(sections.saturating_sub(1)).unwrap_or(u16::MAX));
    PROVIDER_SUMMARY_HEIGHT
        + extra_sections * (PROVIDER_SECTION_HEIGHT + PROVIDER_HEIGHT_SECTION_SPACING)
}

fn active_snapshot_for_account<'a>(
    account: Option<&'a ProviderAccountRuntimeState>,
    provider: &'a ProviderRuntimeState,
) -> Option<&'a UsageSnapshot> {
    account
        .and_then(|account| account.snapshot.as_ref())
        .or(provider.legacy_display_snapshot.as_ref())
}

fn usage_section(
    window: &UsageWindow,
    reset_time_format: ResetTimeFormat,
    usage_amount_format: UsageAmountFormat,
) -> Element<'static, Message> {
    let now = chrono::Utc::now();
    let pace = usage_display::pace(window, now);
    usage_block(
        window.label.clone(),
        usage_display::displayed_amount_percent(window, now, usage_amount_format),
        usage_display::usage_amount_label(window, now, usage_amount_format),
        usage_display::reset_label(window, now, reset_time_format),
        pace,
        pace_marker_percent(pace, usage_amount_format),
    )
}

fn extra_section(
    window: &UsageWindow,
    cost: Option<&ProviderCost>,
    usage_amount_format: UsageAmountFormat,
) -> Element<'static, Message> {
    let now = chrono::Utc::now();
    let pace = usage_display::pace(window, now);
    usage_block(
        window.label.clone(),
        usage_display::displayed_amount_percent(window, now, usage_amount_format),
        usage_display::usage_amount_label(window, now, usage_amount_format),
        cost.map(format_cost),
        pace,
        pace_marker_percent(pace, usage_amount_format),
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

fn usage_block(
    title: String,
    percent: f32,
    primary: String,
    secondary: Option<String>,
    pace: Option<usage_display::UsagePace>,
    pace_marker_percent: Option<f32>,
) -> Element<'static, Message> {
    let pct_row = row![
        widget::text(primary).size(14),
        cosmic::iced::widget::Space::new().width(Length::Fill),
        widget::text(secondary.unwrap_or_default()).size(13),
    ]
    .align_y(Alignment::Center);

    card(
        column![
            widget::text(title).size(18),
            paced_progress_bar(
                percent,
                pace_marker_percent,
                pace.map(usage_display::pace_label)
            ),
            pct_row,
        ]
        .spacing(6),
    )
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

fn account_status_badge(
    account: &ProviderAccountRuntimeState,
    provider: &ProviderRuntimeState,
) -> Element<'static, Message> {
    if provider.is_refreshing {
        return badge_with_tooltip(
            badge_neutral(fl!("badge-refreshing")),
            fl!("badge-refreshing-tooltip"),
        );
    }
    if account.auth_state == AuthState::ActionRequired {
        return badge_with_tooltip(
            badge_warning(fl!("badge-login-required")),
            fl!("badge-login-required-tooltip"),
        );
    }
    if account.health == ProviderHealth::Error {
        return badge_with_tooltip(
            badge_destructive(fl!("badge-error")),
            fl!("badge-error-tooltip"),
        );
    }
    let now = chrono::Utc::now();
    if account.health == ProviderHealth::Ok
        && account.snapshot.is_some()
        && account
            .last_success_at
            .is_some_and(|updated| now - updated < STALE_THRESHOLD)
    {
        return badge_with_tooltip(badge_success(fl!("badge-live")), fl!("badge-live-tooltip"));
    }
    if account.snapshot.is_some() {
        return badge_with_tooltip(
            badge_warning(fl!("badge-stale")),
            fl!("badge-stale-tooltip"),
        );
    }
    badge_with_tooltip(
        badge_neutral(fl!("badge-loading")),
        fl!("badge-loading-tooltip"),
    )
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
