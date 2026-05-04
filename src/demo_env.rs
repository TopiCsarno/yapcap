// SPDX-License-Identifier: MPL-2.0

use crate::config::{
    Config, ManagedClaudeAccountConfig, ManagedCodexAccountConfig, ManagedCursorAccountConfig,
    ProviderVisibilityMode, paths,
};
use crate::model::{
    AccountSelectionStatus, AppState, AuthState, ExtraUsageState, ProviderAccountRuntimeState,
    ProviderCost, ProviderHealth, ProviderId, ProviderIdentity, ProviderRuntimeState,
    UsageHeadline, UsageSnapshot, UsageWindow,
};
use chrono::{DateTime, Duration, Utc};
use std::path::PathBuf;

const DEMO_ENV: &str = "YAPCAP_DEMO";
const CODEX_PRIMARY_ID: &str = "yapcap-demo:codex-primary";
const CODEX_SECONDARY_ID: &str = "yapcap-demo:codex-secondary";
const CODEX_TERTIARY_ID: &str = "yapcap-demo:codex-tertiary";
const CLAUDE_PRIMARY_ID: &str = "yapcap-demo:claude-primary";
const CLAUDE_SECONDARY_ID: &str = "yapcap-demo:claude-secondary";
const CURSOR_PRIMARY_ID: &str = "yapcap-demo:cursor-primary";
const CURSOR_SECONDARY_ID: &str = "yapcap-demo:cursor-secondary";

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

pub fn apply_config(config: &mut Config) {
    if !is_active() {
        return;
    }

    config.codex_enabled = true;
    config.claude_enabled = true;
    config.cursor_enabled = true;

    config.codex_managed_accounts = demo_codex_accounts();
    config.claude_managed_accounts = demo_claude_accounts();
    config.cursor_managed_accounts = demo_cursor_accounts();

    config.provider_visibility_mode = ProviderVisibilityMode::UserManaged;

    config.selected_codex_account_ids = vec![
        CODEX_PRIMARY_ID.to_string(),
        CODEX_SECONDARY_ID.to_string(),
        CODEX_TERTIARY_ID.to_string(),
    ];
    config.selected_claude_account_ids = vec![
        CLAUDE_PRIMARY_ID.to_string(),
        CLAUDE_SECONDARY_ID.to_string(),
    ];
    config.selected_cursor_account_ids = vec![
        CURSOR_PRIMARY_ID.to_string(),
        CURSOR_SECONDARY_ID.to_string(),
    ];

    config.set_provider_show_all(ProviderId::Codex, true);
    config.set_provider_show_all(ProviderId::Claude, true);
    config.set_provider_show_all(ProviderId::Cursor, true);
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
        for account in demo_runtime_accounts(provider) {
            state.upsert_account(account);
        }
        state.upsert_provider(ProviderRuntimeState {
            provider,
            enabled: true,
            selected_account_ids: config.selected_account_ids(provider).to_vec(),
            active_account_id: config.selected_account_ids(provider).first().cloned(),
            system_active_account_id: None,
            account_status: AccountSelectionStatus::Ready,
            is_refreshing: false,
            legacy_display_snapshot: None,
            error: None,
        });
    }
    state.updated_at = Utc::now();
    tracing::warn!(
        env = DEMO_ENV,
        "using synthetic usage snapshots (see demo_env)"
    );
}

fn demo_source(provider: ProviderId) -> String {
    match provider {
        ProviderId::Codex | ProviderId::Claude => "OAuth".to_string(),
        ProviderId::Cursor => "Managed Account".to_string(),
    }
}

