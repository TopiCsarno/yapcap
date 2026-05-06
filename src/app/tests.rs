use super::applet::{
    applet_bar_width, applet_button_size, applet_percent_cell_alignment, applet_percent_cell_width,
    applet_percent_text, select_provider, selected_provider_all_percents,
};
use super::popup_view::{POPUP_COLUMN_WIDTH, popup_session_size};
use super::{
    APPLET_ACCOUNT_GAP, APPLET_ICON_GAP, APPLET_PERCENT_ACCOUNT_GAP, AppState, PanelIconStyle,
    ProviderId, Size, UsageAmountFormat, format_retry_delay, popup_size_limits_with_max_width,
    popup_size_tuple, update_retry_delay,
};
use crate::model::{
    ProviderAccountRuntimeState, ProviderIdentity, UsageHeadline, UsageSnapshot, UsageWindow,
};
use chrono::Utc;
use std::time::Duration;

#[test]
fn popup_limits_allow_wider_max() {
    let limits = popup_size_limits_with_max_width(Size::new(420.0, 640.0), 840.0);

    assert_eq!(limits.min().width, 1.0);
    assert_eq!(limits.max().width, 840.0);
    assert_eq!(limits.min().height, 640.0);
    assert_eq!(limits.max().height, 640.0);
}

#[test]
fn popup_size_tuple_rounds_logical_size() {
    assert_eq!(popup_size_tuple(Size::new(419.6, 640.2)), (420, 640));
}

#[test]
fn update_retry_delay_backs_off_to_cap() {
    assert_eq!(update_retry_delay(1), Duration::from_secs(15));
    assert_eq!(update_retry_delay(2), Duration::from_secs(30));
    assert_eq!(update_retry_delay(7), Duration::from_secs(15 * 60));
    assert_eq!(update_retry_delay(20), Duration::from_secs(15 * 60));
}

#[test]
fn retry_delay_format_is_compact() {
    assert_eq!(format_retry_delay(Duration::from_secs(15)), "15s");
    assert_eq!(format_retry_delay(Duration::from_secs(60)), "1m");
    assert_eq!(format_retry_delay(Duration::from_secs(75)), "1m 15s");
}

#[test]
fn select_provider_keeps_current_when_enabled() {
    let mut state = AppState::empty();
    for p in &mut state.providers {
        p.enabled = true;
    }
    assert_eq!(
        select_provider(ProviderId::Claude, &state),
        ProviderId::Claude
    );
}

#[test]
fn select_provider_falls_back_when_current_disabled() {
    let mut state = AppState::empty();
    for p in &mut state.providers {
        p.enabled = p.provider != ProviderId::Codex;
    }
    let selected = select_provider(ProviderId::Codex, &state);
    assert_ne!(selected, ProviderId::Codex);
}

#[test]
fn applet_button_size_uses_panel_icon_style() {
    let core = cosmic::Core::default();
    let (suggested_w, suggested_h) = core.applet.suggested_size(false);
    let (major_padding, minor_padding) = core.applet.suggested_padding(false);
    let horizontal_padding = if core.applet.is_horizontal() {
        major_padding
    } else {
        minor_padding
    };
    let compact_px = suggested_w.min(suggested_h);
    let logo_width = f32::from(compact_px.saturating_sub(8).max(11));
    let bar_width = applet_bar_width(suggested_w, suggested_h);
    let padding_width = f32::from(2 * horizontal_padding);
    let (logo_bars_width, height) = applet_button_size(&core, PanelIconStyle::LogoAndBars, 1);
    let (bars_only_width, bars_only_height) =
        applet_button_size(&core, PanelIconStyle::BarsOnly, 1);
    let (percent_width, percent_height) =
        applet_button_size(&core, PanelIconStyle::LogoAndPercent, 1);
    let (percent_only_width, percent_only_height) =
        applet_button_size(&core, PanelIconStyle::PercentOnly, 1);

    assert_eq!(bars_only_width, bar_width + padding_width);
    let cell_100 = applet_percent_cell_width();
    assert_eq!(percent_only_width, cell_100 + padding_width);
    assert_eq!(
        logo_bars_width,
        logo_width + APPLET_ICON_GAP + bar_width + padding_width
    );
    assert_eq!(
        percent_width,
        logo_width + APPLET_ICON_GAP + cell_100 + padding_width
    );
    assert_eq!(height, bars_only_height);
    assert_eq!(height, percent_height);
    assert_eq!(height, percent_only_height);
}

