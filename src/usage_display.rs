// SPDX-License-Identifier: MPL-2.0

use crate::config::{ResetTimeFormat, UsageAmountFormat};
use crate::fl;
use crate::model::UsageWindow;
use chrono::{DateTime, Local, Utc};

const PACE_ON_TRACK_THRESHOLD: f32 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UsagePace {
    pub expected_percent: f32,
    pub delta_percent: f32,
}

#[must_use]
pub fn displayed_percent(window: &UsageWindow, now: DateTime<Utc>) -> f32 {
    if is_elapsed(window, now) {
        0.0
    } else {
        window.used_percent.clamp(0.0, 100.0)
    }
}

#[must_use]
pub fn pace(window: &UsageWindow, now: DateTime<Utc>) -> Option<UsagePace> {
    let reset_at = window.reset_at?;
    let window_seconds = window.window_seconds?;
    if window_seconds <= 0 || reset_at <= now {
        return None;
    }
    let window_duration = std::time::Duration::from_secs(u64::try_from(window_seconds).ok()?);
    let remaining_duration = (reset_at - now).to_std().ok()?;
    let elapsed_duration = window_duration.saturating_sub(remaining_duration);
    let expected_percent = ((elapsed_duration.as_secs_f32() / window_duration.as_secs_f32())
        * 100.0)
        .clamp(0.0, 100.0);
    let used_percent = displayed_percent(window, now);
    Some(UsagePace {
        expected_percent,
        delta_percent: used_percent - expected_percent,
    })
}

#[must_use]
pub fn pace_label(pace: UsagePace) -> String {
    let delta = pace.delta_percent;
    if delta.abs() < PACE_ON_TRACK_THRESHOLD {
        fl!("pace-on-track")
    } else {
        let percent = format!("{:.0}%", delta.abs());
        if delta.is_sign_positive() {
            fl!("pace-ahead", percent = percent.as_str())
        } else {
            fl!("pace-room", percent = percent.as_str())
        }
    }
}

#[must_use]
pub fn displayed_amount_percent(
    window: &UsageWindow,
    now: DateTime<Utc>,
    format: UsageAmountFormat,
) -> f32 {
    let used = displayed_percent(window, now);
    match format {
        UsageAmountFormat::Used => used,
        UsageAmountFormat::Left => 100.0 - used,
    }
}

#[must_use]
pub fn usage_amount_label(
    window: &UsageWindow,
    now: DateTime<Utc>,
    format: UsageAmountFormat,
) -> String {
    let percent = format!("{:.1}%", displayed_amount_percent(window, now, format));
    match format {
        UsageAmountFormat::Used => fl!("usage-used-label", percent = percent.as_str()),
        UsageAmountFormat::Left => fl!("usage-left-label", percent = percent.as_str()),
    }
}

#[must_use]
pub fn reset_label(
    window: &UsageWindow,
    now: DateTime<Utc>,
    format: ResetTimeFormat,
) -> Option<String> {
    if is_elapsed(window, now) || is_inactive_session(window) {
        return Some(fl!("reset-now"));
    }
    if let Some(reset_at) = window.reset_at {
        return Some(match format {
            ResetTimeFormat::Relative => format_relative_reset_label(reset_at, now),
            ResetTimeFormat::Absolute => format_absolute_reset_label(reset_at, now),
        });
    }
    None
}

fn is_elapsed(window: &UsageWindow, now: DateTime<Utc>) -> bool {
    window.reset_at.is_some_and(|reset_at| reset_at <= now)
}

fn is_inactive_session(window: &UsageWindow) -> bool {
    window.label == "Session" && window.used_percent <= 0.0
}

fn format_relative_reset_label(reset_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let remaining = reset_at - now;
    if remaining.num_seconds() <= 0 {
        return fl!("reset-now");
    }
    let days = remaining.num_days();
    let hours = remaining.num_hours() % 24;
    let mins = remaining.num_minutes() % 60;
    if days > 0 {
        fl!("resets-in-days-hours", days = days, hours = hours)
    } else if hours > 0 {
        fl!("resets-in-hours-minutes", hours = hours, mins = mins)
    } else {
        fl!("resets-in-minutes", mins = mins)
    }
}

