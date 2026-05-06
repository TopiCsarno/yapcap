use super::super::super::{
    Alignment, Background, Config, Element, Length, Message, ProviderAccountActionSupport,
    ProviderAccountRuntimeState, ProviderId, accent_selection_fill, account_label_text,
    apply_alpha, badge_destructive, badge_destructive_soft, badge_neutral, badge_neutral_soft,
    badge_success, badge_success_soft, badge_warning, badge_warning_soft, badge_with_tooltip,
    container, cursor_account_requires_action, fl, registry, row, widget,
};
use crate::model::{AuthState, ProviderHealth, STALE_THRESHOLD};

pub(super) fn cursor_account_settings_row<'a>(
    account: &'a ProviderAccountRuntimeState,
    selected_ids: &[&str],
    active_id: Option<&str>,
    config: &'a Config,
    enabled: bool,
) -> Element<'a, Message> {
    let is_selected = selected_ids.contains(&account.account_id.as_str());
    let is_active = active_id == Some(account.account_id.as_str());
    let requires_action = cursor_account_requires_action(account);
    let action_support =
        account_action_support(config, ProviderId::Cursor, account.account_id.as_str());
    let can_reauthenticate = enabled
        && requires_action
        && action_support.as_ref().is_some_and(|support| {
            support.can_reauthenticate && support.supports_background_status_refresh
        });
    let account_id = account.account_id.clone();

    let mut title_row = row![account_label_text(&account.label, 14)]
        .spacing(8)
        .align_y(Alignment::Center)
        .width(Length::Fill);
    if is_active {
        title_row = title_row.push(badge_with_tooltip(
            active_badge(enabled),
            fl!("badge-active-tooltip"),
        ));
    }
    let mut selector_body = cosmic::iced::widget::column![title_row]
        .spacing(6)
        .width(Length::Fill);
    if requires_action {
        selector_body = selector_body.push(
            row![badge_with_tooltip(
                if enabled {
                    badge_neutral(fl!("cursor-account-reauth-badge"))
                } else {
                    badge_neutral_soft(fl!("cursor-account-reauth-badge"))
                },
                fl!("badge-reauth-tooltip"),
            )]
            .width(Length::Fill)
            .align_y(Alignment::Center),
        );
    }

    let selector_content = container(selector_body)
        .padding([10, 12])
        .width(Length::Fill);

    let selector = widget::button::custom(selector_content)
        .class(account_row_button_class(is_selected))
        .width(Length::Fill)
        .on_press_maybe(enabled.then_some(Message::ToggleAccountSelection(
            ProviderId::Cursor,
            account_id.clone(),
        )));

    let delete_press = cursor_delete_message(config, account, enabled);
    let mut actions = row![account_selected_marker(is_selected, enabled)]
        .spacing(0)
        .align_y(Alignment::Center);
    if can_reauthenticate {
        actions = actions.push(account_action_icon_button(
            "view-refresh-symbolic",
            fl!("cursor-account-reauth-tooltip"),
            Some(Message::ReauthenticateCursorAccount(
                account.account_id.clone(),
            )),
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
        requires_action,
    ))
}

pub(super) fn codex_account_settings_row<'a>(
    account: &'a ProviderAccountRuntimeState,
    selected_ids: &[&str],
    active_id: Option<&str>,
    config: &Config,
    enabled: bool,
) -> Element<'a, Message> {
    let is_selected = selected_ids.contains(&account.account_id.as_str());
    let is_active = active_id == Some(account.account_id.as_str());
    let requires_action = codex_account_requires_action(account);
    let account_id = account.account_id.clone();
    let mut title_row = row![account_label_text(&account.label, 14)]
        .spacing(8)
        .align_y(Alignment::Center)
        .width(Length::Fill);
    if is_active {
        title_row = title_row.push(badge_with_tooltip(
            active_badge(enabled),
            fl!("badge-active-tooltip"),
        ));
    }
    let mut selector_body = cosmic::iced::widget::column![title_row]
        .spacing(6)
        .width(Length::Fill);
    if requires_action {
        selector_body = selector_body.push(
            row![badge_with_tooltip(
                if enabled {
                    badge_warning(fl!("badge-login-required"))
                } else {
                    badge_warning_soft(fl!("badge-login-required"))
                },
                fl!("badge-login-required-tooltip"),
            )]
            .width(Length::Fill)
            .align_y(Alignment::Center),
        );
    }
    let selector_content = container(selector_body)
        .padding([10, 12])
        .width(Length::Fill);

    let selector = widget::button::custom(selector_content)
        .class(account_row_button_class(is_selected))
        .width(Length::Fill)
        .on_press_maybe(enabled.then_some(Message::ToggleAccountSelection(
            ProviderId::Codex,
            account_id.clone(),
        )));

    let can_delete = account_action_support(config, ProviderId::Codex, account_id.as_str())
        .is_some_and(|support| support.can_delete);
    let delete_press =
        (enabled && can_delete).then_some(Message::DeleteCodexAccount(account_id.clone()));
    let mut actions = row![account_selected_marker(is_selected, enabled)]
        .spacing(0)
        .align_y(Alignment::Center);
    if enabled && requires_action {
        actions = actions.push(account_action_icon_button(
            "view-refresh-symbolic",
            fl!("codex-account-reauth-tooltip"),
            Some(Message::ReauthenticateCodexAccount(account_id)),
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
        requires_action,
    ))
}

