// SPDX-License-Identifier: MPL-2.0

mod app;
mod app_refresh;
mod app_state;
mod auth;
mod browser;
mod cache;
mod config;
mod error;
mod i18n;
mod logging;
mod model;
mod popup_view;
mod provider_assets;
mod providers;
mod runtime;
mod updates;
mod usage_display;

fn main() -> cosmic::iced::Result {
    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();
    i18n::init(&requested_languages);

    let _log_guard = logging::init("info").ok();

    if running_in_cosmic_panel() {
        cosmic::applet::run::<app::AppModel>(app::LaunchMode::Panel)
    } else {
        cosmic::app::run::<app::AppModel>(app::applet_settings(), app::LaunchMode::Standalone)
    }
}

fn running_in_cosmic_panel() -> bool {
    std::env::vars().any(|(key, _)| key.starts_with("COSMIC_PANEL_"))
}