fn demo_runtime_accounts(provider: ProviderId) -> Vec<ProviderAccountRuntimeState> {
    let now = Utc::now();
    match provider {
        ProviderId::Codex => vec![
            demo_account(
                provider,
                DemoAccount {
                    account_id: CODEX_PRIMARY_ID,
                    label: "ada@example.com",
                    last_success_at: now - Duration::minutes(2),
                    health: ProviderHealth::Ok,
                    auth_state: AuthState::Ready,
                    error: None,
                    snapshot: snapshot_codex_primary(),
                },
            ),
            demo_account(
                provider,
                DemoAccount {
                    account_id: CODEX_SECONDARY_ID,
                    label: "pair@example.com",
                    last_success_at: now - Duration::minutes(6),
                    health: ProviderHealth::Ok,
                    auth_state: AuthState::Ready,
                    error: None,
                    snapshot: snapshot_codex_secondary(),
                },
            ),
            demo_account(
                provider,
                DemoAccount {
                    account_id: CODEX_TERTIARY_ID,
                    label: "tri@example.com",
                    last_success_at: now - Duration::minutes(9),
                    health: ProviderHealth::Ok,
                    auth_state: AuthState::Ready,
                    error: None,
                    snapshot: snapshot_codex_tertiary(),
                },
            ),
        ],
        ProviderId::Claude => vec![
            demo_account(
                provider,
                DemoAccount {
                    account_id: CLAUDE_PRIMARY_ID,
                    label: "team@example.com",
                    last_success_at: now - Duration::minutes(3),
                    health: ProviderHealth::Ok,
                    auth_state: AuthState::Ready,
                    error: None,
                    snapshot: snapshot_claude_primary(),
                },
            ),
            demo_account(
                provider,
                DemoAccount {
                    account_id: CLAUDE_SECONDARY_ID,
                    label: "labs@example.com",
                    last_success_at: now - Duration::minutes(8),
                    health: ProviderHealth::Ok,
                    auth_state: AuthState::Ready,
                    error: None,
                    snapshot: snapshot_claude_secondary(),
                },
            ),
        ],
        ProviderId::Cursor => vec![
            demo_account(
                provider,
                DemoAccount {
                    account_id: CURSOR_PRIMARY_ID,
                    label: "solo@example.com",
                    last_success_at: now - Duration::minutes(1),
                    health: ProviderHealth::Ok,
                    auth_state: AuthState::Ready,
                    error: None,
                    snapshot: snapshot_cursor_primary(),
                },
            ),
            demo_account(
                provider,
                DemoAccount {
                    account_id: CURSOR_SECONDARY_ID,
                    label: "reauth@example.com",
                    last_success_at: now - Duration::minutes(14),
                    health: ProviderHealth::Error,
                    auth_state: AuthState::ActionRequired,
                    error: Some("Login required".to_string()),
                    snapshot: snapshot_cursor_secondary(),
                },
            ),
        ],
    }
}

struct DemoAccount {
    account_id: &'static str,
    label: &'static str,
    last_success_at: DateTime<Utc>,
    health: ProviderHealth,
    auth_state: AuthState,
    error: Option<String>,
    snapshot: UsageSnapshot,
}

fn demo_account(provider: ProviderId, account: DemoAccount) -> ProviderAccountRuntimeState {
    ProviderAccountRuntimeState {
        provider,
        account_id: account.account_id.to_string(),
        label: account.label.to_string(),
        source_label: Some(demo_source(provider)),
        last_success_at: Some(account.last_success_at),
        snapshot: Some(account.snapshot),
        health: account.health,
        auth_state: account.auth_state,
        error: account.error,
        rate_limit_until: None,
        consecutive_rate_limits: 0,
    }
}

fn codex_demo_windows_ceiling(now: DateTime<Utc>) -> Vec<UsageWindow> {
    let session_end = now + Duration::hours(1);
    let weekly_end = now + Duration::days(3);
    vec![
        UsageWindow {
            label: "Session".to_string(),
            used_percent: 100.0,
            reset_at: Some(session_end),
            window_seconds: Some(5 * 60 * 60),
            reset_description: None,
        },
        UsageWindow {
            label: "Weekly".to_string(),
            used_percent: 100.0,
            reset_at: Some(weekly_end),
            window_seconds: Some(7 * 24 * 3600),
            reset_description: None,
        },
    ]
}

fn snapshot_codex_primary() -> UsageSnapshot {
    let now = Utc::now();
    UsageSnapshot {
        provider: ProviderId::Codex,
        source: "OAuth".to_string(),
        updated_at: now,
        headline: UsageHeadline(0),
        windows: codex_demo_windows_ceiling(now),
        provider_cost: Some(ProviderCost {
            used: 0.0,
            limit: None,
            units: "credits".to_string(),
        }),
        extra_usage: None,
        identity: ProviderIdentity {
            email: Some("ada@example.com".to_string()),
            account_id: Some("demo-acct-8f2a1c".to_string()),
            plan: Some("plus".to_string()),
            display_name: Some("Ada".to_string()),
        },
    }
}

fn snapshot_codex_secondary() -> UsageSnapshot {
    let now = Utc::now();
    UsageSnapshot {
        provider: ProviderId::Codex,
        source: "OAuth".to_string(),
        updated_at: now,
        headline: UsageHeadline(0),
        windows: codex_demo_windows_ceiling(now),
        provider_cost: Some(ProviderCost {
            used: 0.0,
            limit: None,
            units: "credits".to_string(),
        }),
        extra_usage: None,
        identity: ProviderIdentity {
            email: Some("pair@example.com".to_string()),
            account_id: Some("demo-acct-31be7d".to_string()),
            plan: Some("pro".to_string()),
            display_name: Some("Pair".to_string()),
        },
    }
}

