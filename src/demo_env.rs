// SPDX-License-Identifier: MPL-2.0

use crate::config::Config;
use crate::model::{
    AccountSelectionStatus, AppState, AuthState, ProviderAccountRuntimeState, ProviderCost,
    ProviderHealth, ProviderId, ProviderIdentity, ProviderRuntimeState, UsageHeadline, UsageSnapshot,
    UsageWindow,
};
use chrono::{Duration, Utc};

const DEMO_ENV: &str = "YAPCAP_DEMO";

fn env_truthy() -> bool {
    std::env::var(DEMO_ENV).is_ok_and(|value| {
        let value = value.trim();
        !(value == "0"
            || value.eq_ignore_ascii_case("false")
            || value.eq_ignore_ascii_case("no")
            || value.eq_ignore_ascii_case("off"))
    })
}

pub fn is_active() -> bool {
    if !cfg!(debug_assertions) {
        return false;
    }
    std::env::var(DEMO_ENV).is_ok() && env_truthy()
}

pub fn apply(config: &Config, state: &mut AppState) {
    if !is_active() {
        return;
    }
    state.provider_accounts.clear();
    for provider in ProviderId::ALL {
        if !config.provider_enabled(provider) {
            state.upsert_provider(ProviderRuntimeState::disabled(provider));
            continue;
        }
        let account_id = match provider {
            ProviderId::Codex => "yapcap-demo:codex",
            ProviderId::Claude => "yapcap-demo:claude",
            ProviderId::Cursor => "yapcap-demo:cursor",
        }
        .to_string();
        let snapshot = build_snapshot(provider);
        let account = ProviderAccountRuntimeState {
            provider,
            account_id: account_id.clone(),
            label: "demo@example.com".to_string(),
            source_label: Some(demo_source(provider)),
            last_success_at: Some(Utc::now()),
            snapshot: Some(snapshot),
            health: ProviderHealth::Ok,
            auth_state: AuthState::Ready,
            error: None,
        };
        state.upsert_account(account);
        state.upsert_provider(ProviderRuntimeState {
            provider,
            enabled: true,
            active_account_id: Some(account_id),
            account_status: AccountSelectionStatus::Ready,
            is_refreshing: false,
            legacy_display_snapshot: None,
            error: None,
        });
    }
    state.updated_at = Utc::now();
    tracing::warn!(env = DEMO_ENV, "using synthetic usage snapshots (see demo_env)");
}

fn demo_source(provider: ProviderId) -> String {
    match provider {
        ProviderId::Codex | ProviderId::Claude => "OAuth".to_string(),
        ProviderId::Cursor => "Managed Account".to_string(),
    }
}

fn build_snapshot(provider: ProviderId) -> UsageSnapshot {
    match provider {
        ProviderId::Codex => snapshot_codex(),
        ProviderId::Claude => snapshot_claude(),
        ProviderId::Cursor => snapshot_cursor(),
    }
}

fn snapshot_codex() -> UsageSnapshot {
    let now = Utc::now();
    let session_end = now + Duration::seconds(2 * 3600 + 17 * 60);
    let weekly_end = now + Duration::days(2) + Duration::hours(5);
    let windows = vec![
        UsageWindow {
            label: "Session".to_string(),
            used_percent: 44.0,
            reset_at: Some(session_end),
            window_seconds: Some(5 * 60 * 60),
            reset_description: None,
        },
        UsageWindow {
            label: "Weekly".to_string(),
            used_percent: 88.0,
            reset_at: Some(weekly_end),
            window_seconds: Some(7 * 24 * 3600),
            reset_description: None,
        },
    ];
    UsageSnapshot {
        provider: ProviderId::Codex,
        source: "OAuth".to_string(),
        updated_at: now,
        headline: UsageHeadline(0),
        windows,
        provider_cost: Some(ProviderCost {
            used: 47.32,
            limit: None,
            units: "credits".to_string(),
        }),
        identity: ProviderIdentity {
            email: Some("demo@example.com".to_string()),
            account_id: Some("demo-acct-8f2a1c".to_string()),
            plan: Some("plus".to_string()),
            display_name: None,
        },
    }
}

fn snapshot_claude() -> UsageSnapshot {
    let now = Utc::now();
    let s = now + Duration::hours(3);
    let w = now + Duration::days(2);
    let o = now + Duration::days(4);
    let windows = vec![
        UsageWindow {
            label: "Session".to_string(),
            used_percent: 32.0,
            reset_at: Some(s),
            window_seconds: Some(5 * 60 * 60),
            reset_description: None,
        },
        UsageWindow {
            label: "Weekly".to_string(),
            used_percent: 66.0,
            reset_at: Some(w),
            window_seconds: Some(7 * 24 * 3600),
            reset_description: None,
        },
        UsageWindow {
            label: "Sonnet (weekly)".to_string(),
            used_percent: 41.0,
            reset_at: Some(o),
            window_seconds: Some(7 * 24 * 3600),
            reset_description: None,
        },
    ];
    UsageSnapshot {
        provider: ProviderId::Claude,
        source: "OAuth".to_string(),
        updated_at: now,
        headline: UsageHeadline(0),
        windows,
        provider_cost: None,
        identity: ProviderIdentity {
            email: Some("demo@example.com".to_string()),
            account_id: None,
            plan: Some("pro".to_string()),
            display_name: Some("Demo".to_string()),
        },
    }
}

fn snapshot_cursor() -> UsageSnapshot {
    let now = Utc::now();
    let reset_at = now + Duration::days(20);
    let start = reset_at - Duration::days(30);
    let window_seconds = (reset_at - start).num_seconds();
    let windows = vec![
        window_cursor("Total", 45.0, reset_at, window_seconds),
        window_cursor("Auto + Composer", 24.0, reset_at, window_seconds),
        window_cursor("API", 88.0, reset_at, window_seconds),
    ];
    UsageSnapshot {
        provider: ProviderId::Cursor,
        source: "Managed Account".to_string(),
        updated_at: now,
        headline: UsageHeadline(0),
        windows,
        provider_cost: None,
        identity: ProviderIdentity {
            email: Some("demo@example.com".to_string()),
            account_id: None,
            plan: Some("pro".to_string()),
            display_name: Some("Demo".to_string()),
        },
    }
}

fn window_cursor(
    label: &str,
    used_percent: f32,
    reset_at: chrono::DateTime<Utc>,
    window_seconds: i64,
) -> UsageWindow {
    UsageWindow {
        label: label.to_string(),
        used_percent,
        reset_at: Some(reset_at),
        window_seconds: Some(window_seconds),
        reset_description: Some(reset_at.to_rfc3339()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_snapshots_valid() {
        for provider in ProviderId::ALL {
            let s = build_snapshot(provider);
            assert!(!s.windows.is_empty());
            assert_eq!(s.identity.email.as_deref(), Some("demo@example.com"));
        }
    }
}
