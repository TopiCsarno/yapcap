use super::super::provider_assets::app_icon_handle;
use super::{
    Message, PROVIDER_CARD_SPACING, PROVIDER_GROUP_PADDING, PROVIDER_GROUP_SPACING, PopupRoute,
    account_action_button, account_label_text, apply_alpha, badge_destructive, badge_neutral,
    badge_success, badge_warning, badge_with_tooltip, card, clamp_account_page,
    component_card_background, component_container_style, component_divider_color,
    component_hover_color, component_on_color, detected_without_accounts, info_block,
    pager_account_label, plan_badge, provider_icon_handle, provider_icon_variant, provider_summary,
};
use crate::app::PagerDirection;
use crate::config::{Config, UsageAmountFormat};
use crate::currency_format;
use crate::fl;
use crate::model::{
    AccountSelectionStatus, AppState, AuthState, ExtraUsageState, ProviderAccountRuntimeState,
    ProviderCost, ProviderHealth, ProviderId, ProviderRuntimeState, STALE_THRESHOLD, UsageSnapshot,
    UsageWindow,
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
    detection: &'a crate::detection::DetectionSnapshot,
    account_page: usize,
) -> Element<'a, Message> {
    let Some(provider) = provider else {
        return empty_state_view();
    };
    let accounts = state.accounts_for(provider.provider);
    let detected_without_accounts = detected_without_accounts(state, detection, provider.provider);
    let summary = provider_summary(provider, detected_without_accounts);

    if accounts.is_empty() {
        column![
            summary,
            section_title(fl!("usage-label")),
            usage_card(account_body_items(None, provider, state, config, detection)),
        ]
        .spacing(PROVIDER_CARD_SPACING)
        .width(Length::Fill)
        .into()
    } else {
        let total = accounts.len();
        let active = clamp_account_page(account_page, total);
        let account = accounts[active];
        let pager = (total > 1).then_some((active, total));
        column![
            summary,
            account_view(account, provider, state, config, detection, pager,),
        ]
        .spacing(PROVIDER_CARD_SPACING)
        .width(Length::Fill)
        .into()
    }
}

fn account_pager(
    account: &ProviderAccountRuntimeState,
    active: usize,
    total: usize,
) -> Element<'static, Message> {
    let name = pager_account_label(active, &account.label);
    let previous = widget::tooltip::tooltip(
        widget::button::icon(widget::icon::from_name("go-previous-symbolic"))
            .extra_small()
            .padding(6)
            .class(account_pager_button_class())
            .on_press(Message::PageProviderAccount(PagerDirection::Previous)),
        widget::text(fl!("account-pager-previous")).size(12),
        widget::tooltip::Position::Top,
    );
    let next = widget::tooltip::tooltip(
        widget::button::icon(widget::icon::from_name("go-next-symbolic"))
            .extra_small()
            .padding(6)
            .class(account_pager_button_class())
            .on_press(Message::PageProviderAccount(PagerDirection::Next)),
        widget::text(fl!("account-pager-next")).size(12),
        widget::tooltip::Position::Top,
    );
    let dots = (0..total).fold(
        row![].spacing(10).align_y(Alignment::Center),
        |dots, page| dots.push(account_page_dot(page == active)),
    );
    let center = widget::tooltip::tooltip(
        container(dots),
        widget::text(name).size(12),
        widget::tooltip::Position::Top,
    );

    container(
        row![previous, center, next]
            .spacing(24)
            .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .align_x(Alignment::Center)
    .into()
}

fn account_pager_button_class() -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(|_focused, theme| account_pager_button_style(theme, false)),
        disabled: Box::new(|theme| account_pager_button_style(theme, false)),
        hovered: Box::new(|_focused, theme| account_pager_button_style(theme, true)),
        pressed: Box::new(|_focused, theme| account_pager_button_style(theme, true)),
    }
}