fn snapshot_codex_tertiary() -> UsageSnapshot {
    let now = Utc::now();
    UsageSnapshot {
        provider: ProviderId::Codex,
        source: "OAuth".to_string(),
        updated_at: now,
        headline: UsageHeadline(0),
        windows: codex_demo_windows_ceiling(now),
        provider_cost: Some(ProviderCost {
            used: 0.0,
            limit: None,
            units: "credits".to_string(),
        }),
        extra_usage: None,
        identity: ProviderIdentity {
            email: Some("tri@example.com".to_string()),
            account_id: Some("demo-acct-9cce4e".to_string()),
            plan: Some("team".to_string()),
            display_name: Some("Tri".to_string()),
        },
    }
}

fn snapshot_claude_primary() -> UsageSnapshot {
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
        extra_usage: Some(ExtraUsageState::Active {
            used_percent: 63.75,
            cost: ProviderCost {
                used: 127.5,
                limit: Some(200.0),
                units: "$".to_string(),
            },
        }),
        identity: ProviderIdentity {
            email: Some("team@example.com".to_string()),
            account_id: None,
            plan: Some("pro".to_string()),
            display_name: Some("Team".to_string()),
        },
    }
}

fn snapshot_claude_secondary() -> UsageSnapshot {
    let now = Utc::now();
    UsageSnapshot {
        provider: ProviderId::Claude,
        source: "OAuth".to_string(),
        updated_at: now,
        headline: UsageHeadline(0),
        windows: vec![
            UsageWindow {
                label: "Session".to_string(),
                used_percent: 18.0,
                reset_at: Some(now + Duration::hours(4)),
                window_seconds: Some(5 * 60 * 60),
                reset_description: None,
            },
            UsageWindow {
                label: "Weekly".to_string(),
                used_percent: 52.0,
                reset_at: Some(now + Duration::days(3)),
                window_seconds: Some(7 * 24 * 3600),
                reset_description: None,
            },
            UsageWindow {
                label: "Sonnet (weekly)".to_string(),
                used_percent: 29.0,
                reset_at: Some(now + Duration::days(5)),
                window_seconds: Some(7 * 24 * 3600),
                reset_description: None,
            },
        ],
        provider_cost: None,
        extra_usage: None,
        identity: ProviderIdentity {
            email: Some("labs@example.com".to_string()),
            account_id: None,
            plan: Some("max".to_string()),
            display_name: Some("Labs".to_string()),
        },
    }
}

fn snapshot_cursor_primary() -> UsageSnapshot {
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
        extra_usage: None,
        identity: ProviderIdentity {
            email: Some("solo@example.com".to_string()),
            account_id: None,
            plan: Some("pro".to_string()),
            display_name: Some("Solo".to_string()),
        },
    }
}

fn snapshot_cursor_secondary() -> UsageSnapshot {
    let now = Utc::now();
    let reset_at = now + Duration::days(11);
    let start = reset_at - Duration::days(30);
    let window_seconds = (reset_at - start).num_seconds();
    UsageSnapshot {
        provider: ProviderId::Cursor,
        source: "Managed Account".to_string(),
        updated_at: now,
        headline: UsageHeadline(0),
        windows: vec![
            window_cursor("Total", 92.0, reset_at, window_seconds),
            window_cursor("Auto + Composer", 88.0, reset_at, window_seconds),
            window_cursor("API", 96.0, reset_at, window_seconds),
        ],
        provider_cost: None,
        extra_usage: None,
        identity: ProviderIdentity {
            email: Some("reauth@example.com".to_string()),
            account_id: None,
            plan: Some("pro".to_string()),
            display_name: Some("Reauth".to_string()),
        },
    }
}

fn demo_codex_accounts() -> Vec<ManagedCodexAccountConfig> {
    let now = Utc::now();
    vec![
        ManagedCodexAccountConfig {
            id: CODEX_PRIMARY_ID.to_string(),
            label: "ada@example.com".to_string(),
            codex_home: demo_root().join("codex-primary"),
            email: Some("ada@example.com".to_string()),
            provider_account_id: Some("demo-acct-8f2a1c".to_string()),
            created_at: now,
            updated_at: now,
            last_authenticated_at: Some(now),
        },
        ManagedCodexAccountConfig {
            id: CODEX_SECONDARY_ID.to_string(),
            label: "pair@example.com".to_string(),
            codex_home: demo_root().join("codex-secondary"),
            email: Some("pair@example.com".to_string()),
            provider_account_id: Some("demo-acct-31be7d".to_string()),
            created_at: now,
            updated_at: now,
            last_authenticated_at: Some(now),
        },
        ManagedCodexAccountConfig {
            id: CODEX_TERTIARY_ID.to_string(),
            label: "tri@example.com".to_string(),
            codex_home: demo_root().join("codex-tertiary"),
            email: Some("tri@example.com".to_string()),
            provider_account_id: Some("demo-acct-9cce4e".to_string()),
            created_at: now,
            updated_at: now,
            last_authenticated_at: Some(now),
        },
    ]
}

