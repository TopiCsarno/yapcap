use super::{
    Duration, Id, Limits, Message, POPUP_MAX_HEIGHT, Size, Task, UPDATE_RETRY_INITIAL_SECS,
    UPDATE_RETRY_MAX_SECS, runtime,
};

pub(super) fn open_url(url: &str) {
    if let Err(e) = std::process::Command::new("xdg-open").arg(url).spawn() {
        tracing::warn!(url = %url, error = %e, "failed to open url");
    }
}

pub(super) fn update_check_task(attempt: u32) -> Task<Message> {
    Task::perform(
        async { crate::updates::check(&runtime::http_client()).await },
        move |status| cosmic::Action::App(Message::UpdateChecked { status, attempt }),
    )
}

pub(super) fn update_retry_task(attempt: u32, delay: Duration) -> Task<Message> {
    Task::perform(
        async move {
            tokio::time::sleep(delay).await;
            attempt
        },
        |attempt| cosmic::Action::App(Message::RetryUpdateCheck(attempt)),
    )
}

pub(super) fn update_retry_delay(attempt: u32) -> Duration {
    let exponent = attempt.saturating_sub(1).min(10);
    let secs = UPDATE_RETRY_INITIAL_SECS
        .saturating_mul(2_u64.saturating_pow(exponent))
        .min(UPDATE_RETRY_MAX_SECS);
    Duration::from_secs(secs)
}

pub(super) fn format_retry_delay(delay: Duration) -> String {
    let secs = delay.as_secs();
    if secs < 60 {
        return format!("{secs}s");
    }
    let minutes = secs / 60;
    let seconds = secs % 60;
    if seconds == 0 {
        return format!("{minutes}m");
    }
    format!("{minutes}m {seconds}s")
}

pub(super) fn resize_popup(id: Id, width: u32, height: u32) -> Task<Message> {
    cosmic::iced::platform_specific::shell::wayland::commands::popup::set_size(id, width, height)
}

pub(super) fn popup_size_limits_with_max_width(size: Size, max_width: f32) -> Limits {
    Limits::NONE
        .min_width(1.0)
        .max_width(max_width.max(size.width))
        .height(size.height.clamp(1.0, f32::from(POPUP_MAX_HEIGHT)))
}

pub(super) fn popup_size_tuple(size: Size) -> (u32, u32) {
    (
        rounded_dimension_to_u32(size.width),
        rounded_dimension_to_u32(size.height),
    )
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(super) fn rounded_dimension_to_u32(value: f32) -> u32 {
    const MAX_U32_F32: f32 = 4_294_967_295.0;

    if !value.is_finite() {
        return 0;
    }

    let rounded = value.round().clamp(0.0, MAX_U32_F32);
    rounded as u32
}