pub(super) fn claude_account_settings_row<'a>(
    account: &'a ProviderAccountRuntimeState,
    selected_ids: &[&str],
    active_id: Option<&str>,
    config: &Config,
    enabled: bool,
) -> Element<'a, Message> {
    let is_selected = selected_ids.contains(&account.account_id.as_str());
    let is_active = active_id == Some(account.account_id.as_str());
    let account_id = account.account_id.clone();
    let row_label = claude_account_row_label(account, config);
    let row_status = claude_account_row_status(account);
    let action_support =
        account_action_support(config, ProviderId::Claude, account.account_id.as_str());
    let requires_action = row_status == Some(ClaudeAccountRowStatus::ReauthRequired);
    let can_reauthenticate = enabled
        && requires_action
        && action_support
            .as_ref()
            .is_some_and(|support| support.can_reauthenticate);
    let mut label_row = row![account_label_text(&row_label, 14)]
        .spacing(8)
        .align_y(Alignment::Center)
        .width(Length::Fill);
    if is_active {
        label_row = label_row.push(badge_with_tooltip(
            active_badge(enabled),
            fl!("badge-active-tooltip"),
        ));
    }
    let mut selector_body = cosmic::iced::widget::column![label_row]
        .spacing(6)
        .width(Length::Fill);
    if let Some(status) = row_status {
        selector_body = selector_body.push(
            row![claude_account_row_status_badge(status, enabled)]
                .width(Length::Fill)
                .align_y(Alignment::Center),
        );
    }
    let selector_content = container(selector_body)
        .padding([10, 12])
        .width(Length::Fill);

    let selector = widget::button::custom(selector_content)
        .class(account_row_button_class(is_selected))
        .width(Length::Fill)
        .on_press_maybe(enabled.then_some(Message::ToggleAccountSelection(
            ProviderId::Claude,
            account_id.clone(),
        )));

    let can_delete = action_support.is_some_and(|support| support.can_delete);
    let delete_press =
        (enabled && can_delete).then_some(Message::DeleteClaudeAccount(account_id.clone()));
    let mut actions = row![account_selected_marker(is_selected, enabled)]
        .spacing(0)
        .align_y(Alignment::Center);
    if can_reauthenticate {
        actions = actions.push(account_action_icon_button(
            "view-refresh-symbolic",
            fl!("claude-account-reauth-tooltip"),
            Some(Message::ReauthenticateClaudeAccount(account_id)),
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
        matches!(
            row_status,
            Some(ClaudeAccountRowStatus::ReauthRequired | ClaudeAccountRowStatus::Error)
        ),
    ))
}

pub(super) fn account_selector_list<'a>(
    rows: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    container(rows).width(Length::Fill).into()
}

pub(super) fn show_all_accounts_row(
    provider: ProviderId,
    show_all: bool,
    enabled: bool,
) -> Element<'static, Message> {
    let label = widget::tooltip::tooltip(
        widget::text(fl!("show-all-accounts-label")).size(13),
        widget::text(fl!("show-all-accounts-detail")).size(12),
        widget::tooltip::Position::Top,
    );

    let toggle = widget::toggler(show_all).on_toggle_maybe(
        enabled.then_some(move |enabled| Message::SetShowAllAccounts(provider, enabled)),
    );

    container(
        row![label, toggle]
            .spacing(12)
            .align_y(Alignment::Center)
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .padding([4, 0])
    .into()
}