fn format_absolute_reset_label(reset_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let reset_at = reset_at.with_timezone(&Local);
    let now = now.with_timezone(&Local);
    let days = reset_at
        .date_naive()
        .signed_duration_since(now.date_naive())
        .num_days();
    let time = reset_at.format("%-I:%M %p").to_string();

    match days {
        0 => fl!("resets-today-at", time = time),
        1 => fl!("resets-tomorrow-at", time = time),
        2..=6 => fl!(
            "resets-weekday-at",
            weekday = reset_at.format("%A").to_string(),
            time = time
        ),
        _ => fl!(
            "resets-date-at",
            date = reset_at.format("%b %-e").to_string(),
            time = time
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn strip_isolation_marks(s: &str) -> String {
        s.replace(['\u{2068}', '\u{2069}'], "")
    }

    fn window(reset_at: Option<DateTime<Utc>>, used_percent: f32) -> UsageWindow {
        UsageWindow {
            label: "Session".to_string(),
            used_percent,
            reset_at,
            window_seconds: None,
            reset_description: None,
        }
    }

    fn paced_window(
        reset_at: DateTime<Utc>,
        window_seconds: i64,
        used_percent: f32,
    ) -> UsageWindow {
        UsageWindow {
            label: "Weekly".to_string(),
            used_percent,
            reset_at: Some(reset_at),
            window_seconds: Some(window_seconds),
            reset_description: None,
        }
    }

    #[test]
    fn zeroes_expired_window_percent() {
        let now = Utc.with_ymd_and_hms(2026, 4, 12, 16, 51, 55).unwrap();
        let reset_at = Utc.with_ymd_and_hms(2026, 4, 12, 12, 0, 0).unwrap();
        assert_eq!(displayed_percent(&window(Some(reset_at), 51.0), now), 0.0);
    }

    #[test]
    fn clamps_active_window_percent() {
        let now = Utc.with_ymd_and_hms(2026, 4, 12, 16, 51, 55).unwrap();
        let reset_at = Utc.with_ymd_and_hms(2026, 4, 12, 17, 0, 0).unwrap();
        assert_eq!(
            displayed_percent(&window(Some(reset_at), 120.0), now),
            100.0
        );
        assert_eq!(displayed_percent(&window(Some(reset_at), -5.0), now), 0.0);
    }

    #[test]
    fn formats_used_usage_amount() {
        let now = Utc.with_ymd_and_hms(2026, 4, 12, 16, 51, 55).unwrap();
        let reset_at = Utc.with_ymd_and_hms(2026, 4, 12, 17, 0, 0).unwrap();
        let usage = window(Some(reset_at), 61.2);

        assert!(
            (displayed_amount_percent(&usage, now, UsageAmountFormat::Used) - 61.2).abs() < 0.001
        );
        assert_eq!(
            strip_isolation_marks(&usage_amount_label(&usage, now, UsageAmountFormat::Used)),
            "61.2% used"
        );
    }

    #[test]
    fn formats_left_usage_amount() {
        let now = Utc.with_ymd_and_hms(2026, 4, 12, 16, 51, 55).unwrap();
        let reset_at = Utc.with_ymd_and_hms(2026, 4, 12, 17, 0, 0).unwrap();
        let usage = window(Some(reset_at), 61.2);

        assert!(
            (displayed_amount_percent(&usage, now, UsageAmountFormat::Left) - 38.8).abs() < 0.001
        );
        assert_eq!(
            strip_isolation_marks(&usage_amount_label(&usage, now, UsageAmountFormat::Left)),
            "38.8% left"
        );
    }

    #[test]
    fn left_usage_amount_respects_elapsed_windows() {
        let now = Utc.with_ymd_and_hms(2026, 4, 12, 16, 51, 55).unwrap();
        let reset_at = Utc.with_ymd_and_hms(2026, 4, 12, 12, 0, 0).unwrap();

        assert_eq!(
            displayed_amount_percent(&window(Some(reset_at), 61.25), now, UsageAmountFormat::Left),
            100.0
        );
    }

    #[test]
    fn marks_expired_window_as_reset() {
        let now = Utc.with_ymd_and_hms(2026, 4, 12, 16, 51, 55).unwrap();
        let reset_at = Utc.with_ymd_and_hms(2026, 4, 12, 12, 0, 0).unwrap();
        assert_eq!(
            reset_label(
                &window(Some(reset_at), 51.0),
                now,
                ResetTimeFormat::Relative
            )
            .as_deref(),
            Some("Reset")
        );
    }

    #[test]
    fn calculates_usage_pace() {
        let now = Utc.with_ymd_and_hms(2026, 4, 3, 0, 0, 0).unwrap();
        let reset_at = Utc.with_ymd_and_hms(2026, 4, 8, 0, 0, 0).unwrap();
        let usage = paced_window(reset_at, 7 * 24 * 60 * 60, 50.0);
        let pace = pace(&usage, now).unwrap();

        assert!((pace.expected_percent - 28.571).abs() < 0.01);
        assert!((pace.delta_percent - 21.429).abs() < 0.01);
        assert_eq!(strip_isolation_marks(&pace_label(pace)), "21% ahead");
    }

    #[test]
    fn usage_pace_reports_room() {
        let now = Utc.with_ymd_and_hms(2026, 4, 3, 0, 0, 0).unwrap();
        let reset_at = Utc.with_ymd_and_hms(2026, 4, 8, 0, 0, 0).unwrap();
        let usage = paced_window(reset_at, 7 * 24 * 60 * 60, 10.0);

        assert_eq!(
            strip_isolation_marks(&pace_label(pace(&usage, now).unwrap())),
            "19% room"
        );
    }

    #[test]
    fn marks_zero_session_without_reset_time_as_reset() {
        let now = Utc.with_ymd_and_hms(2026, 4, 12, 16, 51, 55).unwrap();
        assert_eq!(
            reset_label(&window(None, 0.0), now, ResetTimeFormat::Relative).as_deref(),
            Some("Reset")
        );
    }

    #[test]
    fn marks_zero_session_with_future_reset_as_reset() {
        let now = Utc.with_ymd_and_hms(2026, 4, 12, 16, 51, 55).unwrap();
        let next_reset = Utc.with_ymd_and_hms(2026, 4, 12, 21, 51, 55).unwrap();
        assert_eq!(
            reset_label(
                &window(Some(next_reset), 0.0),
                now,
                ResetTimeFormat::Relative
            )
            .as_deref(),
            Some("Reset")
        );
    }

    #[test]
    fn leaves_non_session_without_reset_time_unlabeled() {
        let now = Utc.with_ymd_and_hms(2026, 4, 12, 16, 51, 55).unwrap();
        let mut weekly = window(None, 0.0);
        weekly.label = "Weekly".to_string();
        assert_eq!(reset_label(&weekly, now, ResetTimeFormat::Relative), None);
    }

    #[test]
    fn formats_future_reset_labels() {
        let now = Utc.with_ymd_and_hms(2026, 4, 12, 16, 51, 55).unwrap();
        let v = reset_label(
            &window(
                Some(Utc.with_ymd_and_hms(2026, 4, 12, 17, 10, 55).unwrap()),
                51.0,
            ),
            now,
            ResetTimeFormat::Relative,
        )
        .unwrap();
        assert_eq!(strip_isolation_marks(&v), "Resets in 19m");

        let v = reset_label(
            &window(
                Some(Utc.with_ymd_and_hms(2026, 4, 12, 19, 10, 55).unwrap()),
                51.0,
            ),
            now,
            ResetTimeFormat::Relative,
        )
        .unwrap();
        assert_eq!(strip_isolation_marks(&v), "Resets in 2h 19m");

        let v = reset_label(
            &window(
                Some(Utc.with_ymd_and_hms(2026, 4, 14, 19, 10, 55).unwrap()),
                51.0,
            ),
            now,
            ResetTimeFormat::Relative,
        )
        .unwrap();
        assert_eq!(strip_isolation_marks(&v), "Resets in 2d 2h");
    }

    #[test]
    fn formats_absolute_reset_labels() {
        let now = Utc::now();
        let tomorrow = now + chrono::Duration::days(1);
        let expected_time = tomorrow
            .with_timezone(&Local)
            .format("%-I:%M %p")
            .to_string();
        let v = reset_label(
            &window(Some(tomorrow), 51.0),
            now,
            ResetTimeFormat::Absolute,
        )
        .unwrap();

        assert_eq!(
            strip_isolation_marks(&v),
            format!("Resets tomorrow at {expected_time}")
        );
    }
}