#[test]
fn applet_button_size_ignores_percent_primaries_for_bar_styles() {
    let core = cosmic::Core::default();
    let (a, _) = applet_button_size(&core, PanelIconStyle::BarsOnly, 2);
    let (b, _) = applet_button_size(&core, PanelIconStyle::BarsOnly, 2);
    assert_eq!(a, b);
    let (c, _) = applet_button_size(&core, PanelIconStyle::LogoAndBars, 2);
    let (d, _) = applet_button_size(&core, PanelIconStyle::LogoAndBars, 2);
    assert_eq!(c, d);
}

#[test]
fn applet_button_size_scales_with_account_count() {
    let core = cosmic::Core::default();
    let (w1, _) = applet_button_size(&core, PanelIconStyle::BarsOnly, 1);
    let (w2, _) = applet_button_size(&core, PanelIconStyle::BarsOnly, 2);
    let (w3, _) = applet_button_size(&core, PanelIconStyle::BarsOnly, 3);
    let (suggested_w, suggested_h) = core.applet.suggested_size(false);
    let bar_width = applet_bar_width(suggested_w, suggested_h);
    assert_eq!(w2 - w1, bar_width + APPLET_ACCOUNT_GAP);
    assert_eq!(w3 - w2, bar_width + APPLET_ACCOUNT_GAP);
    let (lw2, _) = applet_button_size(&core, PanelIconStyle::LogoAndBars, 2);
    let (lw1, _) = applet_button_size(&core, PanelIconStyle::LogoAndBars, 1);
    assert_eq!(lw2 - lw1, bar_width + APPLET_ACCOUNT_GAP);
}

#[test]
fn applet_button_size_percent_uses_fixed_slot_width() {
    let core = cosmic::Core::default();
    let cell = applet_percent_cell_width();
    let (w1, _) = applet_button_size(&core, PanelIconStyle::PercentOnly, 1);
    let (w2, _) = applet_button_size(&core, PanelIconStyle::PercentOnly, 2);
    let (w3, _) = applet_button_size(&core, PanelIconStyle::PercentOnly, 3);
    assert_eq!(w2 - w1, cell + APPLET_PERCENT_ACCOUNT_GAP);
    assert_eq!(w3 - w2, cell + APPLET_PERCENT_ACCOUNT_GAP);
}

#[test]
fn applet_button_size_logo_and_percent_uses_fixed_slot_width() {
    let core = cosmic::Core::default();
    let (percent_only, _) = applet_button_size(&core, PanelIconStyle::PercentOnly, 2);
    let (logo_percent, _) = applet_button_size(&core, PanelIconStyle::LogoAndPercent, 2);
    let (suggested_w, suggested_h) = core.applet.suggested_size(false);
    let logo_width = f32::from(suggested_w.min(suggested_h).saturating_sub(8).max(11));

    assert_eq!(logo_percent - percent_only, logo_width + APPLET_ICON_GAP);
}

#[test]
fn applet_button_size_percent_styles_ignore_current_percent_digits() {
    let core = cosmic::Core::default();
    let short_state = state_with_selected_account_percents(&[0.0, 8.5]);
    let wide_state = state_with_selected_account_percents(&[86.5, 100.0]);
    let short_n =
        selected_provider_all_percents(&short_state, ProviderId::Codex, UsageAmountFormat::Used)
            .len();
    let wide_n =
        selected_provider_all_percents(&wide_state, ProviderId::Codex, UsageAmountFormat::Used)
            .len();

    assert_eq!(short_n, 2);
    assert_eq!(wide_n, 2);
    assert_eq!(
        applet_button_size(&core, PanelIconStyle::PercentOnly, short_n),
        applet_button_size(&core, PanelIconStyle::PercentOnly, wide_n)
    );
    assert_eq!(
        applet_button_size(&core, PanelIconStyle::LogoAndPercent, short_n),
        applet_button_size(&core, PanelIconStyle::LogoAndPercent, wide_n)
    );
}

