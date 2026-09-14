use super::{
    AppletBarLayout, applet_bar_layout, applet_button_size, applet_fallback_button_size,
    applet_fallback_indicator, applet_indicator, applet_paddings,
};
use crate::app::{
    APPLET_CELL_SPACING, Alignment, AppState, Config, Element, Message, PanelIconStyle, ProviderId,
    row,
};
use crate::model::ProviderAccountRuntimeState;

pub(in crate::app) struct PanelCell<'a> {
    pub account: &'a ProviderAccountRuntimeState,
    pub layout: AppletBarLayout,
}

pub(in crate::app) fn panel_cells<'a>(state: &'a AppState, config: &Config) -> Vec<PanelCell<'a>> {
    let now = chrono::Utc::now();
    let mut cells = Vec::new();
    for provider in ProviderId::ALL {
        let Some(runtime) = state.provider(provider).filter(|runtime| runtime.enabled) else {
            continue;
        };
        let flagged = config.panel_account_ids(provider);
        for account in state.accounts_for(provider) {
            if !flagged.contains(&account.account_id) {
                continue;
            }
            let snapshot = account
                .snapshot
                .as_ref()
                .or(runtime.legacy_display_snapshot.as_ref());
            cells.push(PanelCell {
                account,
                layout: applet_bar_layout(
                    snapshot.and_then(|snapshot| snapshot.applet_windows()),
                    now,
                    config.usage_amount_format,
                ),
            });
        }
    }
    cells
}

pub(in crate::app) fn panel_indicator<'a>(
    state: &AppState,
    config: &Config,
    core: &cosmic::Core,
) -> Element<'a, Message> {
    let cells = panel_cells(state, config);
    let indicator = |cell: &PanelCell<'_>| {
        applet_indicator(
            cell.account.provider,
            cell.layout,
            config.panel_icon_style,
            core,
        )
    };
    match cells.as_slice() {
        [] => applet_fallback_indicator(core),
        [cell] => indicator(cell),
        _ => row(cells.iter().map(indicator))
            .spacing(APPLET_CELL_SPACING)
            .align_y(Alignment::Center)
            .into(),
    }
}

pub(in crate::app) fn panel_button_size(
    core: &cosmic::Core,
    state: &AppState,
    config: &Config,
) -> (f32, f32) {
    panel_cells_size(
        core,
        config.panel_icon_style,
        panel_cells(state, config).len(),
    )
}

pub(super) fn panel_cells_size(
    core: &cosmic::Core,
    style: PanelIconStyle,
    count: usize,
) -> (f32, f32) {
    if count == 0 {
        return applet_fallback_button_size(core);
    }
    let (single_width, height) = applet_button_size(core, style);
    let (horizontal_padding, _) = applet_paddings(core);
    let cell_width = single_width - f32::from(2 * horizontal_padding);
    let width = (1..count).fold(single_width, |width, _| {
        width + cell_width + APPLET_CELL_SPACING
    });
    (width, height)
}