fn demo_claude_accounts() -> Vec<ManagedClaudeAccountConfig> {
    let now = Utc::now();
    vec![
        ManagedClaudeAccountConfig {
            id: CLAUDE_PRIMARY_ID.to_string(),
            label: "team@example.com".to_string(),
            config_dir: demo_root().join("claude-primary"),
            email: Some("team@example.com".to_string()),
            organization: Some("YapCap".to_string()),
            subscription_type: Some("pro".to_string()),
            created_at: now,
            updated_at: now,
            last_authenticated_at: Some(now),
        },
        ManagedClaudeAccountConfig {
            id: CLAUDE_SECONDARY_ID.to_string(),
            label: "labs@example.com".to_string(),
            config_dir: demo_root().join("claude-secondary"),
            email: Some("labs@example.com".to_string()),
            organization: Some("YapCap Labs".to_string()),
            subscription_type: Some("max".to_string()),
            created_at: now,
            updated_at: now,
            last_authenticated_at: Some(now),
        },
    ]
}

fn demo_cursor_accounts() -> Vec<ManagedCursorAccountConfig> {
    let now = Utc::now();
    vec![
        ManagedCursorAccountConfig {
            id: CURSOR_PRIMARY_ID.to_string(),
            email: "solo@example.com".to_string(),
            label: "solo@example.com".to_string(),
            account_root: demo_root().join("cursor-primary"),
            display_name: Some("Solo".to_string()),
            plan: Some("pro".to_string()),
            created_at: now,
            updated_at: now,
            last_authenticated_at: Some(now),
        },
        ManagedCursorAccountConfig {
            id: CURSOR_SECONDARY_ID.to_string(),
            email: "reauth@example.com".to_string(),
            label: "reauth@example.com".to_string(),
            account_root: demo_root().join("cursor-secondary"),
            display_name: Some("Reauth".to_string()),
            plan: Some("pro".to_string()),
            created_at: now,
            updated_at: now,
            last_authenticated_at: Some(now - Duration::days(1)),
        },
    ]
}

fn demo_root() -> PathBuf {
    paths().cache_dir.join("demo")
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
    use crate::test_support;

    #[test]
    fn build_snapshots_valid() {
        for snapshot in [
            snapshot_codex_primary(),
            snapshot_codex_secondary(),
            snapshot_codex_tertiary(),
            snapshot_claude_primary(),
            snapshot_claude_secondary(),
            snapshot_cursor_primary(),
            snapshot_cursor_secondary(),
        ] {
            assert!(!snapshot.windows.is_empty());
            assert!(snapshot.identity.email.is_some());
        }
    }

    #[test]
    fn demo_config_is_multi_account_ready() {
        let _guard = test_support::env_lock();
        unsafe {
            std::env::set_var(DEMO_ENV, "1");
        }
        let mut config = Config::default();
        apply_config(&mut config);
        unsafe {
            std::env::remove_var(DEMO_ENV);
        }

        assert_eq!(config.codex_managed_accounts.len(), 3);
        assert_eq!(config.claude_managed_accounts.len(), 2);
        assert_eq!(config.cursor_managed_accounts.len(), 2);
        assert_eq!(config.selected_codex_account_ids.len(), 3);
        assert_eq!(config.selected_claude_account_ids.len(), 2);
        assert_eq!(config.selected_cursor_account_ids.len(), 2);
        assert!(config.show_all_accounts(ProviderId::Codex));
        assert!(config.show_all_accounts(ProviderId::Claude));
        assert!(config.show_all_accounts(ProviderId::Cursor));
        assert_eq!(
            config.provider_visibility_mode,
            ProviderVisibilityMode::UserManaged
        );
    }

    #[test]
    fn codex_demo_snapshots_have_full_session_and_weekly() {
        for snapshot in [
            snapshot_codex_primary(),
            snapshot_codex_secondary(),
            snapshot_codex_tertiary(),
        ] {
            assert_eq!(snapshot.windows.len(), 2);
            for window in &snapshot.windows {
                assert!((window.used_percent - 100.0).abs() < f32::EPSILON);
            }
        }
    }

    #[test]
    fn claude_demo_primary_includes_extra_usage() {
        assert!(matches!(
            snapshot_claude_primary().extra_usage.as_ref(),
            Some(ExtraUsageState::Active { .. })
        ));
        assert!(snapshot_claude_secondary().extra_usage.is_none());
    }
}