#[test]
fn applet_percent_groups_are_capped_to_four_selected_accounts() {
    let state = state_with_selected_account_percents(&[1.0, 2.0, 3.0, 4.0, 5.0]);

    let percents =
        selected_provider_all_percents(&state, ProviderId::Codex, UsageAmountFormat::Used);

    assert_eq!(percents.len(), 4);
    assert_eq!(percents.last().map(|&(p0, _)| p0), Some(4.0));
}

#[test]
fn popup_session_width_is_capped_to_four_selected_accounts() {
    let state = state_with_selected_account_percents(&[1.0, 2.0, 3.0, 4.0, 5.0]);

    let size = popup_session_size(&state, ProviderId::Codex);

    assert_eq!(size.width, POPUP_COLUMN_WIDTH * 4.0);
}

#[test]
fn applet_percent_cell_width_is_fixed_to_widest_normal_percent() {
    let expected =
        super::APPLET_PERCENT_CELL_HORIZONTAL_PAD + 6.0 * super::APPLET_PERCENT_GLYPH_WIDTH;

    assert_eq!(applet_percent_text(0.0), "0.0%");
    assert_eq!(applet_percent_text(86.5), "86.5%");
    assert_eq!(applet_percent_text(100.0), "100.0%");
    assert_eq!(applet_percent_cell_width(), expected);
}

#[test]
fn applet_percent_cells_left_align_text_in_fixed_slot() {
    assert_eq!(
        applet_percent_cell_alignment(),
        cosmic::iced::Alignment::Start
    );
}

#[test]
fn applet_percent_text_uses_one_decimal_through_100_percent() {
    assert_eq!(applet_percent_text(86.54), "86.5%");
    assert_eq!(applet_percent_text(100.0), "100.0%");
}

#[test]
fn selected_provider_all_percents_uses_first_panel_window() {
    let mut state = AppState::empty();
    let mut account = ProviderAccountRuntimeState::empty(ProviderId::Codex, "codex-1", "Codex");
    account.snapshot = Some(UsageSnapshot {
        provider: ProviderId::Codex,
        source: "test".to_string(),
        updated_at: Utc::now(),
        headline: UsageHeadline(0),
        windows: vec![
            UsageWindow {
                label: "Session".to_string(),
                used_percent: 86.5,
                reset_at: None,
                window_seconds: None,
                reset_description: None,
            },
            UsageWindow {
                label: "Weekly".to_string(),
                used_percent: 42.0,
                reset_at: None,
                window_seconds: None,
                reset_description: None,
            },
        ],
        provider_cost: None,
        extra_usage: None,
        identity: ProviderIdentity::default(),
    });

    state
        .provider_mut(ProviderId::Codex)
        .unwrap()
        .selected_account_ids = vec!["codex-1".to_string()];
    state.upsert_account(account);

    let percents_used =
        selected_provider_all_percents(&state, ProviderId::Codex, UsageAmountFormat::Used);
    assert_eq!(percents_used.first().map(|&(p0, _)| p0), Some(86.5));

    let percents_left =
        selected_provider_all_percents(&state, ProviderId::Codex, UsageAmountFormat::Left);
    assert_eq!(percents_left.first().map(|&(p0, _)| p0), Some(13.5));
}

fn state_with_selected_account_percents(percents: &[f32]) -> AppState {
    let mut state = AppState::empty();
    let selected_account_ids = percents
        .iter()
        .enumerate()
        .map(|(i, _)| format!("codex-{i}"))
        .collect::<Vec<_>>();
    state
        .provider_mut(ProviderId::Codex)
        .unwrap()
        .selected_account_ids = selected_account_ids;

    for (i, percent) in percents.iter().enumerate() {
        let id = format!("codex-{i}");
        let mut account =
            ProviderAccountRuntimeState::empty(ProviderId::Codex, id.clone(), "Codex");
        account.snapshot = Some(UsageSnapshot {
            provider: ProviderId::Codex,
            source: "test".to_string(),
            updated_at: Utc::now(),
            headline: UsageHeadline(0),
            windows: vec![UsageWindow {
                label: "Session".to_string(),
                used_percent: *percent,
                reset_at: None,
                window_seconds: None,
                reset_description: None,
            }],
            provider_cost: None,
            extra_usage: None,
            identity: ProviderIdentity::default(),
        });
        state.upsert_account(account);
    }

    state
}
