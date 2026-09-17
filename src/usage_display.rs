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

#[derive(Debug, Clone, PartialEq)]
pub struct UsageMeter {
    pub fill_percent: f32,
    pub marker_percent: Option<f32>,
    pub tooltip: String,
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
pub fn usage_meter(
    window: &UsageWindow,
    now: DateTime<Utc>,
    format: UsageAmountFormat,
) -> UsageMeter {
    let pace = pace(window, now);
    UsageMeter {
        fill_percent: displayed_amount_percent(window, now, format),
        marker_percent: pace.map(|pace| match format {
            UsageAmountFormat::Used => pace.expected_percent,
            UsageAmountFormat::Left => 100.0 - pace.expected_percent,
        }),
        tooltip: pace.map_or_else(|| usage_amount_label(window, now, format), pace_label),
    }
}

#[must_use]
pub fn refill_meter(
    window: &UsageWindow,
    now: DateTime<Utc>,
    format: UsageAmountFormat,
    tick: Option<crate::model::RegenerationTick>,
) -> UsageMeter {
    UsageMeter {
        fill_percent: displayed_amount_percent(window, now, format),
        marker_percent: None,
        tooltip: full_regeneration_label(window, now, tick)
            .unwrap_or_else(|| usage_amount_label(window, now, format)),
    }
}

#[must_use]
pub fn full_regeneration_label(
    window: &UsageWindow,
    now: DateTime<Utc>,
    tick: Option<crate::model::RegenerationTick>,
) -> Option<String> {
    let used = displayed_percent(window, now);
    if used <= 0.0 {
        return Some(fl!("quota-full"));
    }
    if let (Some(tick), Some(next_tick_at)) = (tick, window.reset_at)
        && tick.interval_seconds > 0
        && (0.0..=1.0).contains(&tick.fraction)
    {
        let ticks_needed = (used / 100.0 / tick.fraction).ceil().max(1.0) as i64;
        let until_next = (next_tick_at - now).num_seconds().max(0);
        let remaining_full = chrono::Duration::seconds(
            until_next + (ticks_needed - 1).saturating_mul(tick.interval_seconds),
        );
        return Some(format_full_in(remaining_full));
    }
    let window_seconds = window.window_seconds.filter(|seconds| *seconds > 0)?;
    let remaining_full = chrono::Duration::seconds(
        (f64::from(used) / 100.0 * (window_seconds as f64)).round() as i64,
    );
    Some(format_full_in(remaining_full))
}

fn format_full_in(remaining_full: chrono::Duration) -> String {
    let days = remaining_full.num_days();
    let hours = remaining_full.num_hours() % 24;
    let mins = remaining_full.num_minutes() % 60;
    if days > 0 {
        fl!("full-in-days-hours", days = days, hours = hours)
    } else if hours > 0 {
        fl!("full-in-hours-minutes", hours = hours, mins = mins)
    } else {
        fl!("full-in-minutes", mins = mins.max(1))
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
    renewal_label(window, now, format, RenewalVerb::Reset)
}

#[must_use]
pub fn refill_label(
    window: &UsageWindow,
    now: DateTime<Utc>,
    format: ResetTimeFormat,
) -> Option<String> {
    renewal_label(window, now, format, RenewalVerb::Refill)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenewalVerb {
    Reset,
    Refill,
}

fn renewal_label(
    window: &UsageWindow,
    now: DateTime<Utc>,
    format: ResetTimeFormat,
    verb: RenewalVerb,
) -> Option<String> {
    if is_elapsed(window, now) || is_inactive_window(window, now) {
        return Some(match verb {
            RenewalVerb::Reset => fl!("reset-now"),
            RenewalVerb::Refill => fl!("refilled"),
        });
    }
    if let Some(reset_at) = window.reset_at {
        return Some(match format {
            ResetTimeFormat::Relative => format_relative_reset_label(reset_at, now, verb),
            ResetTimeFormat::Absolute => format_absolute_reset_label(reset_at, now, verb),
        });
    }
    None
}

fn is_elapsed(window: &UsageWindow, now: DateTime<Utc>) -> bool {
    window.reset_at.is_some_and(|reset_at| reset_at <= now)
}

const FRESH_WINDOW_FRACTION: i64 = 20;

fn is_inactive_window(window: &UsageWindow, now: DateTime<Utc>) -> bool {
    if window.used_percent > 0.0 {
        return false;
    }
    let Some(window_seconds) = window.window_seconds.filter(|s| *s > 0) else {
        return true;
    };
    let Some(reset_at) = window.reset_at else {
        return true;
    };
    let window_start = reset_at - chrono::Duration::seconds(window_seconds);
    let elapsed = now - window_start;
    elapsed >= chrono::Duration::zero()
        && elapsed < chrono::Duration::seconds(window_seconds / FRESH_WINDOW_FRACTION)
}

fn format_relative_reset_label(
    reset_at: DateTime<Utc>,
    now: DateTime<Utc>,
    verb: RenewalVerb,
) -> String {
    let remaining = reset_at - now;
    if remaining.num_seconds() <= 0 {
        return match verb {
            RenewalVerb::Reset => fl!("reset-now"),
            RenewalVerb::Refill => fl!("refilled"),
        };
    }
    let days = remaining.num_days();
    let hours = remaining.num_hours() % 24;
    let mins = remaining.num_minutes() % 60;
    match verb {
        RenewalVerb::Reset => {
            if days > 0 {
                fl!("resets-in-days-hours", days = days, hours = hours)
            } else if hours > 0 {
                fl!("resets-in-hours-minutes", hours = hours, mins = mins)
            } else {
                fl!("resets-in-minutes", mins = mins)
            }
        }
        RenewalVerb::Refill => {
            if days > 0 {
                fl!("refills-in-days-hours", days = days, hours = hours)
            } else if hours > 0 {
                fl!("refills-in-hours-minutes", hours = hours, mins = mins)
            } else {
                fl!("refills-in-minutes", mins = mins)
            }
        }
    }
}

fn format_absolute_reset_label(
    reset_at: DateTime<Utc>,
    now: DateTime<Utc>,
    verb: RenewalVerb,
) -> String {
    let reset_at = reset_at.with_timezone(&Local);
    let now = now.with_timezone(&Local);
    let days = reset_at
        .date_naive()
        .signed_duration_since(now.date_naive())
        .num_days();
    let time = reset_at.format("%-I:%M %p").to_string();

    match verb {
        RenewalVerb::Reset => match days {
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
        },
        RenewalVerb::Refill => match days {
            0 => fl!("refills-today-at", time = time),
            1 => fl!("refills-tomorrow-at", time = time),
            2..=6 => fl!(
                "refills-weekday-at",
                weekday = reset_at.format("%A").to_string(),
                time = time
            ),
            _ => fl!(
                "refills-date-at",
                date = reset_at.format("%b %-e").to_string(),
                time = time
            ),
        },
    }
}

#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn portion_percent(used: f64, scale: f64) -> f32 {
    if scale <= f64::EPSILON {
        return 0.0;
    }
    ((used / scale) * 100.0_f64).clamp(0.0, 100.0) as f32
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
            group: None,
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
            group: None,
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
    fn full_regeneration_counts_discrete_ticks_from_the_next_one() {
        let now = Utc.with_ymd_and_hms(2026, 4, 12, 16, 51, 55).unwrap();
        let next_tick = Utc.with_ymd_and_hms(2026, 4, 12, 16, 52, 55).unwrap();
        let five_hour_tick = crate::model::RegenerationTick {
            interval_seconds: 900,
            fraction: 0.05,
        };

        let one_tick_left = UsageWindow {
            label: "5-hour".to_string(),
            used_percent: 5.0,
            reset_at: Some(next_tick),
            window_seconds: Some(18_000),
            reset_description: None,
            group: None,
        };
        assert_eq!(
            strip_isolation_marks(
                &full_regeneration_label(&one_tick_left, now, Some(five_hour_tick)).unwrap()
            ),
            "Full in 1m"
        );

        let four_ticks_left = UsageWindow {
            used_percent: 20.0,
            ..one_tick_left.clone()
        };
        assert_eq!(
            strip_isolation_marks(
                &full_regeneration_label(&four_ticks_left, now, Some(five_hour_tick)).unwrap()
            ),
            "Full in 46m"
        );

        let fractional_usage = UsageWindow {
            used_percent: 20.1,
            ..one_tick_left
        };
        assert_eq!(
            strip_isolation_marks(
                &full_regeneration_label(&fractional_usage, now, Some(five_hour_tick)).unwrap()
            ),
            "Full in 1h 1m"
        );

        let week_tick = crate::model::RegenerationTick {
            interval_seconds: 12_120,
            fraction: 0.02,
        };
        let one_week_tick_left = UsageWindow {
            label: "Weekly credits".to_string(),
            used_percent: 2.0,
            reset_at: Some(next_tick),
            window_seconds: Some(604_800),
            reset_description: None,
            group: None,
        };
        assert_eq!(
            strip_isolation_marks(
                &full_regeneration_label(&one_week_tick_left, now, Some(week_tick)).unwrap()
            ),
            "Full in 1m"
        );
    }

    #[test]
    fn full_regeneration_scales_with_used_quota() {
        let now = Utc.with_ymd_and_hms(2026, 4, 12, 16, 51, 55).unwrap();
        let next_tick = Utc.with_ymd_and_hms(2026, 4, 12, 17, 30, 0).unwrap();
        let five_hour = UsageWindow {
            label: "5-hour".to_string(),
            used_percent: 20.0,
            reset_at: Some(next_tick),
            window_seconds: Some(18_000),
            reset_description: None,
            group: None,
        };

        assert_eq!(
            strip_isolation_marks(&full_regeneration_label(&five_hour, now, None).unwrap()),
            "Full in 1h 0m"
        );

        let weekly = UsageWindow {
            label: "Weekly credits".to_string(),
            used_percent: 50.0,
            reset_at: Some(next_tick),
            window_seconds: Some(604_800),
            reset_description: None,
            group: None,
        };

        assert_eq!(
            strip_isolation_marks(&full_regeneration_label(&weekly, now, None).unwrap()),
            "Full in 3d 12h"
        );

        let full = UsageWindow {
            used_percent: 0.0,
            ..five_hour
        };
        assert_eq!(
            strip_isolation_marks(&full_regeneration_label(&full, now, None).unwrap()),
            "Full"
        );
    }

    #[test]
    fn refill_meter_omits_pace_marker() {
        let now = Utc.with_ymd_and_hms(2026, 4, 12, 16, 51, 55).unwrap();
        let next_tick = Utc.with_ymd_and_hms(2026, 4, 12, 17, 30, 0).unwrap();
        let regen = UsageWindow {
            label: "5-hour".to_string(),
            used_percent: 20.0,
            reset_at: Some(next_tick),
            window_seconds: Some(18_000),
            reset_description: None,
            group: None,
        };

        let meter = refill_meter(&regen, now, UsageAmountFormat::Used, None);

        assert!((meter.fill_percent - 20.0).abs() < 0.001);
        assert!(meter.marker_percent.is_none());
        assert_eq!(strip_isolation_marks(&meter.tooltip), "Full in 1h 0m");
    }

    #[test]
    fn refill_labels_use_partial_refill_wording() {
        let now = Utc.with_ymd_and_hms(2026, 4, 12, 16, 51, 55).unwrap();
        let next_tick = Utc.with_ymd_and_hms(2026, 4, 12, 17, 30, 0).unwrap();
        let regen = UsageWindow {
            label: "5-hour".to_string(),
            used_percent: 20.0,
            reset_at: Some(next_tick),
            window_seconds: Some(18_000),
            reset_description: None,
            group: None,
        };

        assert_eq!(
            strip_isolation_marks(&refill_label(&regen, now, ResetTimeFormat::Relative).unwrap()),
            "Refills in 38m"
        );

        let elapsed = Utc.with_ymd_and_hms(2026, 4, 12, 12, 0, 0).unwrap();
        assert_eq!(
            refill_label(&window(Some(elapsed), 51.0), now, ResetTimeFormat::Relative).as_deref(),
            Some("Refilled")
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
    fn usage_meter_standardizes_marker_and_tooltip() {
        let now = Utc.with_ymd_and_hms(2026, 4, 3, 0, 0, 0).unwrap();
        let reset_at = Utc.with_ymd_and_hms(2026, 4, 8, 0, 0, 0).unwrap();
        let usage = paced_window(reset_at, 7 * 24 * 60 * 60, 50.0);

        let meter = usage_meter(&usage, now, UsageAmountFormat::Used);

        assert_eq!(meter.fill_percent, 50.0);
        assert!((meter.marker_percent.unwrap() - 28.571).abs() < 0.01);
        assert_eq!(strip_isolation_marks(&meter.tooltip), "21% ahead");
    }

    #[test]
    fn usage_meter_always_has_hover_text() {
        let now = Utc.with_ymd_and_hms(2026, 4, 3, 0, 0, 0).unwrap();
        let usage = window(None, 25.0);

        let meter = usage_meter(&usage, now, UsageAmountFormat::Used);

        assert_eq!(meter.marker_percent, None);
        assert_eq!(strip_isolation_marks(&meter.tooltip), "25.0% used");
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
    fn marks_zero_usage_window_inside_fresh_fraction_as_reset() {
        let window_seconds = 24 * 60 * 60;
        let window_start = Utc.with_ymd_and_hms(2026, 4, 12, 0, 0, 0).unwrap();
        let next_reset = window_start + chrono::Duration::seconds(window_seconds);
        let now = window_start + chrono::Duration::minutes(30);
        let usage = paced_window(next_reset, window_seconds, 0.0);
        assert_eq!(
            reset_label(&usage, now, ResetTimeFormat::Relative).as_deref(),
            Some("Reset")
        );
    }

    #[test]
    fn does_not_mark_zero_usage_window_past_fresh_fraction_as_reset() {
        let window_seconds = 24 * 60 * 60;
        let window_start = Utc.with_ymd_and_hms(2026, 4, 12, 0, 0, 0).unwrap();
        let next_reset = window_start + chrono::Duration::seconds(window_seconds);
        let now = window_start + chrono::Duration::hours(6);
        let usage = paced_window(next_reset, window_seconds, 0.0);
        let label = reset_label(&usage, now, ResetTimeFormat::Relative).unwrap();
        assert_ne!(strip_isolation_marks(&label), "Reset");
    }

    #[test]
    fn does_not_mark_nonzero_usage_window_as_reset() {
        let window_seconds = 24 * 60 * 60;
        let window_start = Utc.with_ymd_and_hms(2026, 4, 12, 0, 0, 0).unwrap();
        let next_reset = window_start + chrono::Duration::seconds(window_seconds);
        let now = window_start + chrono::Duration::minutes(10);
        let usage = paced_window(next_reset, window_seconds, 12.5);
        let label = reset_label(&usage, now, ResetTimeFormat::Relative).unwrap();
        assert_ne!(strip_isolation_marks(&label), "Reset");
    }

    #[test]
    fn marks_zero_weekly_without_reset_time_as_reset() {
        let now = Utc.with_ymd_and_hms(2026, 4, 12, 16, 51, 55).unwrap();
        let mut weekly = window(None, 0.0);
        weekly.label = "Weekly".to_string();
        assert_eq!(
            reset_label(&weekly, now, ResetTimeFormat::Relative).as_deref(),
            Some("Reset")
        );
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
