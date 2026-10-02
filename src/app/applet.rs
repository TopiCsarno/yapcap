use super::provider_assets::app_icon_handle;
use super::{
    APPLET_BAR_WIDTH_HEIGHT_MULTIPLIER, APPLET_ICON_GAP, APPLET_PERCENT_CELL_HORIZONTAL_PAD,
    APPLET_PERCENT_GLYPH_WIDTH, Alignment, AppModel, AppState, Config, CosmicButton,
    CosmicConfigEntry, Element, Length, Limits, Message, PanelIconStyle, PanelValueDisplay,
    ProviderId, Size, UsageAmountFormat, progress_bar, provider_icon_handle, provider_icon_variant,
    row, usage_display, widget,
};
use crate::currency_format;
use crate::model::{AppletWindows, UsageSnapshot};

const APPLET_PRIMARY_BAR_GIRTH: f32 = 6.0;
const APPLET_SECONDARY_BAR_GIRTH: f32 = 3.0;
const APPLET_BAR_SPACING: f32 = 3.0;
const APPLET_BAR_STACK_HEIGHT: f32 =
    APPLET_PRIMARY_BAR_GIRTH + APPLET_BAR_SPACING + APPLET_SECONDARY_BAR_GIRTH;
pub(super) const APPLET_ALL_PROVIDERS_GAP: f32 = 10.0;
pub(super) const APPLET_DEFAULT_FONT_SIZE: f32 = 13.0;

#[derive(Debug, Clone, Copy)]
pub(super) struct PanelDisplayOptions {
    pub style: PanelIconStyle,
    pub value_display: PanelValueDisplay,
    pub show_all_providers: bool,
    pub show_all_accounts: bool,
    pub font_size: f32,
    pub usage_amount_format: UsageAmountFormat,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct AppletBarLayout {
    pub primary: f32,
    pub secondary: Option<f32>,
}

impl AppletBarLayout {
    fn empty_two_bar() -> Self {
        Self {
            primary: 0.0,
            secondary: Some(0.0),
        }
    }

    pub(super) fn single_bar(primary: f32) -> Self {
        Self {
            primary,
            secondary: None,
        }
    }

    pub(super) fn two_bar(primary: f32, secondary: f32) -> Self {
        Self {
            primary,
            secondary: Some(secondary),
        }
    }
}

pub(crate) fn applet_settings() -> cosmic::app::Settings {
    let preview_core = cosmic::Core::default();
    let config = crate::config::cosmic_config_context(
        <AppModel as cosmic::Application>::APP_ID,
        Config::VERSION,
    )
    .ok()
    .map(|ctx| match Config::get_entry(&ctx) {
        Ok(cfg) | Err((_, cfg)) => cfg,
    })
    .unwrap_or_default();
    let detection = crate::detection::startup_snapshot(crate::config::host_user_home_dir());
    let enabled_provider_count = enabled_provider_panel_count(&config, &detection);
    let font_size = f32::from(config.effective_panel_font_size());
    let options = PanelDisplayOptions {
        style: config.panel_icon_style,
        value_display: config.panel_value_display,
        show_all_providers: config.show_all_providers,
        show_all_accounts: config.show_all_accounts,
        font_size,
        usage_amount_format: config.usage_amount_format,
    };
    let (width, height) = if enabled_provider_count == 0 {
        applet_fallback_button_size(&preview_core)
    } else {
        applet_button_size(&preview_core, options, enabled_provider_count)
    };

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
        .default_text_size(font_size)
        .transparent(true)
}

pub(super) fn applet_indicator<'a>(
    state: &AppState,
    selected_provider: ProviderId,
    options: PanelDisplayOptions,
    core: &cosmic::Core,
) -> Element<'a, Message> {
    let (suggested_w, suggested_h) = core.applet.suggested_size(false);
    let compact_px = suggested_w.min(suggested_h);
    let logo_size_px = compact_px.saturating_sub(8).max(11);
    let logo_size = f32::from(logo_size_px);
    let bar_width = applet_bar_width(suggested_w, suggested_h);

    let segments = panel_segments(
        state,
        selected_provider,
        options.show_all_providers,
        options.show_all_accounts,
    );
    if segments.len() == 1 && !options.show_all_providers {
        let (provider, account_id) = &segments[0];
        return provider_indicator_segment(
            state,
            *provider,
            account_id.as_deref(),
            options,
            logo_size_px,
            logo_size,
            bar_width,
        );
    }

    let mut providers_row = row![]
        .spacing(APPLET_ALL_PROVIDERS_GAP)
        .align_y(Alignment::Center);
    for (provider_id, account_id) in &segments {
        providers_row = providers_row.push(provider_indicator_segment(
            state,
            *provider_id,
            account_id.as_deref(),
            options,
            logo_size_px,
            logo_size,
            bar_width,
        ));
    }
    providers_row.into()
}

