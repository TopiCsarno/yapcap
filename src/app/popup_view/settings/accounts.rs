mod login_controls;
mod rows;

use self::login_controls::{
    claude_login_controls, codex_login_controls, cursor_scan_controls, gemini_login_controls,
};
use self::rows::{
    account_selector_list, claude_account_settings_row, codex_account_settings_row,
    cursor_account_settings_row, gemini_account_settings_row, show_all_accounts_row,
};
use super::super::{
    AppState, ClaudeLoginState, CodexLoginState, Config, CursorScanState, Element,
    GeminiLoginState, Length, Message, ProviderId, ProviderLoginStates, fl, settings_block,
    settings_block_enabled, widget,
};

pub(super) fn provider_settings_view<'a>(
    state: &'a AppState,
    config: &'a Config,
    logins: ProviderLoginStates<'a>,
    provider_id: ProviderId,
) -> Element<'a, Message> {
    let enabled = state
        .provider(provider_id)
        .is_some_and(|provider| provider.enabled);

    let enable_section = settings_block(
        widget::text(fl!("provider-enabled-title")).size(16).into(),
        widget::toggler(enabled)
            .width(Length::Fill)
            .on_toggle(move |enabled| Message::SetProviderEnabled(provider_id, enabled)),
    );

    let accounts_section = match provider_id {
        ProviderId::Codex => codex_accounts_section(state, config, logins.codex, enabled),
        ProviderId::Claude => claude_accounts_section(state, config, logins.claude, enabled),
        ProviderId::Cursor => cursor_accounts_section(state, config, logins.cursor_scan, enabled),
        ProviderId::Gemini => gemini_accounts_section(state, config, logins.gemini, enabled),
    };

    Element::from(
        cosmic::iced::widget::column![enable_section, accounts_section]
            .spacing(14)
            .width(Length::Fill),
    )
}

