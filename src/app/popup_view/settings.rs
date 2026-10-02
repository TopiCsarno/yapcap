mod about;
mod accounts;
mod general;

use super::{
    Alignment, AppState, Background, Config, DetectionSnapshot, Element, Length, Message,
    ProviderId, ProviderLoginStates, UpdateStatus, component_container_style,
    component_divider_color, container, fl, provider_icon_handle, provider_icon_variant, widget,
};

pub(crate) fn general_settings_view<'a>(config: &'a Config) -> Element<'a, Message> {
    general::general_settings_view(config)
}

pub(super) fn manage_providers_view(state: &AppState) -> Element<'static, Message> {
    let providers = ProviderId::ALL;
    let mut rows = cosmic::iced::widget::column![].width(Length::Fill);
    for (index, provider_id) in providers.into_iter().enumerate() {
        let enabled = state
            .provider(provider_id)
            .is_some_and(|provider| provider.enabled);
        rows = rows.push(manage_provider_row(provider_id, enabled));
        if index + 1 < providers.len() {
            rows = rows.push(manage_provider_divider());
        }
    }
    Element::from(
        cosmic::iced::widget::column![
            cosmic::widget::text(fl!("manage-providers")).size(20),
            container(rows)
                .width(Length::Fill)
                .style(manage_provider_list_style),
        ]
        .spacing(16)
        .width(Length::Fill),
    )
}

fn manage_provider_row(provider: ProviderId, enabled: bool) -> Element<'static, Message> {
    container(
        cosmic::iced::widget::row![
            widget::icon::icon(provider_icon_handle(provider, provider_icon_variant())).size(20),
            cosmic::widget::text(provider.label()).size(16),
            cosmic::iced::widget::Space::new().width(Length::Fill),
            cosmic::widget::toggler(enabled)
                .on_toggle(move |enabled| Message::SetProviderEnabled(provider, enabled)),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .width(Length::Fill),
    )
    .padding([12, 12])
    .width(Length::Fill)
    .into()
}

fn manage_provider_divider() -> Element<'static, Message> {
    container(cosmic::iced::widget::Space::new().height(Length::Fixed(1.0)))
        .width(Length::Fill)
        .style(|theme: &cosmic::Theme| widget::container::Style {
            text_color: None,
            background: Some(Background::Color(component_divider_color(theme))),
            border: cosmic::iced::Border::default(),
            shadow: cosmic::iced::Shadow::default(),
            icon_color: None,
            snap: true,
        })
        .into()
}

fn manage_provider_list_style(theme: &cosmic::Theme) -> widget::container::Style {
    component_container_style(theme)
}

pub(super) fn about_view(update_status: &UpdateStatus) -> Element<'static, Message> {
    about::about_view(update_status)
}

pub(super) fn provider_settings_view<'a>(
    state: &'a AppState,
    config: &'a Config,
    detection: &'a DetectionSnapshot,
    logins: ProviderLoginStates<'a>,
    provider_id: ProviderId,
) -> Element<'a, Message> {
    accounts::provider_settings_view(state, config, detection, logins, provider_id)
}