fn provider_indicator_segment<'a>(
    state: &AppState,
    provider: ProviderId,
    account_id: Option<&str>,
    options: PanelDisplayOptions,
    logo_size_px: u16,
    logo_size: f32,
    bar_width: f32,
) -> Element<'a, Message> {
    let layout = account_bar_layout(state, provider, account_id, options.usage_amount_format);
    let bars = applet_bar_column(layout, bar_width);
    let value_text = applet_value_text(
        account_snapshot(state, provider, account_id),
        chrono::Utc::now(),
        options.usage_amount_format,
        options.value_display,
    );
    let value = applet_value_cell(value_text, options.font_size);

    match options.style {
        PanelIconStyle::LogoAndBars => {
            row![provider_logo(provider, logo_size_px, logo_size), bars,]
                .spacing(APPLET_ICON_GAP)
                .align_y(Alignment::Center)
                .into()
        }
        PanelIconStyle::BarsOnly => bars,
        PanelIconStyle::LogoAndPercent => {
            row![provider_logo(provider, logo_size_px, logo_size), value,]
                .spacing(APPLET_ICON_GAP)
                .align_y(Alignment::Center)
                .into()
        }
        PanelIconStyle::PercentOnly => value,
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

pub(super) fn panel_fallback_active(state: &AppState) -> bool {
    !state.provider_accounts.iter().any(|account| {
        state
            .provider(account.provider)
            .is_some_and(|provider| provider.enabled)
    })
}

pub(super) fn applet_fallback_indicator<'a>(core: &cosmic::Core) -> Element<'a, Message> {
    let icon_px = applet_fallback_icon_px(core);
    let icon_size = f32::from(icon_px);
    widget::icon::icon(app_icon_handle())
        .size(icon_px)
        .width(Length::Fixed(icon_size))
        .height(Length::Fixed(icon_size))
        .into()
}

pub(super) fn applet_fallback_button_size(core: &cosmic::Core) -> (f32, f32) {
    let (_, suggested_h) = core.applet.suggested_size(false);
    let (horizontal_padding, vertical_padding) = applet_paddings(core);
    let width = f32::from(applet_fallback_icon_px(core)) + f32::from(2 * horizontal_padding);
    let height = f32::from(suggested_h + 2 * vertical_padding);

    (width, height)
}

fn applet_fallback_icon_px(core: &cosmic::Core) -> u16 {
    let (suggested_w, suggested_h) = core.applet.suggested_size(false);
    suggested_w.min(suggested_h)
}

pub(super) fn panel_button_size(
    core: &cosmic::Core,
    state: &AppState,
    options: PanelDisplayOptions,
    selected_provider: ProviderId,
) -> (f32, f32) {
    if panel_fallback_active(state) {
        applet_fallback_button_size(core)
    } else {
        let (_, suggested_h) = core.applet.suggested_size(false);
        let (horizontal_padding, vertical_padding) = applet_paddings(core);
        let content_width = panel_content_width(core, state, options, selected_provider);
        let width = content_width + f32::from(2 * horizontal_padding);
        let content_height = suggested_h.max((options.font_size * 1.6).ceil() as u16);
        let height = f32::from(content_height + 2 * vertical_padding);

        (width, height)
    }
}

pub(super) fn panel_content_width(
    core: &cosmic::Core,
    state: &AppState,
    options: PanelDisplayOptions,
    selected_provider: ProviderId,
) -> f32 {
    let (suggested_w, suggested_h) = core.applet.suggested_size(false);
    let compact_px = suggested_w.min(suggested_h);
    let logo_width = f32::from(compact_px.saturating_sub(8).max(11));
    let bar_width = applet_bar_width(suggested_w, suggested_h);
    let now = chrono::Utc::now();
    let segments = panel_segments(
        state,
        selected_provider,
        options.show_all_providers,
        options.show_all_accounts,
    );
    let segment_width = |provider: ProviderId, account_id: Option<&str>| {
        let value_text = applet_value_text(
            account_snapshot(state, provider, account_id),
            now,
            options.usage_amount_format,
            options.value_display,
        );
        provider_segment_content_width(
            options.style,
            logo_width,
            bar_width,
            &value_text,
            options.font_size,
        )
    };

    let count = segments.len() as f32;
    let total: f32 = segments
        .iter()
        .map(|(provider, account_id)| segment_width(*provider, account_id.as_deref()))
        .sum();
    if segments.len() <= 1 {
        total
    } else {
        total + (count - 1.0) * APPLET_ALL_PROVIDERS_GAP
    }
}

pub(super) fn applet_button<'a>(
    core: &cosmic::Core,
    (width, height): (f32, f32),
    content: impl Into<Element<'a, Message>>,
) -> widget::Button<'a, Message> {
    let (horizontal_padding, _) = applet_paddings(core);

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
    options: PanelDisplayOptions,
    all_providers_count: usize,
) -> (f32, f32) {
    let (suggested_w, suggested_h) = core.applet.suggested_size(false);
    let (horizontal_padding, vertical_padding) = applet_paddings(core);
    let compact_px = suggested_w.min(suggested_h);
    let logo_width = f32::from(compact_px.saturating_sub(8).max(11));
    let bar_width = applet_bar_width(suggested_w, suggested_h);
    let segment_width = provider_segment_content_width(
        options.style,
        logo_width,
        bar_width,
        &applet_worst_case_value_text(options.value_display),
        options.font_size,
    );
    let content_width = if options.show_all_providers || options.show_all_accounts {
        let count = all_providers_count.max(1) as f32;
        count * segment_width + (count - 1.0) * APPLET_ALL_PROVIDERS_GAP
    } else {
        segment_width
    };
    let width = content_width + f32::from(2 * horizontal_padding);
    let content_height = suggested_h.max((options.font_size * 1.6).ceil() as u16);
    let height = f32::from(content_height + 2 * vertical_padding);

    (width, height)
}

