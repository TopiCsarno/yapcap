// SPDX-License-Identifier: MPL-2.0

pub mod account_storage;
mod app;
mod auth;
mod cache;
mod config;
#[cfg(debug_assertions)]
mod debug_env;
mod demo_env;
mod error;
mod i18n;
mod logging;
mod model;
mod providers;
mod runtime;
#[cfg(test)]
mod test_support;
mod updates;
mod usage_display;

fn main() -> cosmic::iced::Result {
    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();
    i18n::init(&requested_languages);

    let default_level = if cfg!(debug_assertions) {
        "debug"
    } else {
        "info"
    };
    let _log_guard = logging::init(default_level).ok();

    if running_in_cosmic_panel() {
        cosmic::applet::run::<app::AppModel>(app::LaunchMode::Panel)
    } else {
        cosmic::app::run::<app::AppModel>(app::applet_settings(), app::LaunchMode::Standalone)
    }
}

fn running_in_cosmic_panel() -> bool {
    std::env::vars().any(|(key, _)| key.starts_with("COSMIC_PANEL_"))
}
