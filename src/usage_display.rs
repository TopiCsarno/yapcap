// SPDX-License-Identifier: MPL-2.0

use crate::model::UsageWindow;
use chrono::{DateTime, Utc};

#[must_use]
pub fn displayed_percent(window: &UsageWindow, now: DateTime<Utc>) -> f64 {
    if is_elapsed(window, now) {
        0.0
    } else {
        window.used_percent.clamp(0.0, 100.0)
    }
}

#[must_use]
pub fn reset_label(window: &UsageWindow, now: DateTime<Utc>) -> Option<String> {
    if let Some(reset_at) = window.reset_at {
        return Some(format_reset_label(reset_at, now));
    }
    if is_inactive_session(window) {
        return Some("Reset".to_string());
    }
    None
}

fn is_elapsed(window: &UsageWindow, now: DateTime<Utc>) -> bool {
    window.reset_at.is_some_and(|reset_at| reset_at <= now)
}

fn is_inactive_session(window: &UsageWindow) -> bool {
    window.label == "Session" && window.used_percent <= 0.0
}

fn format_reset_label(reset_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let remaining = reset_at - now;
    if remaining.num_seconds() <= 0 {
        return "Reset".to_string();
    }
    let days = remaining.num_days();
    let hours = remaining.num_hours() % 24;
    let mins = remaining.num_minutes() % 60;
    if days > 0 {
        format!("Resets in {days}d {hours}h")
    } else if hours > 0 {
        format!("Resets in {hours}h {mins}m")
    } else {
        format!("Resets in {mins}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn window(reset_at: Option<DateTime<Utc>>, used_percent: f64) -> UsageWindow {
        UsageWindow {
            label: "Session".to_string(),
            used_percent,
            reset_at,
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
    fn marks_expired_window_as_reset() {
        let now = Utc.with_ymd_and_hms(2026, 4, 12, 16, 51, 55).unwrap();
        let reset_at = Utc.with_ymd_and_hms(2026, 4, 12, 12, 0, 0).unwrap();
        assert_eq!(
            reset_label(&window(Some(reset_at), 51.0), now).as_deref(),
            Some("Reset")
        );
    }

    #[test]
    fn marks_zero_session_without_reset_time_as_reset() {
        let now = Utc.with_ymd_and_hms(2026, 4, 12, 16, 51, 55).unwrap();
        assert_eq!(
            reset_label(&window(None, 0.0), now).as_deref(),
            Some("Reset")
        );
    }

    #[test]
    fn leaves_non_session_without_reset_time_unlabeled() {
        let now = Utc.with_ymd_and_hms(2026, 4, 12, 16, 51, 55).unwrap();
        let mut weekly = window(None, 0.0);
        weekly.label = "Weekly".to_string();
        assert_eq!(reset_label(&weekly, now), None);
    }

    #[test]
    fn formats_future_reset_labels() {
        let now = Utc.with_ymd_and_hms(2026, 4, 12, 16, 51, 55).unwrap();
        assert_eq!(
            reset_label(
                &window(
                    Some(Utc.with_ymd_and_hms(2026, 4, 12, 17, 10, 55).unwrap()),
                    51.0
                ),
                now
            )
            .as_deref(),
            Some("Resets in 19m")
        );
        assert_eq!(
            reset_label(
                &window(
                    Some(Utc.with_ymd_and_hms(2026, 4, 12, 19, 10, 55).unwrap()),
                    51.0
                ),
                now
            )
            .as_deref(),
            Some("Resets in 2h 19m")
        );
        assert_eq!(
            reset_label(
                &window(
                    Some(Utc.with_ymd_and_hms(2026, 4, 14, 19, 10, 55).unwrap()),
                    51.0
                ),
                now
            )
            .as_deref(),
            Some("Resets in 2d 2h")
        );
    }
}