fn account_pager_button_style(theme: &cosmic::Theme, hovered: bool) -> widget::button::Style {
    let cosmic = theme.cosmic();
    let mut style = widget::button::Style::new();
    style.background = Some(Background::Color(if hovered {
        component_hover_color(theme)
    } else {
        component_divider_color(theme)
    }));
    style.border_radius = cosmic.corner_radii.radius_s.into();
    style.icon_color = Some(component_on_color(theme));
    style.text_color = Some(component_on_color(theme));
    style
}

fn account_page_dot(active: bool) -> Element<'static, Message> {
    container(
        cosmic::iced::widget::Space::new()
            .width(Length::Fixed(7.0))
            .height(Length::Fixed(7.0)),
    )
    .style(move |theme: &cosmic::Theme| {
        let cosmic = theme.cosmic();
        let color = if active {
            cosmic.accent.base.into()
        } else {
            component_divider_color(theme)
        };
        let shadow = cosmic::iced::Shadow {
            color: apply_alpha(color, if active { 0.72 } else { 0.38 }),
            offset: cosmic::iced::Vector::new(0.0, 0.0),
            blur_radius: if active { 6.0 } else { 3.0 },
        };
        widget::container::Style {
            text_color: None,
            background: Some(Background::Color(color)),
            border: cosmic::iced::Border {
                radius: 4.0.into(),
                width: 0.0,
                color: Color::TRANSPARENT,
            },
            shadow,
            icon_color: None,
            snap: true,
        }
    })
    .into()
}

fn account_body_items<'a>(
    account: Option<&'a ProviderAccountRuntimeState>,
    provider: &'a ProviderRuntimeState,
    state: &'a AppState,
    config: &'a Config,
    detection: &'a crate::detection::DetectionSnapshot,
) -> Vec<Element<'a, Message>> {
    let mut items = Vec::new();
    let snapshot = active_snapshot_for_account(account, provider);
    if let Some(snapshot) = snapshot {
        if account.is_some_and(|account| account.health == ProviderHealth::Error) {
            items.extend(provider_status_info(provider, state, account, detection));
        }
        items.extend(window_sections(snapshot, config));
        match snapshot.provider {
            ProviderId::Claude => {
                if let Some(extra) = snapshot.extra_usage.as_ref() {
                    items.push(extra_usage_detail_section(
                        extra,
                        config.usage_amount_format,
                    ));
                } else if let Some(cost) = snapshot.provider_cost.as_ref() {
                    items.push(extra_usage_cost_bar(cost, None, config.usage_amount_format));
                }
            }
            _ => {
                if let Some(cost) = snapshot.provider_cost.as_ref() {
                    items.push(cost_section(snapshot.provider, cost));
                }
            }
        }
    } else {
        items.extend(provider_status_info(provider, state, account, detection));
    }
    items
}

fn account_header_content<'a>(
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
    let active_id = provider.system_active_account_id.as_deref();
    if active_id == Some(account.account_id.as_str()) {
        status_row = status_row.push(badge_with_tooltip(
            badge_success(fl!("badge-active")),
            fl!("badge-active-tooltip"),
        ));
    }
    if let Some(updated) = account.last_success_at.map(format_updated_label) {
        status_row = status_row.push(cosmic::iced::widget::Space::new().width(Length::Fill));
        status_row = status_row.push(widget::text(updated).size(12));
    }

    column![
        label_row,
        status_row,
        account_card_divider(),
        account_action_button(
            fl!("manage-accounts"),
            Some(Message::NavigateTo(PopupRoute::ManageAccounts(
                provider.provider,
            ))),
        ),
    ]
    .spacing(8)
    .width(Length::Fill)
    .into()
}

