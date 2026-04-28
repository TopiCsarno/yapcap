use super::{
    APPLET_ACCOUNT_GAP, APPLET_BAR_WIDTH_HEIGHT_MULTIPLIER, APPLET_ICON_GAP,
    APPLET_PERCENT_TEXT_WIDTH, Alignment, AppModel, AppState, Config, CosmicButton,
    CosmicConfigEntry, Element, Length, Limits, Message, PanelIconStyle, ProviderId, Size,
    UsageAmountFormat, cosmic_config, progress_bar, provider_icon_handle, provider_icon_variant,
    row, usage_display, widget,
};

pub(crate) fn applet_settings() -> cosmic::app::Settings {
    let preview_core = cosmic::Core::default();
    let config =
        cosmic_config::Config::new(<AppModel as cosmic::Application>::APP_ID, Config::VERSION)
            .ok()
            .map(|ctx| match Config::get_entry(&ctx) {
                Ok(cfg) | Err((_, cfg)) => cfg,
            })
            .unwrap_or_default();
    let n_accounts = ProviderId::ALL
        .iter()
        .filter(|&&p| config.provider_enabled(p))
        .map(|&p| config.selected_account_ids(p).len().max(1))
        .max()
        .unwrap_or(1);
    let (width, height) = applet_button_size(&preview_core, config.panel_icon_style, n_accounts);

    cosmic::app::Settings::default()
        .size(Size::new(width, height))
        .size_limits(
            Limits::NONE
                .min_width(width)
                .max_width(width)
                .min_height(height)
                .max_height(height),
        )
        .resizable(None)
        .client_decorations(false)
        .default_text_size(14.0)
        .transparent(true)
}

pub(super) fn applet_indicator<'a>(
    state: &AppState,
    selected_provider: ProviderId,
    style: PanelIconStyle,
    usage_amount_format: UsageAmountFormat,
    core: &cosmic::Core,
    n_accounts: usize,
) -> Element<'a, Message> {
    let (suggested_w, suggested_h) = core.applet.suggested_size(false);
    let compact_px = suggested_w.min(suggested_h);
    let logo_size_px = compact_px.saturating_sub(8).max(11);
    let logo_size = f32::from(logo_size_px);
    let bar_width = applet_bar_width(suggested_w, suggested_h);
    let percent = selected_provider_percent(state, selected_provider, usage_amount_format);

    let account_percents =
        selected_provider_all_percents(state, selected_provider, usage_amount_format);

    let bars_row = {
        let mut r = row![].align_y(Alignment::Center);
        for i in 0..n_accounts {
            if i > 0 {
                r = r.push(
                    cosmic::iced::widget::Space::new().width(Length::Fixed(APPLET_ACCOUNT_GAP)),
                );
            }
            let (p0, p1) = account_percents.get(i).copied().unwrap_or((0.0, 0.0));
            r = r.push(
                cosmic::iced::widget::column![
                    progress_bar(0.0..=100.0, p0).girth(Length::Fixed(6.0)),
                    progress_bar(0.0..=100.0, p1).girth(Length::Fixed(3.0)),
                ]
                .spacing(3)
                .width(Length::Fixed(bar_width)),
            );
        }
        r
    };

    match style {
        PanelIconStyle::LogoAndBars => row![
            provider_logo(selected_provider, logo_size_px, logo_size),
            bars_row,
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into(),
        PanelIconStyle::BarsOnly => bars_row.into(),
        PanelIconStyle::LogoAndPercent => row![
            provider_logo(selected_provider, logo_size_px, logo_size),
            widget::text(applet_percent_text(percent)).size(13),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into(),
        PanelIconStyle::PercentOnly => widget::text(applet_percent_text(percent)).size(13).into(),
    }
}

pub(super) fn provider_logo<'a>(
    provider: ProviderId,
    logo_size_px: u16,
    logo_size: f32,
) -> Element<'a, Message> {
    widget::icon::icon(provider_icon_handle(provider, provider_icon_variant()))
        .size(logo_size_px)
        .width(Length::Fixed(logo_size))
        .height(Length::Fixed(logo_size))
        .into()
}

pub(super) fn applet_button<'a>(
    core: &cosmic::Core,
    style: PanelIconStyle,
    n_accounts: usize,
    content: impl Into<Element<'a, Message>>,
) -> widget::Button<'a, Message> {
    let (major_padding, minor_padding) = core.applet.suggested_padding(true);
    let horizontal_padding = if core.applet.is_horizontal() {
        major_padding
    } else {
        minor_padding
    };
    let (width, height) = applet_button_size(core, style, n_accounts);

    widget::button::custom(
        widget::layer_container(content)
            .padding(cosmic::iced::Padding::from([0, horizontal_padding]))
            .align_y(cosmic::iced::alignment::Vertical::Center.into()),
    )
    .padding(0)
    .width(Length::Fixed(width))
    .height(Length::Fixed(height))
    .class(CosmicButton::AppletIcon)
}

