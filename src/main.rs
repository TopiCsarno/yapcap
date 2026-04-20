// SPDX-License-Identifier: MPL-2.0

mod app;
mod app_refresh;
mod i18n;
mod popup_view;
mod provider_assets;

fn main() -> cosmic::iced::Result {
    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();
    i18n::init(&requested_languages);

    // Guard must stay alive until process exits to flush background log writer.
    let _log_guard = yapcap::logging::init("info").ok();

    cosmic::applet::run::<app::AppModel>(())
}
