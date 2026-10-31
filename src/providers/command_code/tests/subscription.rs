// SPDX-License-Identifier: MPL-2.0

use super::*;

fn plan(data: Value) -> Option<String> {
    usage::parse(
        &credits(),
        Some(&data),
        ProviderIdentity {
            plan: Some("Stale".to_string()),
            ..ProviderIdentity::default()
        },
        chrono::Utc::now(),
    )
    .unwrap()
    .identity
    .plan
}

#[test]
fn only_current_subscriptions_have_plan_labels() {
    for status in ["active", "trialing", "past_due"] {
        assert_eq!(
            plan(json!({"data":{"planId":"individual-pro-v1","status":status}})),
            Some("Pro".to_string())
        );
    }
    for status in ["canceled", "cancelled", "expired", "unpaid", "incomplete"] {
        assert!(plan(json!({"data":{"planId":"individual-pro","status":status}})).is_none());
    }
    assert!(plan(json!({"data":{"planId":"individual-pro"}})).is_none());
}

#[test]
fn known_plan_ids_use_display_names_without_prefix_collisions() {
    for (id, name) in [
        ("individual-go-v1", "Go"),
        ("INDIVIDUAL_GOAT", "GOAT"),
        ("individual-pro-v1", "Pro"),
        ("individual-provider", "Provider"),
        ("individual-max", "Max"),
        ("individual-ultra", "Ultra"),
        ("teams-pro", "Teams Pro"),
    ] {
        assert_eq!(
            plan(json!({"data":{"planId":id,"status":"active"}})),
            Some(name.to_string())
        );
    }
}

#[test]
fn unknown_or_malformed_plan_ids_are_not_displayed_raw() {
    for id in [
        json!("unknown-plan"),
        json!("individual-gold"),
        json!(""),
        json!(null),
    ] {
        assert!(plan(json!({"data":{"planId":id,"status":"active"}})).is_none());
    }
}