fn applet_worst_case_value_text(display: PanelValueDisplay) -> String {
    let n_chars = match display {
        PanelValueDisplay::Percent => 6,
        PanelValueDisplay::Amount => 21,
        PanelValueDisplay::Both => 6 + 3 + 21,
    };
    "0".repeat(n_chars)
}

pub(super) fn provider_segment_content_width(
    style: PanelIconStyle,
    logo_width: f32,
    bar_width: f32,
    value_text: &str,
    font_size: f32,
) -> f32 {
    match style {
        PanelIconStyle::LogoAndBars => logo_width + APPLET_ICON_GAP + bar_width,
        PanelIconStyle::BarsOnly => bar_width,
        PanelIconStyle::LogoAndPercent => {
            logo_width + APPLET_ICON_GAP + applet_value_cell_width_for_text(value_text, font_size)
        }
        PanelIconStyle::PercentOnly => applet_value_cell_width_for_text(value_text, font_size),
    }
}

pub(super) fn enabled_provider_panel_count(
    config: &Config,
    detection: &crate::detection::DetectionSnapshot,
) -> usize {
    ProviderId::ALL
        .iter()
        .filter(|&&provider| {
            crate::provider_enablement::provider_enabled(config, detection, provider)
                && !config.selected_account_ids(provider).is_empty()
        })
        .count()
}

