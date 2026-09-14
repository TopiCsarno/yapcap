mod cells;

#[cfg(test)]
pub(super) use cells::panel_cells;
pub(super) use cells::{panel_button_size, panel_indicator};

use super::provider_assets::app_icon_handle;
use super::{
    APPLET_BAR_WIDTH_HEIGHT_MULTIPLIER, APPLET_ICON_GAP, APPLET_PERCENT_CELL_HORIZONTAL_PAD,
    APPLET_PERCENT_GLYPH_WIDTH, Alignment, AppModel, AppState, Config, CosmicButton,
    CosmicConfigEntry, Element, Length, Limits, Message, PanelIconStyle, ProviderId, Size,
    UsageAmountFormat, progress_bar, provider_icon_handle, provider_icon_variant, row,
    usage_display, widget,
};
use crate::model::AppletWindows;

const APPLET_PRIMARY_BAR_GIRTH: f32 = 6.0;
const APPLET_SECONDARY_BAR_GIRTH: f32 = 3.0;
const APPLET_BAR_SPACING: f32 = 3.0;
const APPLET_BAR_STACK_HEIGHT: f32 =
    APPLET_PRIMARY_BAR_GIRTH + APPLET_BAR_SPACING + APPLET_SECONDARY_BAR_GIRTH;

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
    let cell_count = ProviderId::ALL
        .into_iter()
        .filter(|&provider| {
            crate::provider_enablement::provider_enabled(&config, &detection, provider)
        })
        .map(|provider| config.panel_account_ids(provider).len())
        .sum();
    let (width, height) =
        cells::panel_cells_size(&preview_core, config.panel_icon_style, cell_count);

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

fn applet_indicator<'a>(
    provider: ProviderId,
    layout: AppletBarLayout,
    style: PanelIconStyle,
    core: &cosmic::Core,
) -> Element<'a, Message> {
    let (suggested_w, suggested_h) = core.applet.suggested_size(false);
    let compact_px = suggested_w.min(suggested_h);
    let logo_size_px = compact_px.saturating_sub(8).max(11);
    let logo_size = f32::from(logo_size_px);
    let bar_width = applet_bar_width(suggested_w, suggested_h);
    let bars = applet_bar_column(layout, bar_width);
    let percent = account_percent(layout);

    match style {
        PanelIconStyle::LogoAndBars => {
            row![provider_logo(provider, logo_size_px, logo_size), bars,]
                .spacing(6)
                .align_y(Alignment::Center)
                .into()
        }
        PanelIconStyle::BarsOnly => bars,
        PanelIconStyle::LogoAndPercent => {
            row![provider_logo(provider, logo_size_px, logo_size), percent,]
                .spacing(6)
                .align_y(Alignment::Center)
                .into()
        }
        PanelIconStyle::PercentOnly => percent,
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

pub(super) fn applet_button_size(core: &cosmic::Core, style: PanelIconStyle) -> (f32, f32) {
    let (suggested_w, suggested_h) = core.applet.suggested_size(false);
    let (horizontal_padding, vertical_padding) = applet_paddings(core);
    let compact_px = suggested_w.min(suggested_h);
    let logo_width = f32::from(compact_px.saturating_sub(8).max(11));
    let bar_width = applet_bar_width(suggested_w, suggested_h);
    let content_width = match style {
        PanelIconStyle::LogoAndBars => logo_width + APPLET_ICON_GAP + bar_width,
        PanelIconStyle::BarsOnly => bar_width,
        PanelIconStyle::LogoAndPercent => {
            logo_width + APPLET_ICON_GAP + applet_percent_cell_width()
        }
        PanelIconStyle::PercentOnly => applet_percent_cell_width(),
    };
    let width = content_width + f32::from(2 * horizontal_padding);
    let height = f32::from(suggested_h + 2 * vertical_padding);

    (width, height)
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

pub(super) fn applet_percent_cell_width() -> f32 {
    let n_chars = u8::try_from(applet_percent_text(100.0).chars().count()).unwrap_or(u8::MAX);
    f32::from(n_chars) * APPLET_PERCENT_GLYPH_WIDTH + APPLET_PERCENT_CELL_HORIZONTAL_PAD
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

fn account_percent(layout: AppletBarLayout) -> Element<'static, Message> {
    widget::container(widget::text(applet_percent_text(layout.primary)).size(13))
        .width(Length::Fixed(applet_percent_cell_width()))
        .align_x(applet_percent_cell_alignment())
        .into()
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
