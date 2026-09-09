// SPDX-License-Identifier: MPL-2.0

use crate::usage_display;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const STALE_THRESHOLD: chrono::Duration = chrono::Duration::minutes(10);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    Codex,
    Claude,
    Cursor,
    Antigravity,
    Gemini,
    Copilot,
    Minimax,
    Kimi,
    OpenCodeGo,
    Grok,
}

impl ProviderId {
    pub const ALL: [Self; 10] = [
        Self::Codex,
        Self::Claude,
        Self::Cursor,
        Self::Antigravity,
        Self::Gemini,
        Self::Copilot,
        Self::Minimax,
        Self::Kimi,
        Self::OpenCodeGo,
        Self::Grok,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude",
            Self::Cursor => "Cursor",
            Self::Gemini => "Gemini",
            Self::Antigravity => "Antigravity",
            Self::Copilot => "Copilot",
            Self::Minimax => "Minimax",
            Self::Kimi => "Kimi",
            Self::OpenCodeGo => "OpenCode Go",
            Self::Grok => "Grok",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageWindow {
    pub label: String,
    pub used_percent: f32,
    pub reset_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub window_seconds: Option<i64>,
    pub reset_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UsageHeadline(pub usize);

impl UsageHeadline {
    #[must_use]
    pub fn first_available(_windows: &[UsageWindow]) -> Self {
        Self(0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderCost {
    pub used: f64,
    pub limit: Option<f64>,
    pub units: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExtraUsageState {
    Disabled,
    Active {
        used_percent: f32,
        cost: ProviderCost,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ProviderIdentity {
    pub email: Option<String>,
    pub account_id: Option<String>,
    pub plan: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageSnapshot {
    pub provider: ProviderId,
    pub source: String,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub headline: UsageHeadline,
    #[serde(default)]
    pub windows: Vec<UsageWindow>,
    pub provider_cost: Option<ProviderCost>,
    #[serde(default)]
    pub extra_usage: Option<ExtraUsageState>,
    pub identity: ProviderIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AppletWindows<'a> {
    pub primary: &'a UsageWindow,
    pub secondary: Option<&'a UsageWindow>,
}

impl UsageSnapshot {
    #[must_use]
    pub fn headline_window(&self) -> Option<&UsageWindow> {
        self.windows.get(self.headline.0)
    }

    #[must_use]
    pub fn applet_windows(&self) -> Option<AppletWindows<'_>> {
        if self.provider == ProviderId::Cursor {
            return self.windows.first().map(|primary| AppletWindows {
                primary,
                secondary: self.windows.get(2).or_else(|| self.windows.get(1)),
            });
        }
        if self.provider == ProviderId::Antigravity {
            let five_hour: Vec<&UsageWindow> = self
                .windows
                .iter()
                .filter(|window| window.window_seconds == Some(5 * 3600))
                .collect();
            if let [primary, secondary, ..] = five_hour.as_slice() {
                return Some(AppletWindows {
                    primary,
                    secondary: Some(secondary),
                });
            }
        }
        self.windows.first().map(|primary| AppletWindows {
            primary,
            secondary: self.windows.get(1),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProviderHealth {
    Ok,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuthState {
    Ready,
    ActionRequired,
    Error,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum AccountSelectionStatus {
    Ready,
    LoginRequired,
    SelectionRequired,
    #[default]
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderRuntimeState {
    pub provider: ProviderId,
    pub enabled: bool,
    #[serde(default)]
    pub selected_account_ids: Vec<String>,
    #[serde(default)]
    pub active_account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_active_account_id: Option<String>,
    #[serde(default)]
    pub account_status: AccountSelectionStatus,
    pub is_refreshing: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_started_at: Option<DateTime<Utc>>,
    #[serde(default, alias = "snapshot")]
    pub legacy_display_snapshot: Option<UsageSnapshot>,
    pub error: Option<String>,
}

impl ProviderRuntimeState {
    #[must_use]
    pub fn empty(provider: ProviderId) -> Self {
        Self {
            provider,
            enabled: true,
            selected_account_ids: Vec::new(),
            active_account_id: None,
            system_active_account_id: None,
            account_status: AccountSelectionStatus::Unavailable,
            is_refreshing: false,
            refresh_started_at: None,
            legacy_display_snapshot: None,
            error: Some("Not refreshed yet".to_string()),
        }
    }

    #[must_use]
    pub fn disabled(provider: ProviderId) -> Self {
        Self {
            provider,
            enabled: false,
            selected_account_ids: Vec::new(),
            active_account_id: None,
            system_active_account_id: None,
            account_status: AccountSelectionStatus::Unavailable,
            is_refreshing: false,
            refresh_started_at: None,
            legacy_display_snapshot: None,
            error: Some("Disabled in config".to_string()),
        }
    }

    #[must_use]
    pub fn status_line(&self, account: Option<&ProviderAccountRuntimeState>) -> String {
        if !self.enabled {
            return "Disabled in config".to_string();
        }
        if self.is_refreshing {
            return "Refreshing...".to_string();
        }
        if let Some(account) = account {
            return account.status_line();
        }
        if let Some(snapshot) = &self.legacy_display_snapshot {
            let now = Utc::now();
            let headline = snapshot.headline_window().map_or_else(
                || "No usage window".to_string(),
                |window| {
                    format!(
                        "{} {:.0}%",
                        window.label,
                        usage_display::displayed_percent(window, now)
                    )
                },
            );
            return format!("{headline} (stale)");
        }
        match self.account_status {
            AccountSelectionStatus::LoginRequired => "Login required".to_string(),
            AccountSelectionStatus::SelectionRequired => "Select an account".to_string(),
            AccountSelectionStatus::Unavailable => "Account unavailable".to_string(),
            AccountSelectionStatus::Ready => self
                .error
                .clone()
                .unwrap_or_else(|| "No usage data yet".to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderAccountRuntimeState {
    pub provider: ProviderId,
    pub account_id: String,
    pub label: String,
    pub source_label: Option<String>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub snapshot: Option<UsageSnapshot>,
    pub health: ProviderHealth,
    pub auth_state: AuthState,
    pub error: Option<String>,
    #[serde(default, alias = "rate_limit_until")]
    pub retry_after: Option<DateTime<Utc>>,
    #[serde(default, alias = "consecutive_rate_limits")]
    pub consecutive_failures: u32,
}

impl ProviderAccountRuntimeState {
    #[must_use]
    pub fn empty(
        provider: ProviderId,
        account_id: impl Into<String>,
        label: impl Into<String>,
    ) -> Self {
        Self {
            provider,
            account_id: account_id.into(),
            label: label.into(),
            source_label: None,
            last_success_at: None,
            snapshot: None,
            health: ProviderHealth::Ok,
            auth_state: AuthState::ActionRequired,
            error: Some("Not refreshed yet".to_string()),
            retry_after: None,
            consecutive_failures: 0,
        }
    }

    #[must_use]
    pub fn is_backing_off(&self) -> bool {
        self.retry_after.is_some_and(|until| until > Utc::now())
    }

    #[must_use]
    pub fn status_line(&self) -> String {
        if let Some(snapshot) = &self.snapshot {
            let now = Utc::now();
            let headline = snapshot.headline_window().map_or_else(
                || "No usage window".to_string(),
                |window| {
                    format!(
                        "{} {:.0}%",
                        window.label,
                        usage_display::displayed_percent(window, now)
                    )
                },
            );
            let is_stale = self.health == ProviderHealth::Error
                || self
                    .last_success_at
                    .is_none_or(|t| now - t >= STALE_THRESHOLD);
            return if is_stale {
                format!("{headline} (stale)")
            } else {
                headline
            };
        }
        self.error
            .clone()
            .unwrap_or_else(|| "No usage data yet".to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppState {
    pub providers: Vec<ProviderRuntimeState>,
    #[serde(default)]
    pub provider_accounts: Vec<ProviderAccountRuntimeState>,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kimi_provider_id_has_label_and_is_enumerated() {
        assert_eq!(ProviderId::Kimi.label(), "Kimi");
        assert!(ProviderId::ALL.contains(&ProviderId::Kimi));
    }

    #[test]
    fn grok_provider_id_has_label_and_is_enumerated() {
        assert_eq!(ProviderId::Grok.label(), "Grok");
        assert!(ProviderId::ALL.contains(&ProviderId::Grok));
    }

    fn window(label: &str) -> UsageWindow {
        UsageWindow {
            label: label.to_string(),
            used_percent: 10.0,
            reset_at: None,
            window_seconds: None,
            reset_description: None,
            group: None,
        }
    }

    fn snapshot(provider: ProviderId) -> UsageSnapshot {
        UsageSnapshot {
            provider,
            source: "test".to_string(),
            updated_at: Utc::now(),
            headline: UsageHeadline(0),
            windows: vec![window("first"), window("second"), window("third")],
            provider_cost: None,
            extra_usage: None,
            identity: ProviderIdentity::default(),
        }
    }

    #[test]
    fn applet_windows_returns_first_two() {
        let snapshot = snapshot(ProviderId::Codex);
        let windows = snapshot.applet_windows().unwrap();
        assert_eq!(windows.primary.label, "first");
        assert_eq!(windows.secondary.map(|w| w.label.as_str()), Some("second"));
    }

    #[test]
    fn applet_windows_preserves_single_window_shape() {
        let mut snapshot = snapshot(ProviderId::Copilot);
        snapshot.windows = vec![window("premium_interactions")];
        let windows = snapshot.applet_windows().unwrap();
        assert_eq!(windows.primary.label, "premium_interactions");
        assert_eq!(windows.secondary.map(|w| w.label.as_str()), None);
    }

    fn five_hour_window(label: &str) -> UsageWindow {
        UsageWindow {
            label: label.to_string(),
            used_percent: 10.0,
            reset_at: None,
            window_seconds: Some(5 * 3600),
            reset_description: None,
            group: None,
        }
    }

    fn weekly_window(label: &str) -> UsageWindow {
        UsageWindow {
            label: label.to_string(),
            used_percent: 10.0,
            reset_at: None,
            window_seconds: Some(7 * 24 * 3600),
            reset_description: None,
            group: None,
        }
    }

    #[test]
    fn antigravity_applet_selects_two_five_hour_windows_in_order() {
        let mut snap = snapshot(ProviderId::Antigravity);
        snap.windows = vec![
            weekly_window("Gemini Weekly"),
            five_hour_window("Gemini 5h"),
            weekly_window("3p Weekly"),
            five_hour_window("3p 5h"),
        ];
        let windows = snap.applet_windows().unwrap();
        assert_eq!(windows.primary.label, "Gemini 5h");
        assert_eq!(windows.secondary.map(|w| w.label.as_str()), Some("3p 5h"));
    }

    #[test]
    fn antigravity_applet_falls_back_to_first_two_when_fewer_than_two_five_hour() {
        let mut snap = snapshot(ProviderId::Antigravity);
        snap.windows = vec![weekly_window("Weekly"), five_hour_window("5h")];
        let windows = snap.applet_windows().unwrap();
        assert_eq!(windows.primary.label, "Weekly");
        assert_eq!(windows.secondary.map(|w| w.label.as_str()), Some("5h"));
    }

    #[test]
    fn antigravity_applet_falls_back_when_no_five_hour_windows() {
        let mut snap = snapshot(ProviderId::Antigravity);
        snap.windows = vec![weekly_window("A"), weekly_window("B")];
        let windows = snap.applet_windows().unwrap();
        assert_eq!(windows.primary.label, "A");
        assert_eq!(windows.secondary.map(|w| w.label.as_str()), Some("B"));
    }

    #[test]
    fn usage_window_group_round_trips_and_defaults_to_none() {
        let json = r#"{"label":"x","used_percent":1.0,"reset_at":null,"reset_description":null}"#;
        let parsed: UsageWindow = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.group, None);
        let mut grouped = parsed.clone();
        grouped.group = Some("Gemini Models".to_string());
        let round: UsageWindow =
            serde_json::from_str(&serde_json::to_string(&grouped).unwrap()).unwrap();
        assert_eq!(round.group.as_deref(), Some("Gemini Models"));
    }

    #[test]
    fn cursor_applet_windows_use_total_and_api() {
        let mut snap = snapshot(ProviderId::Cursor);
        snap.windows = vec![window("Total"), window("Auto + Composer"), window("API")];
        let windows = snap.applet_windows().unwrap();
        assert_eq!(windows.primary.label, "Total");
        assert_eq!(windows.secondary.map(|w| w.label.as_str()), Some("API"));
    }

    #[test]
    fn headline_selects_by_index() {
        let snapshot = snapshot(ProviderId::Codex);
        assert_eq!(
            UsageHeadline::first_available(&snapshot.windows),
            UsageHeadline(0)
        );
    }

    #[test]
    fn status_line_uses_headline_when_present() {
        let state = ProviderRuntimeState::empty(ProviderId::Codex);
        let mut account = ProviderAccountRuntimeState::empty(ProviderId::Codex, "codex-1", "Codex");
        let mut snapshot = snapshot(ProviderId::Codex);
        snapshot.windows = vec![
            UsageWindow {
                label: "Session".to_string(),
                used_percent: 31.0,
                reset_at: None,
                window_seconds: None,
                reset_description: None,
                group: None,
            },
            UsageWindow {
                label: "Weekly".to_string(),
                used_percent: 88.0,
                reset_at: None,
                window_seconds: None,
                reset_description: None,
                group: None,
            },
        ];
        snapshot.headline = UsageHeadline::first_available(&snapshot.windows);
        account.snapshot = Some(snapshot);
        account.source_label = Some("OAuth".to_string());
        account.health = ProviderHealth::Ok;
        account.last_success_at = Some(Utc::now());

        assert_eq!(state.status_line(Some(&account)), "Session 31%");
    }

    #[test]
    fn status_line_marks_stale_when_refresh_failed() {
        let state = ProviderRuntimeState::empty(ProviderId::Codex);
        let mut account = ProviderAccountRuntimeState::empty(ProviderId::Codex, "codex-1", "Codex");
        let mut snap = snapshot(ProviderId::Codex);
        snap.windows = vec![UsageWindow {
            label: "Session".to_string(),
            used_percent: 31.0,
            reset_at: None,
            window_seconds: None,
            reset_description: None,
            group: None,
        }];
        snap.headline = UsageHeadline(0);
        account.snapshot = Some(snap);
        account.source_label = Some("OAuth".to_string());
        account.health = ProviderHealth::Error;
        account.last_success_at = Some(Utc::now());

        assert_eq!(state.status_line(Some(&account)), "Session 31% (stale)");
    }
}