fn applet_paddings(core: &cosmic::Core) -> (u16, u16) {
    let (major_padding, minor_padding) = core.applet.suggested_padding(false);
    if core.applet.is_horizontal() {
        (major_padding, minor_padding)
    } else {
        (minor_padding, major_padding)
    }
}

pub(super) fn applet_bar_width(suggested_w: u16, suggested_h: u16) -> f32 {
    let min_width = suggested_h.saturating_mul(APPLET_BAR_WIDTH_HEIGHT_MULTIPLIER);

    f32::from(suggested_w.max(min_width))
}

pub(super) fn applet_percent_text(percent: f32) -> String {
    format!("{percent:.1}%")
}

#[cfg(test)]
pub(super) fn applet_percent_cell_width() -> f32 {
    let n_chars = u8::try_from(applet_percent_text(100.0).chars().count()).unwrap_or(u8::MAX);
    f32::from(n_chars) * APPLET_PERCENT_GLYPH_WIDTH + APPLET_PERCENT_CELL_HORIZONTAL_PAD
}

pub(super) fn applet_value_cell_width_for_text(text: &str, font_size: f32) -> f32 {
    let n_chars = text.chars().count().max(4);
    let scale = font_size / APPLET_DEFAULT_FONT_SIZE;
    n_chars as f32 * APPLET_PERCENT_GLYPH_WIDTH * scale + APPLET_PERCENT_CELL_HORIZONTAL_PAD
}

pub(super) fn applet_value_text(
    snapshot: Option<&UsageSnapshot>,
    now: chrono::DateTime<chrono::Utc>,
    usage_amount_format: UsageAmountFormat,
    panel_value_display: PanelValueDisplay,
) -> String {
    let displayed_percent = snapshot
        .and_then(|snapshot| snapshot.applet_windows())
        .map(|windows| {
            usage_display::displayed_amount_percent(windows.primary, now, usage_amount_format)
        })
        .unwrap_or(0.0);
    let percent = applet_percent_text(displayed_percent);
    let amount = snapshot
        .and_then(|snapshot| snapshot.provider_cost.as_ref())
        .map(currency_format::format_panel_amount);
    match panel_value_display {
        PanelValueDisplay::Percent => percent,
        PanelValueDisplay::Amount => amount.unwrap_or(percent),
        PanelValueDisplay::Both => match amount {
            Some(amount) => format!("{percent} · {amount}"),
            None => percent,
        },
    }
}

pub(super) fn applet_percent_cell_alignment() -> Alignment {
    Alignment::Start
}

fn applet_bar_column(layout: AppletBarLayout, bar_width: f32) -> Element<'static, Message> {
    let primary = progress_bar(0.0..=100.0, layout.primary)
        .girth(Length::Fixed(APPLET_PRIMARY_BAR_GIRTH))
        .length(Length::Fixed(bar_width));
    let content: Element<'static, Message> = match layout.secondary {
        Some(secondary) => cosmic::iced::widget::column![
            primary,
            progress_bar(0.0..=100.0, secondary)
                .girth(Length::Fixed(APPLET_SECONDARY_BAR_GIRTH))
                .length(Length::Fixed(bar_width)),
        ]
        .spacing(APPLET_BAR_SPACING)
        .width(Length::Fixed(bar_width))
        .into(),
        None => primary.into(),
    };

    widget::container(content)
        .width(Length::Fixed(bar_width))
        .height(Length::Fixed(APPLET_BAR_STACK_HEIGHT))
        .align_y(cosmic::iced::alignment::Vertical::Center)
        .into()
}