fn account_view<'a>(
    account: &'a ProviderAccountRuntimeState,
    provider: &'a ProviderRuntimeState,
    state: &'a AppState,
    config: &'a Config,
    detection: &'a crate::detection::DetectionSnapshot,
    pager: Option<(usize, usize)>,
) -> Element<'a, Message> {
    let header = account_header_content(account, provider);
    let body = account_body_items(Some(account), provider, state, config, detection);
    let mut account_content = column![header].spacing(8).width(Length::Fill);
    if let Some((active, total)) = pager {
        account_content = account_content
            .push(account_card_divider())
            .push(account_pager(account, active, total));
    }
    let account_card: Element<'a, Message> = container(account_content)
        .width(Length::Fill)
        .padding(8)
        .style(component_card_style)
        .into();

    column![
        section_title(fl!("account-label")),
        account_card,
        section_title(fl!("usage-label")),
        usage_card(body),
    ]
    .spacing(PROVIDER_CARD_SPACING)
    .width(Length::Fill)
    .into()
}

fn section_title(label: String) -> Element<'static, Message> {
    widget::text(label).size(15).into()
}

fn account_card_divider() -> Element<'static, Message> {
    container(cosmic::iced::widget::Space::new().height(Length::Fixed(1.0)))
        .width(Length::Fill)
        .style(|theme: &cosmic::Theme| widget::container::Style {
            text_color: None,
            background: Some(Background::Color(component_divider_color(theme))),
            border: cosmic::iced::Border::default(),
            shadow: cosmic::iced::Shadow::default(),
            icon_color: None,
            snap: true,
        })
        .into()
}

fn usage_card<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let mut content = column![].spacing(PROVIDER_CARD_SPACING).width(Length::Fill);
    for item in items {
        content = content.push(item);
    }
    container(content)
        .width(Length::Fill)
        .padding(8)
        .style(component_card_style)
        .into()
}

fn component_card_style(theme: &cosmic::Theme) -> widget::container::Style {
    let cosmic = theme.cosmic();
    let mut style = component_container_style(theme);
    style.background = Some(component_card_background(theme));
    style.border.radius = cosmic.corner_radii.radius_m.into();
    style.border.width = 0.0;
    style.border.color = Color::TRANSPARENT;
    style.text_color = None;
    style.icon_color = None;
    style.snap = false;
    style
}

pub(super) fn empty_state_view<'a>() -> Element<'a, Message> {
    let logos = row![
        widget::icon::icon(provider_icon_handle(
            ProviderId::Claude,
            provider_icon_variant()
        ))
        .size(22),
        widget::icon::icon(app_icon_handle()).size(48),
        widget::icon::icon(provider_icon_handle(
            ProviderId::Codex,
            provider_icon_variant()
        ))
        .size(22),
    ]
    .spacing(12)
    .align_y(Alignment::Center);
    let content = column![
        logos,
        widget::text(fl!("no-providers")).size(22),
        widget::text(fl!("no-providers-detail")).size(13),
        widget::button::suggested(fl!("no-providers-open-settings"))
            .on_press(Message::NavigateTo(PopupRoute::ManageProviders)),
    ]
    .spacing(12)
    .align_x(Alignment::Center)
    .width(Length::Fill);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .padding(32)
        .into()
}

fn provider_status_info(
    provider: &ProviderRuntimeState,
    state: &AppState,
    active_account: Option<&ProviderAccountRuntimeState>,
    detection: &crate::detection::DetectionSnapshot,
) -> Option<Element<'static, Message>> {
    if detected_without_accounts(state, detection, provider.provider) {
        return Some(detected_provider_cta(provider.provider));
    }
    let message = provider_status_message(provider, state, active_account);
    if message.is_empty() {
        return None;
    }
    Some(info_block(
        fl!("status-label"),
        message,
        None,
        login_required_settings_action(provider, state, active_account),
    ))
}

fn detected_provider_cta(provider: ProviderId) -> Element<'static, Message> {
    info_block(
        fl!("provider-detected-chip"),
        fl!("provider-detected-cta", provider = provider.label()),
        None,
        Some(
            widget::button::suggested(fl!("provider-detected-add-account"))
                .on_press(Message::NavigateTo(PopupRoute::ManageAccounts(provider)))
                .into(),
        ),
    )
}