fn cursor_delete_message(
    config: &Config,
    account: &ProviderAccountRuntimeState,
    enabled: bool,
) -> Option<Message> {
    let can_delete =
        account_action_support(config, ProviderId::Cursor, account.account_id.as_str())
            .is_some_and(|support| support.can_delete);
    if !enabled || !can_delete {
        return None;
    }
    Some(Message::DeleteCursorAccount(account.account_id.clone()))
}

fn claude_account_row_label(account: &ProviderAccountRuntimeState, config: &Config) -> String {
    let id = account.account_id.as_str();
    let managed = config
        .claude_managed_accounts
        .iter()
        .find(|managed| managed.id == id);
    let config_email = managed
        .and_then(|managed| managed.email.as_deref())
        .filter(|email| !email.is_empty());
    let snapshot_email = account
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.identity.email.as_deref())
        .filter(|email| !email.is_empty());
    snapshot_email
        .or(config_email)
        .unwrap_or(account.label.as_str())
        .to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaudeAccountRowStatus {
    ReauthRequired,
    Error,
    Stale,
}

fn claude_account_row_status(
    account: &ProviderAccountRuntimeState,
) -> Option<ClaudeAccountRowStatus> {
    if account.auth_state == AuthState::ActionRequired {
        return Some(ClaudeAccountRowStatus::ReauthRequired);
    }
    if account.health == ProviderHealth::Error {
        if account.snapshot.is_some() {
            return Some(ClaudeAccountRowStatus::Stale);
        }
        return Some(ClaudeAccountRowStatus::Error);
    }
    if account.snapshot.is_some()
        && account
            .last_success_at
            .is_none_or(|updated| chrono::Utc::now() - updated >= STALE_THRESHOLD)
    {
        return Some(ClaudeAccountRowStatus::Stale);
    }
    None
}

fn active_badge(enabled: bool) -> Element<'static, Message> {
    if enabled {
        badge_success(fl!("badge-active"))
    } else {
        badge_success_soft(fl!("badge-active"))
    }
}

