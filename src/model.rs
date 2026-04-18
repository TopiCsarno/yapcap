use crate::usage_display;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    Codex,
    Claude,
    Cursor,
}

impl ProviderId {
    pub const ALL: [Self; 3] = [Self::Codex, Self::Claude, Self::Cursor];

    pub fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude",
            Self::Cursor => "Cursor",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageWindow {
    pub label: String,
    pub used_percent: f64,
    pub reset_at: Option<DateTime<Utc>>,
    pub reset_description: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UsageHeadline {
    #[default]
    Primary,
    Secondary,
    Tertiary,
}

impl UsageHeadline {
    pub fn primary_first(
        primary: Option<&UsageWindow>,
        secondary: Option<&UsageWindow>,
        tertiary: Option<&UsageWindow>,
    ) -> Self {
        if primary.is_some() {
            Self::Primary
        } else if secondary.is_some() {
            Self::Secondary
        } else if tertiary.is_some() {
            Self::Tertiary
        } else {
            Self::Primary
        }
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
    pub primary: Option<UsageWindow>,
    pub secondary: Option<UsageWindow>,
    pub tertiary: Option<UsageWindow>,
    pub provider_cost: Option<ProviderCost>,
    pub identity: ProviderIdentity,
}

impl UsageSnapshot {
    pub fn headline_window(&self) -> Option<&UsageWindow> {
        match self.headline {
            UsageHeadline::Primary => self.primary.as_ref(),
            UsageHeadline::Secondary => self.secondary.as_ref(),
            UsageHeadline::Tertiary => self.tertiary.as_ref(),
        }
    }

    pub fn applet_windows(&self) -> (Option<&UsageWindow>, Option<&UsageWindow>) {
        match self.provider {
            ProviderId::Cursor => (self.primary.as_ref(), self.tertiary.as_ref()),
            ProviderId::Codex | ProviderId::Claude => {
                (self.primary.as_ref(), self.secondary.as_ref())
            }
        }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRuntimeState {
    pub provider: ProviderId,
    pub enabled: bool,
    pub is_refreshing: bool,
    pub health: ProviderHealth,
    pub auth_state: AuthState,
    pub source_label: Option<String>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub snapshot: Option<UsageSnapshot>,
    pub error: Option<String>,
}

impl ProviderRuntimeState {
    pub fn empty(provider: ProviderId) -> Self {
        Self {
            provider,
            enabled: true,
            is_refreshing: false,
            health: ProviderHealth::Ok,
            auth_state: AuthState::ActionRequired,
            source_label: None,
            last_success_at: None,
            snapshot: None,
            error: Some("Not refreshed yet".to_string()),
        }
    }

    pub fn disabled(provider: ProviderId) -> Self {
        Self {
            provider,
            enabled: false,
            is_refreshing: false,
            health: ProviderHealth::Ok,
            auth_state: AuthState::Ready,
            source_label: None,
            last_success_at: None,
            snapshot: None,
            error: Some("Disabled in config".to_string()),
        }
    }

    pub fn status_line(&self) -> String {
        if !self.enabled {
            return "Disabled in config".to_string();
        }
        if self.is_refreshing {
            return "Refreshing...".to_string();
        }
        if let Some(snapshot) = &self.snapshot {
            let now = Utc::now();
            let headline = snapshot
                .headline_window()
                .as_ref()
                .map(|window| {
                    format!(
                        "{} {:.0}%",
                        window.label,
                        usage_display::displayed_percent(window, now)
                    )
                })
                .unwrap_or_else(|| "No usage window".to_string());
            let is_stale = self.health == ProviderHealth::Error
                || self
                    .last_success_at
                    .is_none_or(|t| now - t >= chrono::Duration::minutes(10));
            let source = self.source_label.as_deref().unwrap_or("unknown source");
            return if is_stale {
                format!("{headline} via {source} (stale)")
            } else {
                format!("{headline} via {source}")
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
    pub updated_at: DateTime<Utc>,
}

impl AppState {
    pub fn empty() -> Self {
        Self {
            providers: ProviderId::ALL
                .into_iter()
                .map(ProviderRuntimeState::empty)
                .collect(),
            updated_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(label: &str) -> UsageWindow {
        UsageWindow {
            label: label.to_string(),
            used_percent: 10.0,
            reset_at: None,
            reset_description: None,
        }
    }

    fn snapshot(provider: ProviderId) -> UsageSnapshot {
        UsageSnapshot {
            provider,
            source: "test".to_string(),
            updated_at: Utc::now(),
            headline: UsageHeadline::Primary,
            primary: Some(window("primary")),
            secondary: Some(window("secondary")),
            tertiary: Some(window("tertiary")),
            provider_cost: None,
            identity: ProviderIdentity::default(),
        }
    }

    #[test]
    fn applet_windows_codex_uses_secondary() {
        let snapshot = snapshot(ProviderId::Codex);
        let (_, secondary) = snapshot.applet_windows();
        assert_eq!(secondary.map(|w| w.label.as_str()), Some("secondary"));
    }

    #[test]
    fn applet_windows_cursor_uses_tertiary() {
        let snapshot = snapshot(ProviderId::Cursor);
        let (_, secondary) = snapshot.applet_windows();
        assert_eq!(secondary.map(|w| w.label.as_str()), Some("tertiary"));
    }

    #[test]
    fn headline_prefers_primary_usage_window() {
        let snapshot = snapshot(ProviderId::Codex);
        assert_eq!(
            UsageHeadline::primary_first(
                snapshot.primary.as_ref(),
                snapshot.secondary.as_ref(),
                snapshot.tertiary.as_ref(),
            ),
            UsageHeadline::Primary
        );
    }

    #[test]
    fn status_line_uses_five_hour_headline_when_present() {
        let mut state = ProviderRuntimeState::empty(ProviderId::Codex);
        let mut snapshot = snapshot(ProviderId::Codex);
        snapshot.primary = Some(UsageWindow {
            label: "5h".to_string(),
            used_percent: 31.0,
            reset_at: None,
            reset_description: None,
        });
        snapshot.secondary = Some(UsageWindow {
            label: "7d".to_string(),
            used_percent: 88.0,
            reset_at: None,
            reset_description: None,
        });
        snapshot.headline = UsageHeadline::primary_first(
            snapshot.primary.as_ref(),
            snapshot.secondary.as_ref(),
            snapshot.tertiary.as_ref(),
        );
        state.snapshot = Some(snapshot);
        state.source_label = Some("OAuth".to_string());
        state.health = ProviderHealth::Ok;
        state.last_success_at = Some(Utc::now());

        assert_eq!(state.status_line(), "5h 31% via OAuth");
    }

    #[test]
    fn status_line_marks_stale_when_refresh_failed() {
        let mut state = ProviderRuntimeState::empty(ProviderId::Codex);
        let mut snap = snapshot(ProviderId::Codex);
        snap.primary = Some(UsageWindow {
            label: "5h".to_string(),
            used_percent: 31.0,
            reset_at: None,
            reset_description: None,
        });
        snap.headline = UsageHeadline::Primary;
        state.snapshot = Some(snap);
        state.source_label = Some("OAuth".to_string());
        state.health = ProviderHealth::Error;
        state.last_success_at = Some(Utc::now());

        assert_eq!(state.status_line(), "5h 31% via OAuth (stale)");
    }
}