fn codex_accounts_section<'a>(
    state: &'a AppState,
    config: &'a Config,
    codex_login: Option<&'a CodexLoginState>,
    enabled: bool,
) -> Element<'a, Message> {
    let codex = state.provider(ProviderId::Codex);
    let selected_ids: Vec<&str> = codex
        .map(|provider| {
            provider
                .selected_account_ids
                .iter()
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();
    let accounts = state.accounts_for(ProviderId::Codex);
    let active_id = codex.and_then(|provider| provider.system_active_account_id.as_deref());
    let mut rows = cosmic::iced::widget::column![]
        .spacing(8)
        .width(Length::Fill);

    if accounts.is_empty() {
        rows = rows.push(widget::text(fl!("codex-accounts-empty")).size(13));
    } else {
        let mut account_rows = cosmic::iced::widget::column![]
            .spacing(6)
            .width(Length::Fill);
        for account in &accounts {
            account_rows = account_rows.push(codex_account_settings_row(
                account,
                &selected_ids,
                active_id,
                config,
                enabled,
            ));
        }
        rows = rows.push(account_selector_list(account_rows));
    }

    if let Some(provider) = codex
        && provider.account_status == crate::model::AccountSelectionStatus::SelectionRequired
    {
        rows = rows.push(widget::text(fl!("codex-account-select-required")).size(13));
    }

    if accounts.len() > 1 {
        rows = rows.push(show_all_accounts_row(
            ProviderId::Codex,
            config.show_all_accounts(ProviderId::Codex),
            enabled,
        ));
    }

    rows = rows.push(codex_login_controls(codex_login, enabled));

    settings_block_enabled(
        widget::text(fl!("codex-accounts-title")).size(16).into(),
        rows,
        enabled,
    )
}

fn claude_accounts_section<'a>(
    state: &'a AppState,
    config: &'a Config,
    claude_login: Option<&'a ClaudeLoginState>,
    enabled: bool,
) -> Element<'a, Message> {
    let selected_ids: Vec<&str> = state
        .provider(ProviderId::Claude)
        .map(|provider| {
            provider
                .selected_account_ids
                .iter()
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();
    let accounts = state.accounts_for(ProviderId::Claude);
    let active_id = state
        .provider(ProviderId::Claude)
        .and_then(|provider| provider.system_active_account_id.as_deref());
    let mut rows = cosmic::iced::widget::column![]
        .spacing(8)
        .width(Length::Fill);

    if accounts.is_empty() {
        rows = rows.push(widget::text(fl!("claude-accounts-empty")).size(13));
    } else {
        let mut account_rows = cosmic::iced::widget::column![]
            .spacing(6)
            .width(Length::Fill);
        for account in &accounts {
            account_rows = account_rows.push(claude_account_settings_row(
                account,
                &selected_ids,
                active_id,
                config,
                enabled,
            ));
        }
        rows = rows.push(account_selector_list(account_rows));
    }

    if accounts.len() > 1 {
        rows = rows.push(show_all_accounts_row(
            ProviderId::Claude,
            config.show_all_accounts(ProviderId::Claude),
            enabled,
        ));
    }

    rows = rows.push(claude_login_controls(claude_login, enabled));

    settings_block_enabled(
        widget::text(fl!("claude-accounts-title")).size(16).into(),
        rows,
        enabled,
    )
}

fn gemini_accounts_section<'a>(
    state: &'a AppState,
    config: &'a Config,
    gemini_login: Option<&'a GeminiLoginState>,
    enabled: bool,
) -> Element<'a, Message> {
    let selected_ids: Vec<&str> = state
        .provider(ProviderId::Gemini)
        .map(|provider| {
            provider
                .selected_account_ids
                .iter()
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();
    let accounts = state.accounts_for(ProviderId::Gemini);
    let active_id = state
        .provider(ProviderId::Gemini)
        .and_then(|provider| provider.system_active_account_id.as_deref());
    let mut rows = cosmic::iced::widget::column![]
        .spacing(8)
        .width(Length::Fill);

    if accounts.is_empty() {
        rows = rows.push(widget::text(fl!("gemini-accounts-empty")).size(13));
    } else {
        let mut account_rows = cosmic::iced::widget::column![]
            .spacing(6)
            .width(Length::Fill);
        for account in &accounts {
            account_rows = account_rows.push(gemini_account_settings_row(
                account,
                &selected_ids,
                active_id,
                config,
                enabled,
            ));
        }
        rows = rows.push(account_selector_list(account_rows));
    }

    if accounts.len() > 1 {
        rows = rows.push(show_all_accounts_row(
            ProviderId::Gemini,
            config.show_all_accounts(ProviderId::Gemini),
            enabled,
        ));
    }

    rows = rows.push(gemini_login_controls(gemini_login, enabled));

    settings_block_enabled(
        widget::text(fl!("gemini-accounts-title")).size(16).into(),
        rows,
        enabled,
    )
}

fn cursor_accounts_section<'a>(
    state: &'a AppState,
    config: &'a Config,
    cursor_scan: &'a CursorScanState,
    enabled: bool,
) -> Element<'a, Message> {
    let selected_ids: Vec<&str> = state
        .provider(ProviderId::Cursor)
        .map(|provider| {
            provider
                .selected_account_ids
                .iter()
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();
    let accounts = state.accounts_for(ProviderId::Cursor);
    let active_id = state
        .provider(ProviderId::Cursor)
        .and_then(|provider| provider.system_active_account_id.as_deref());
    let mut rows = cosmic::iced::widget::column![]
        .spacing(8)
        .width(Length::Fill);

    if accounts.is_empty() {
        rows = rows.push(widget::text(fl!("cursor-accounts-empty")).size(13));
    } else {
        let mut account_rows = cosmic::iced::widget::column![]
            .spacing(6)
            .width(Length::Fill);
        for account in &accounts {
            account_rows = account_rows.push(cursor_account_settings_row(
                account,
                &selected_ids,
                active_id,
                config,
                enabled,
            ));
        }
        rows = rows.push(account_selector_list(account_rows));
    }

    if accounts.len() > 1 {
        rows = rows.push(show_all_accounts_row(
            ProviderId::Cursor,
            config.show_all_accounts(ProviderId::Cursor),
            enabled,
        ));
    }

    rows = rows.push(cursor_scan_controls(cursor_scan, enabled));

    settings_block_enabled(
        widget::text(fl!("cursor-accounts-title")).size(16).into(),
        rows,
        enabled,
    )
}