fn applet_value_cell(text: String, font_size: f32) -> Element<'static, Message> {
    let width = applet_value_cell_width_for_text(&text, font_size);
    widget::container(widget::text(text).size(font_size))
        .width(Length::Fixed(width))
        .align_x(applet_percent_cell_alignment())
        .into()
}

pub(super) fn selected_provider_snapshot(
    state: &AppState,
    selected_provider: ProviderId,
) -> Option<&UsageSnapshot> {
    state
        .active_account(selected_provider)
        .and_then(|account| account.snapshot.as_ref())
        .or_else(|| {
            state
                .provider(selected_provider)
                .and_then(|provider| provider.legacy_display_snapshot.as_ref())
        })
}

pub(super) fn account_snapshot<'a>(
    state: &'a AppState,
    provider: ProviderId,
    account_id: Option<&str>,
) -> Option<&'a UsageSnapshot> {
    match account_id {
        Some(account_id) => state
            .provider_accounts
            .iter()
            .find(|entry| entry.provider == provider && entry.account_id == account_id)
            .and_then(|account| account.snapshot.as_ref())
            .or_else(|| {
                state
                    .provider(provider)
                    .and_then(|provider| provider.legacy_display_snapshot.as_ref())
            }),
        None => selected_provider_snapshot(state, provider),
    }
}

pub(super) fn panel_segments(
    state: &AppState,
    selected_provider: ProviderId,
    show_all_providers: bool,
    show_all_accounts: bool,
) -> Vec<(ProviderId, Option<String>)> {
    let providers: Vec<ProviderId> = if show_all_providers {
        ProviderId::ALL
            .iter()
            .copied()
            .filter(|provider_id| {
                state
                    .providers
                    .iter()
                    .any(|p| p.provider == *provider_id && p.enabled)
            })
            .collect()
    } else {
        vec![selected_provider]
    };
    let providers = if providers.is_empty() {
        vec![selected_provider]
    } else {
        providers
    };

    let mut segments = Vec::new();
    for provider in providers {
        if show_all_accounts {
            let accounts = state.accounts_for(provider);
            if accounts.is_empty() {
                segments.push((provider, None));
            } else {
                for account in accounts {
                    segments.push((provider, Some(account.account_id.clone())));
                }
            }
        } else {
            segments.push((provider, None));
        }
    }
    segments
}

pub(super) fn account_bar_layout(
    state: &AppState,
    provider: ProviderId,
    account_id: Option<&str>,
    usage_amount_format: UsageAmountFormat,
) -> AppletBarLayout {
    let now = chrono::Utc::now();
    let snapshot = account_snapshot(state, provider, account_id);
    applet_bar_layout(
        snapshot.and_then(|snapshot| snapshot.applet_windows()),
        now,
        usage_amount_format,
    )
}

#[cfg(test)]
pub(super) fn selected_provider_bar_layout(
    state: &AppState,
    selected_provider: ProviderId,
    usage_amount_format: UsageAmountFormat,
) -> AppletBarLayout {
    account_bar_layout(state, selected_provider, None, usage_amount_format)
}

pub(super) fn applet_bar_layout(
    windows: Option<AppletWindows<'_>>,
    now: chrono::DateTime<chrono::Utc>,
    usage_amount_format: UsageAmountFormat,
) -> AppletBarLayout {
    let Some(windows) = windows else {
        return AppletBarLayout::empty_two_bar();
    };
    let primary =
        usage_display::displayed_amount_percent(windows.primary, now, usage_amount_format);
    match windows.secondary {
        Some(secondary) => AppletBarLayout::two_bar(
            primary,
            usage_display::displayed_amount_percent(secondary, now, usage_amount_format),
        ),
        None => AppletBarLayout::single_bar(primary),
    }
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