pub(super) fn applet_button_size(
    core: &cosmic::Core,
    style: PanelIconStyle,
    n_accounts: usize,
) -> (f32, f32) {
    let (suggested_w, suggested_h) = core.applet.suggested_size(false);
    let (major_padding, minor_padding) = core.applet.suggested_padding(true);
    let (horizontal_padding, vertical_padding) = if core.applet.is_horizontal() {
        (major_padding, minor_padding)
    } else {
        (minor_padding, major_padding)
    };
    let compact_px = suggested_w.min(suggested_h);
    let logo_width = f32::from(compact_px.saturating_sub(8).max(11));
    let bar_width = applet_bar_width(suggested_w, suggested_h);
    let n = f32::from(u8::try_from(n_accounts.max(1)).unwrap_or(u8::MAX));
    let bars_total = n * bar_width + (n - 1.0) * APPLET_ACCOUNT_GAP;
    let content_width = match style {
        PanelIconStyle::LogoAndBars => logo_width + APPLET_ICON_GAP + bars_total,
        PanelIconStyle::BarsOnly => bars_total,
        PanelIconStyle::LogoAndPercent => logo_width + APPLET_ICON_GAP + APPLET_PERCENT_TEXT_WIDTH,
        PanelIconStyle::PercentOnly => APPLET_PERCENT_TEXT_WIDTH,
    };
    let width = content_width + f32::from(2 * horizontal_padding);
    let height = f32::from(suggested_h + 2 * vertical_padding);

    (width, height)
}

pub(super) fn applet_bar_width(suggested_w: u16, suggested_h: u16) -> f32 {
    let min_width = suggested_h.saturating_mul(APPLET_BAR_WIDTH_HEIGHT_MULTIPLIER);

    f32::from(suggested_w.max(min_width))
}

pub(super) fn applet_percent_text(percent: f32) -> String {
    format!("{percent:.1}%")
}

pub(super) fn selected_provider_all_percents(
    state: &AppState,
    selected_provider: ProviderId,
    usage_amount_format: UsageAmountFormat,
) -> Vec<(f32, f32)> {
    let now = chrono::Utc::now();
    let accounts = state.selected_accounts(selected_provider);
    if accounts.is_empty() {
        let snapshot = state
            .provider(selected_provider)
            .and_then(|p| p.legacy_display_snapshot.as_ref());
        let (w0, w1) = snapshot.map_or((None, None), |s| s.applet_windows());
        let p0 = w0.map_or(0.0, |w| {
            usage_display::displayed_amount_percent(w, now, usage_amount_format)
        });
        let p1 = w1.map_or(0.0, |w| {
            usage_display::displayed_amount_percent(w, now, usage_amount_format)
        });
        return vec![(p0, p1)];
    }
    accounts
        .iter()
        .map(|account| {
            let (w0, w1) = account
                .snapshot
                .as_ref()
                .map_or((None, None), |s| s.applet_windows());
            let p0 = w0.map_or(0.0, |w| {
                usage_display::displayed_amount_percent(w, now, usage_amount_format)
            });
            let p1 = w1.map_or(0.0, |w| {
                usage_display::displayed_amount_percent(w, now, usage_amount_format)
            });
            (p0, p1)
        })
        .collect()
}

pub(super) fn selected_provider_percent(
    state: &AppState,
    selected_provider: ProviderId,
    usage_amount_format: UsageAmountFormat,
) -> f32 {
    let now = chrono::Utc::now();
    state
        .providers
        .iter()
        .find(|p| p.provider == selected_provider)
        .and_then(|p| {
            state
                .active_account(p.provider)
                .and_then(|account| account.snapshot.as_ref())
                .or(p.legacy_display_snapshot.as_ref())
        })
        .and_then(|snapshot| snapshot.applet_windows().0)
        .map_or(0.0, |window| {
            usage_display::displayed_amount_percent(window, now, usage_amount_format)
        })
}

pub(super) fn select_provider(current: ProviderId, state: &AppState) -> ProviderId {
    if state
        .providers
        .iter()
        .any(|p| p.provider == current && p.enabled)
    {
        current
    } else {
        state
            .providers
            .iter()
            .find(|p| p.enabled)
            .map_or(ProviderId::Codex, |p| p.provider)
    }
}