fn claude_account_row_status_badge(
    status: ClaudeAccountRowStatus,
    enabled: bool,
) -> Element<'static, Message> {
    match status {
        ClaudeAccountRowStatus::ReauthRequired => badge_with_tooltip(
            if enabled {
                badge_warning(fl!("badge-login-required"))
            } else {
                badge_warning_soft(fl!("badge-login-required"))
            },
            fl!("badge-login-required-tooltip"),
        ),
        ClaudeAccountRowStatus::Error => badge_with_tooltip(
            if enabled {
                badge_destructive(fl!("badge-error"))
            } else {
                badge_destructive_soft(fl!("badge-error"))
            },
            fl!("badge-error-tooltip"),
        ),
        ClaudeAccountRowStatus::Stale => badge_with_tooltip(
            if enabled {
                badge_warning(fl!("badge-stale"))
            } else {
                badge_warning_soft(fl!("badge-stale"))
            },
            fl!("badge-stale-tooltip"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ManagedClaudeAccountConfig;
    use crate::model::{AuthState, ProviderHealth};
    use chrono::Utc;
    use std::path::PathBuf;

    fn claude_config(id: &str, email: Option<&str>) -> Config {
        Config {
            claude_managed_accounts: vec![ManagedClaudeAccountConfig {
                id: id.to_string(),
                label: "Claude account".to_string(),
                config_dir: PathBuf::from("/tmp/claude-test"),
                email: email.map(str::to_string),
                organization: None,
                subscription_type: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                last_authenticated_at: Some(Utc::now()),
            }],
            ..Config::default()
        }
    }

    #[test]
    fn claude_row_label_prefers_email_from_config() {
        let config = claude_config("claude-1", Some("user@example.com"));
        let account =
            ProviderAccountRuntimeState::empty(ProviderId::Claude, "claude-1", "Claude account");

        assert_eq!(
            claude_account_row_label(&account, &config),
            "user@example.com"
        );
    }

    #[test]
    fn claude_row_status_marks_action_required_before_error() {
        let mut account =
            ProviderAccountRuntimeState::empty(ProviderId::Claude, "claude-1", "Claude account");
        account.health = ProviderHealth::Error;
        account.auth_state = AuthState::ActionRequired;

        assert_eq!(
            claude_account_row_status(&account),
            Some(ClaudeAccountRowStatus::ReauthRequired)
        );
    }

    #[test]
    fn codex_row_requires_action_when_auth_state_demands_it() {
        let mut account =
            ProviderAccountRuntimeState::empty(ProviderId::Codex, "codex-1", "Codex account");
        account.auth_state = AuthState::ActionRequired;
        assert!(codex_account_requires_action(&account));
    }

    #[test]
    fn cursor_reauth_copy_does_not_use_inactive() {
        assert_eq!(fl!("cursor-account-reauth-badge"), "Re-auth needed");
        assert_eq!(
            fl!("cursor-account-reauth-tooltip"),
            "Rescan Cursor account"
        );
        assert!(!fl!("cursor-account-reauth-detail").contains("inactive"));
        assert!(!fl!("cursor-accounts-reauth-summary").contains("inactive"));
    }

    #[test]
    fn claude_row_status_marks_stale_snapshot() {
        let mut account =
            ProviderAccountRuntimeState::empty(ProviderId::Claude, "claude-1", "Claude account");
        account.health = ProviderHealth::Error;
        account.auth_state = AuthState::Ready;
        account.snapshot = Some(crate::model::UsageSnapshot {
            provider: ProviderId::Claude,
            source: "test".to_string(),
            updated_at: Utc::now(),
            headline: crate::model::UsageHeadline(0),
            windows: Vec::new(),
            provider_cost: None,
            extra_usage: None,
            identity: crate::model::ProviderIdentity::default(),
        });

        assert_eq!(
            claude_account_row_status(&account),
            Some(ClaudeAccountRowStatus::Stale)
        );
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
            apply_alpha(cosmic.background.component.on.into(), 0.45)
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

fn codex_account_requires_action(account: &ProviderAccountRuntimeState) -> bool {
    account.provider == ProviderId::Codex && account.auth_state == AuthState::ActionRequired
}

fn account_action_support(
    config: &Config,
    provider: ProviderId,
    account_id: &str,
) -> Option<ProviderAccountActionSupport> {
    registry::discover_accounts(provider, config)
        .into_iter()
        .find(|account| account.provider == provider && account.account_id == account_id)
        .map(|account| account.action_support())
}

fn account_row_container<'a>(
    selector: Element<'a, Message>,
    delete_button: Element<'a, Message>,
    selected: bool,
    enabled: bool,
    action_required: bool,
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
        let surface = &cosmic.background.component;
        let warning = cosmic.warning.base;
        widget::container::Style {
            text_color: Some(surface.on.into()),
            background: Some(Background::Color(if action_required {
                apply_alpha(warning.into(), 0.08)
            } else if selected && enabled {
                accent_selection_fill(theme)
            } else {
                surface.base.into()
            })),
            border: cosmic::iced::Border {
                radius: cosmic.corner_radii.radius_s.into(),
                width: if selected { 2.0 } else { 1.0 },
                color: if selected {
                    if enabled {
                        cosmic.accent.base.into()
                    } else {
                        apply_alpha(surface.on.into(), 0.45)
                    }
                } else if action_required {
                    apply_alpha(warning.into(), 0.72)
                } else {
                    surface.divider.into()
                },
            },
            shadow: cosmic::iced::Shadow::default(),
            icon_color: Some(if enabled {
                surface.on.into()
            } else {
                apply_alpha(surface.on.into(), 0.45)
            }),
            snap: true,
        }
    })
    .into()
}

fn account_row_button_class(selected: bool) -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(move |focused, theme| {
            account_row_button_style(theme, selected, focused, 1.0)
        }),
        disabled: Box::new(move |theme| {
            let opacity = if selected { 1.0 } else { 0.45 };
            account_row_button_style(theme, selected, false, opacity)
        }),
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
    let foreground = cosmic.background.component.on.into();

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
            color: Some(theme.cosmic().background.component.on.into()),
        }))
        .opacity(if available { 1.0 } else { 0.45 })
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
    let foreground = cosmic.background.component.on.into();

    style.icon_color = Some(apply_alpha(foreground, opacity));
    style.text_color = Some(apply_alpha(foreground, opacity));
    style.border_radius = cosmic.corner_radii.radius_m.into();

    style
}
