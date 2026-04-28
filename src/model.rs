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
}

impl ProviderId {
    pub const ALL: [Self; 3] = [Self::Codex, Self::Claude, Self::Cursor];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude",
            Self::Cursor => "Cursor",
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
    pub identity: ProviderIdentity,
}

impl UsageSnapshot {
    #[must_use]
    pub fn headline_window(&self) -> Option<&UsageWindow> {
        self.windows.get(self.headline.0)
    }

    #[must_use]
    pub fn applet_windows(&self) -> (Option<&UsageWindow>, Option<&UsageWindow>) {
        if self.provider == ProviderId::Cursor {
            return (
                self.windows.first(),
                self.windows.get(2).or_else(|| self.windows.get(1)),
            );
        }
        (self.windows.first(), self.windows.get(1))
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRuntimeState {
    pub provider: ProviderId,
    pub enabled: bool,
    #[serde(default)]
    pub selected_account_ids: Vec<String>,
    #[serde(default)]
    pub account_status: AccountSelectionStatus,
    pub is_refreshing: bool,
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
            account_status: AccountSelectionStatus::Unavailable,
            is_refreshing: false,
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
            account_status: AccountSelectionStatus::Unavailable,
            is_refreshing: false,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub providers: Vec<ProviderRuntimeState>,
    #[serde(default)]
    pub provider_accounts: Vec<ProviderAccountRuntimeState>,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(label: &str) -> UsageWindow {
        UsageWindow {
            label: label.to_string(),
            used_percent: 10.0,
            reset_at: None,
            window_seconds: None,
            reset_description: None,
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
            identity: ProviderIdentity::default(),
        }
    }

    #[test]
    fn applet_windows_returns_first_two() {
        let snapshot = snapshot(ProviderId::Codex);
        let (first, second) = snapshot.applet_windows();
        assert_eq!(first.map(|w| w.label.as_str()), Some("first"));
        assert_eq!(second.map(|w| w.label.as_str()), Some("second"));
    }

    #[test]
    fn cursor_applet_windows_use_total_and_api() {
        let mut snap = snapshot(ProviderId::Cursor);
        snap.windows = vec![window("Total"), window("Auto + Composer"), window("API")];
        let (first, second) = snap.applet_windows();
        assert_eq!(first.map(|w| w.label.as_str()), Some("Total"));
        assert_eq!(second.map(|w| w.label.as_str()), Some("API"));
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
            },
            UsageWindow {
                label: "Weekly".to_string(),
                used_percent: 88.0,
                reset_at: None,
                window_seconds: None,
                reset_description: None,
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
        }];
        snap.headline = UsageHeadline(0);
        account.snapshot = Some(snap);
        account.source_label = Some("OAuth".to_string());
        account.health = ProviderHealth::Error;
        account.last_success_at = Some(Utc::now());

        assert_eq!(state.status_line(Some(&account)), "Session 31% (stale)");
    }
}