fn login_required_settings_action(
    provider: &ProviderRuntimeState,
    state: &AppState,
    active_account: Option<&ProviderAccountRuntimeState>,
) -> Option<Element<'static, Message>> {
    if !should_show_login_required_settings_action(provider, state, active_account) {
        return None;
    }

    Some(
        widget::button::standard(fl!(
            "open-provider-settings",
            provider = provider.provider.label()
        ))
        .on_press(Message::NavigateTo(PopupRoute::ManageAccounts(
            provider.provider,
        )))
        .into(),
    )
}

fn should_show_login_required_settings_action(
    provider: &ProviderRuntimeState,
    state: &AppState,
    active_account: Option<&ProviderAccountRuntimeState>,
) -> bool {
    active_account.is_none()
        && state.accounts_for(provider.provider).is_empty()
        && provider.account_status == AccountSelectionStatus::LoginRequired
}

fn provider_status_message(
    provider: &ProviderRuntimeState,
    _state: &AppState,
    active_account: Option<&ProviderAccountRuntimeState>,
) -> String {
    let mut messages = Vec::new();

    let cursor_reauth_needed = active_account.is_some_and(|a| {
        a.provider == ProviderId::Cursor && a.auth_state == AuthState::ActionRequired
    });

    if cursor_reauth_needed {
        messages.push(fl!("cursor-account-reauth-detail"));
    } else if let Some(account) = active_account
        && account.health == ProviderHealth::Error
    {
        if account.auth_state == AuthState::ActionRequired {
            messages.push(fl!("account-reauth-summary"));
        } else if let Some(error) = &account.error {
            messages.push(error.clone());
        }
    } else {
        messages.push(provider.status_line(active_account));
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

fn active_snapshot_for_account<'a>(
    account: Option<&'a ProviderAccountRuntimeState>,
    provider: &'a ProviderRuntimeState,
) -> Option<&'a UsageSnapshot> {
    account
        .and_then(|account| account.snapshot.as_ref())
        .or(provider.legacy_display_snapshot.as_ref())
}

fn window_sections<'a>(
    snapshot: &'a UsageSnapshot,
    config: &'a Config,
) -> Vec<Element<'static, Message>> {
    let mut items = Vec::new();
    let mut windows = snapshot.windows.iter().peekable();
    while let Some(window) = windows.next() {
        let Some(group) = window.group.as_deref() else {
            items.push(card(usage_section_content(
                window,
                snapshot.provider,
                config,
            )));
            continue;
        };
        let mut sections = vec![usage_section_content(window, snapshot.provider, config)];
        while let Some(next) = windows.next_if(|next| next.group.as_deref() == Some(group)) {
            sections.push(usage_section_content(next, snapshot.provider, config));
        }
        items.push(usage_group_card(group, sections));
    }
    items
}

fn usage_group_card(
    group: &str,
    sections: Vec<Element<'static, Message>>,
) -> Element<'static, Message> {
    let mut content = column![widget::text(group.to_string()).size(18)]
        .spacing(PROVIDER_GROUP_SPACING)
        .width(Length::Fill);
    for section in sections {
        content = content.push(section);
    }
    container(content)
        .width(Length::Fill)
        .padding(PROVIDER_GROUP_PADDING / 2.0)
        .style(group_card_style)
        .into()
}

fn group_card_style(theme: &cosmic::Theme) -> widget::container::Style {
    let cosmic = theme.cosmic();
    let mut style = component_card_style(theme);
    style.border.width = 1.0;
    style.border.color = cosmic
        .background(theme.transparent)
        .component
        .divider
        .into();
    style
}

fn usage_section_content(
    window: &UsageWindow,
    provider: ProviderId,
    config: &Config,
) -> Element<'static, Message> {
    let now = chrono::Utc::now();
    usage_block_content(
        window_display_label(provider, &window.label),
        usage_display::usage_amount_label(window, now, config.usage_amount_format),
        UsageBlockDetails {
            secondary: usage_display::reset_label(window, now, config.reset_time_format),
            secondary_tooltip: None,
            meter: usage_display::usage_meter(window, now, config.usage_amount_format),
            overage: overage_text(window),
        },
    )
}

