mod accounts;
mod general;

use super::{
    AppState, Config, Element, Message, ProviderId, ProviderLoginStates,
    SETTINGS_PROVIDER_ROW_HEIGHT, SETTINGS_SECTION_HEIGHT, UpdateStatus,
};

pub(super) fn general_settings_view<'a>(
    config: &'a Config,
    update_status: &'a UpdateStatus,
) -> Element<'a, Message> {
    general::general_settings_view(config, update_status)
}

pub(super) fn provider_settings_view<'a>(
    state: &'a AppState,
    config: &'a Config,
    logins: ProviderLoginStates<'a>,
    provider_id: ProviderId,
) -> Element<'a, Message> {
    accounts::provider_settings_view(state, config, logins, provider_id)
}

pub(super) fn settings_body_height(state: &AppState) -> f32 {
    let account_counts: Vec<usize> = [ProviderId::Codex, ProviderId::Claude, ProviderId::Cursor]
        .into_iter()
        .map(|provider| state.accounts_for(provider).len())
        .collect();
    let max_accounts = account_counts.iter().copied().max().unwrap_or(0).max(1);
    let account_rows = f32::from(u16::try_from(max_accounts).unwrap_or(u16::MAX));
    let show_all_row = if account_counts.iter().copied().any(|count| count > 1) {
        36.0
    } else {
        0.0
    };
    let general_height = {
        let refresh = SETTINGS_SECTION_HEIGHT;
        let panel_icon = 128.0;
        let reset_time = SETTINGS_SECTION_HEIGHT;
        let usage_amount = SETTINGS_SECTION_HEIGHT;
        let about = SETTINGS_SECTION_HEIGHT;
        refresh + panel_icon + reset_time + usage_amount + about + 70.0
    };
    let provider_settings_height = {
        let enable_section = 40.0 + SETTINGS_PROVIDER_ROW_HEIGHT;
        let accounts_section =
            40.0 + account_rows * SETTINGS_PROVIDER_ROW_HEIGHT + show_all_row + 40.0;
        enable_section + accounts_section + 28.0 + 8.0
    };
    let placeholder_height = SETTINGS_SECTION_HEIGHT * 2.0 + 28.0;
    general_height
        .max(provider_settings_height)
        .max(placeholder_height)
}
