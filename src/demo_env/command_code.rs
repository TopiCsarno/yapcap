// SPDX-License-Identifier: MPL-2.0

use super::*;
use crate::config::ManagedCommandCodeAccountConfig;

const ID: &str = "yapcap-demo:command-code";

pub(super) fn configure(config: &mut Config) {
    let now = Utc::now();
    config.command_code_enablement = ProviderEnablement::Enabled;
    config.selected_command_code_account_ids = vec![ID.to_string()];
    config.command_code_managed_accounts = vec![ManagedCommandCodeAccountConfig {
        id: ID.to_string(),
        label: "Command Code".to_string(),
        created_at: now,
        updated_at: now,
        last_authenticated_at: Some(now),
    }];
}

pub(super) fn accounts(now: DateTime<Utc>) -> Vec<ProviderAccountRuntimeState> {
    let windows = [("5 Hour", 25.0, 18_000), ("Weekly", 50.0, 604_800)]
        .into_iter()
        .map(|(label, used_percent, seconds)| UsageWindow {
            label: label.to_string(),
            used_percent,
            reset_at: Some(now + Duration::seconds(seconds)),
            reset_description: None,
            window_seconds: Some(seconds),
            group: None,
        })
        .collect();
    vec![demo_account(
        ProviderId::CommandCode,
        DemoAccount {
            account_id: ID,
            label: "Command Code",
            last_success_at: now,
            health: ProviderHealth::Ok,
            auth_state: AuthState::Ready,
            error: None,
            snapshot: UsageSnapshot {
                provider: ProviderId::CommandCode,
                source: "Demo".to_string(),
                updated_at: now,
                headline: UsageHeadline(0),
                windows,
                provider_cost: Some(ProviderCost {
                    used: 4.0,
                    limit: None,
                    units: "credits".to_string(),
                }),
                extra_usage: None,
                identity: ProviderIdentity {
                    plan: Some("Demo".to_string()),
                    ..ProviderIdentity::default()
                },
            },
        },
    )]
}