fn window_display_label(provider: ProviderId, label: &str) -> String {
    if provider == ProviderId::Copilot {
        match label {
            "chat" => return fl!("copilot-window-chat"),
            "completions" => return fl!("copilot-window-completions"),
            "premium_interactions" => return fl!("copilot-window-premium"),
            "credits" => return fl!("copilot-window-credits"),
            _ => {}
        }
    }
    label.to_string()
}

fn overage_text(window: &UsageWindow) -> Option<String> {
    if window.label == "premium_interactions" || window.label == "credits" {
        return window.reset_description.clone();
    }
    None
}

fn extra_usage_detail_section(
    state: &ExtraUsageState,
    usage_amount_format: UsageAmountFormat,
) -> Element<'static, Message> {
    match state {
        ExtraUsageState::Disabled => info_block(
            fl!("extra-usage-label"),
            fl!("extra-usage-disabled"),
            None,
            None,
        ),
        ExtraUsageState::Active { used_percent, cost } => {
            extra_usage_cost_bar(cost, Some(*used_percent), usage_amount_format)
        }
    }
}

fn extra_usage_pct_from_cost(cost: &ProviderCost) -> f32 {
    cost.limit
        .filter(|l| *l > f64::EPSILON)
        .map_or(0.0_f32, |l| usage_display::portion_percent(cost.used, l))
}

fn extra_usage_cost_bar(
    cost: &ProviderCost,
    used_percent: Option<f32>,
    usage_amount_format: UsageAmountFormat,
) -> Element<'static, Message> {
    let now = chrono::Utc::now();
    let used_percent = used_percent
        .unwrap_or_else(|| extra_usage_pct_from_cost(cost))
        .clamp(0.0, 100.0);
    let window = UsageWindow {
        label: String::new(),
        used_percent,
        reset_at: None,
        window_seconds: None,
        reset_description: None,
        group: None,
    };
    let (cost_line, cost_tip) = currency_format::format_provider_cost(cost);
    usage_block(
        fl!("extra-usage-label"),
        usage_display::usage_amount_label(&window, now, usage_amount_format),
        UsageBlockDetails {
            secondary: Some(cost_line),
            secondary_tooltip: Some(cost_tip),
            meter: usage_display::usage_meter(&window, now, usage_amount_format),
            overage: None,
        },
    )
}

fn cost_section(provider: ProviderId, cost: &ProviderCost) -> Element<'static, Message> {
    if provider == ProviderId::Codex || provider == ProviderId::Grok {
        return credit_section(cost);
    }
    let (primary, iso_tip) = currency_format::format_provider_cost(cost);
    let body = widget::tooltip::tooltip(
        widget::text(primary).size(14),
        widget::text(iso_tip).size(12),
        widget::tooltip::Position::Top,
    );
    card(column![widget::text(fl!("extra-usage-label")).size(15), body,].spacing(6))
}

fn credit_section(cost: &ProviderCost) -> Element<'static, Message> {
    let balance = if cost.used.fract() == 0.0 {
        format!("{:.0}", cost.used)
    } else {
        format!("{:.2}", cost.used)
    };

    card(
        column![
            widget::text(fl!("credits-label")).size(15),
            widget::text(fl!("credits-available", balance = balance.as_str())).size(14),
        ]
        .spacing(6),
    )
}

fn usage_block(
    title: String,
    primary: String,
    details: UsageBlockDetails,
) -> Element<'static, Message> {
    card(usage_block_content(title, primary, details))
}

