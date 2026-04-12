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
            return format!(
                "{} via {}",
                headline,
                self.source_label.as_deref().unwrap_or("unknown source")
            );
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