fn usage_block_content(
    title: String,
    primary: String,
    details: UsageBlockDetails,
) -> Element<'static, Message> {
    let pct_row = row![
        widget::text(primary).size(14),
        cosmic::iced::widget::Space::new().width(Length::Fill),
        secondary_cost_text(
            details.secondary.unwrap_or_default(),
            details.secondary_tooltip
        ),
    ]
    .align_y(Alignment::Center);

    let mut content = column![
        widget::text(title).size(15),
        usage_progress_bar(details.meter),
    ]
    .spacing(6);

    if let Some(overage) = details.overage {
        content = content.push(overage_line(overage));
    }

    content.push(pct_row).into()
}

struct UsageBlockDetails {
    secondary: Option<String>,
    secondary_tooltip: Option<String>,
    meter: usage_display::UsageMeter,
    overage: Option<String>,
}

fn overage_line(text: String) -> Element<'static, Message> {
    container(widget::text(text).size(13))
        .style(|theme: &cosmic::Theme| {
            let color = apply_alpha(theme.cosmic().warning.base.into(), 0.92);
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

fn secondary_cost_text(text: String, tooltip: Option<String>) -> Element<'static, Message> {
    if text.is_empty() {
        cosmic::iced::widget::Space::new()
            .width(Length::Shrink)
            .into()
    } else if let Some(tip) = tooltip {
        widget::tooltip::tooltip(
            widget::text(text).size(13),
            widget::text(tip).size(12),
            widget::tooltip::Position::Top,
        )
        .into()
    } else {
        widget::text(text).size(13).into()
    }
}

fn usage_progress_bar(meter: usage_display::UsageMeter) -> Element<'static, Message> {
    let progress: Element<'static, Message> = progress_bar(0.0..=100.0, meter.fill_percent)
        .length(Length::Fill)
        .girth(Length::Fixed(8.0))
        .into();

    let bar = if let Some(marker_percent) = meter.marker_percent {
        cosmic::iced::widget::Stack::new()
            .push(progress)
            .push(pace_marker(marker_percent))
            .width(Length::Fill)
            .height(Length::Fixed(8.0))
            .into()
    } else {
        progress
    };

    widget::tooltip::tooltip(
        bar,
        widget::text(meter.tooltip).size(12),
        widget::tooltip::Position::Top,
    )
    .into()
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
                        radius: cosmic.corner_radii.radius_xl.into(),
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
        if account.provider == ProviderId::Cursor {
            return badge_with_tooltip(
                badge_neutral(fl!("badge-cursor-reauth-needed")),
                fl!("badge-cursor-reauth-needed-tooltip"),
            );
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AccountSelectionStatus, AuthState, ProviderHealth};

    #[test]
    fn action_required_account_reports_reauth_message() {
        let provider = ProviderRuntimeState {
            provider: ProviderId::Claude,
            enabled: true,
            selected_account_ids: vec!["claude-1".to_string()],
            active_account_id: Some("claude-1".to_string()),
            system_active_account_id: None,
            account_status: AccountSelectionStatus::Ready,
            is_refreshing: false,
            refresh_started_at: None,
            legacy_display_snapshot: None,
            error: None,
        };
        let mut account =
            ProviderAccountRuntimeState::empty(ProviderId::Claude, "claude-1", "Claude account");
        account.health = ProviderHealth::Error;
        account.auth_state = AuthState::ActionRequired;
        account.error = Some("claude token refresh returned http 400".to_string());
        let state = AppState::empty();

        let message = provider_status_message(&provider, &state, Some(&account));
        assert!(
            !message.contains("http 400"),
            "status message must not contain raw error: {message}"
        );
        assert!(
            message.contains("Re-authenticate") || message.contains("Settings"),
            "status message should be action-oriented: {message}"
        );
    }

    #[test]
    fn cursor_action_required_account_reports_reauth_needed_message() {
        let provider = ProviderRuntimeState {
            provider: ProviderId::Cursor,
            enabled: true,
            selected_account_ids: vec!["cursor-1".to_string()],
            active_account_id: Some("cursor-1".to_string()),
            system_active_account_id: Some("cursor-1".to_string()),
            account_status: AccountSelectionStatus::Ready,
            is_refreshing: false,
            refresh_started_at: None,
            legacy_display_snapshot: None,
            error: None,
        };
        let mut account =
            ProviderAccountRuntimeState::empty(ProviderId::Cursor, "cursor-1", "user@example.com");
        account.health = ProviderHealth::Error;
        account.auth_state = AuthState::ActionRequired;
        account.error = Some("Unauthorized".to_string());
        let state = AppState::empty();

        let message = provider_status_message(&provider, &state, Some(&account));

        assert!(
            message.contains("Re-authenticate") || message.contains("rescan"),
            "status message should explain the Cursor recovery path: {message}"
        );
        assert!(
            !message.contains("Inactive") && !message.contains("inactive"),
            "status message must not describe auth failures as inactive: {message}"
        );
    }

    #[test]
    fn cursor_action_required_badge_copy_uses_reauth_needed() {
        assert_eq!(fl!("badge-cursor-reauth-needed"), "Re-auth needed");
        assert!(!fl!("badge-cursor-reauth-needed-tooltip").contains("inactive"));
    }

    #[test]
    fn codex_without_accounts_reports_login_required() {
        let provider = ProviderRuntimeState {
            provider: ProviderId::Codex,
            enabled: true,
            selected_account_ids: Vec::new(),
            active_account_id: None,
            system_active_account_id: None,
            account_status: AccountSelectionStatus::LoginRequired,
            is_refreshing: false,
            refresh_started_at: None,
            legacy_display_snapshot: None,
            error: Some("Login required".to_string()),
        };
        let state = AppState::empty();

        assert_eq!(
            provider_status_message(&provider, &state, None),
            "Login required"
        );
    }

    #[test]
    fn login_required_empty_state_links_to_each_provider_settings_page() {
        for provider_id in ProviderId::ALL {
            let provider = ProviderRuntimeState {
                provider: provider_id,
                enabled: true,
                selected_account_ids: Vec::new(),
                active_account_id: None,
                system_active_account_id: None,
                account_status: AccountSelectionStatus::LoginRequired,
                is_refreshing: false,
                refresh_started_at: None,
                legacy_display_snapshot: None,
                error: Some("Login required".to_string()),
            };
            let state = AppState::empty();

            assert!(should_show_login_required_settings_action(
                &provider, &state, None
            ));
        }
    }

    #[test]
    fn login_required_settings_action_hides_when_account_exists() {
        let provider = ProviderRuntimeState {
            provider: ProviderId::Codex,
            enabled: true,
            selected_account_ids: Vec::new(),
            active_account_id: None,
            system_active_account_id: None,
            account_status: AccountSelectionStatus::LoginRequired,
            is_refreshing: false,
            refresh_started_at: None,
            legacy_display_snapshot: None,
            error: Some("Login required".to_string()),
        };
        let mut state = AppState::empty();
        state
            .provider_accounts
            .push(ProviderAccountRuntimeState::empty(
                ProviderId::Codex,
                "codex-test",
                "test@example.com",
            ));

        assert!(!should_show_login_required_settings_action(
            &provider, &state, None
        ));
    }

    #[test]
    fn detected_provider_without_accounts_shows_detected_cta() {
        let home = tempfile::tempdir().expect("create temporary home");
        std::fs::create_dir(home.path().join(".codex")).expect("create Codex marker");
        let detection = crate::detection::detect(home.path());
        let state = AppState::empty();

        assert!(detected_without_accounts(
            &state,
            &detection,
            ProviderId::Codex
        ));
        assert!(!detected_without_accounts(
            &state,
            &detection,
            ProviderId::Claude
        ));

        let mut with_account = state.clone();
        with_account
            .provider_accounts
            .push(ProviderAccountRuntimeState::empty(
                ProviderId::Codex,
                "codex-test",
                "test@example.com",
            ));
        assert!(!detected_without_accounts(
            &with_account,
            &detection,
            ProviderId::Codex
        ));
    }
}
