// SPDX-License-Identifier: MPL-2.0

use super::*;

fn parse(data: &Value) -> Result<UsageSnapshot, CommandCodeError> {
    usage::parse(data, None, ProviderIdentity::default(), chrono::Utc::now())
}

#[test]
fn parses_service_reported_limits_and_extra_credit_balance() {
    let snapshot = usage::parse(
        &credits(),
        Some(&json!({"data":{"planId":"individual-pro-v1","status":"active"}})),
        ProviderIdentity::default(),
        chrono::Utc::now(),
    )
    .unwrap();
    assert_eq!(snapshot.provider, ProviderId::CommandCode);
    let cost = snapshot.provider_cost.unwrap();
    assert_eq!(cost.used, 4.0);
    assert!(cost.limit.is_none());
    assert_eq!(cost.units, "credits");
    assert_eq!(snapshot.windows.len(), 2);
    assert_eq!(snapshot.windows[0].used_percent, 25.0);
    assert_eq!(snapshot.windows[1].used_percent, 50.0);
    assert_eq!(snapshot.windows[0].reset_at, snapshot.windows[1].reset_at);
    assert_eq!(snapshot.windows[0].window_seconds, Some(18_000));
    assert_eq!(snapshot.identity.plan.as_deref(), Some("Pro"));
}

#[test]
fn included_plan_credits_never_inflate_tiny_extra_balances() {
    let data = json!({
        "credits":{"monthlyCredits":20,"purchasedCredits":0.004,"freeCredits":0}
    });
    assert_eq!(parse(&data).unwrap().provider_cost.unwrap().used, 0.004);
}

#[test]
fn included_plan_credits_never_turn_zero_extra_credits_into_a_positive_balance() {
    let data = json!({
        "credits":{"monthlyCredits":20,"purchasedCredits":0,"freeCredits":0}
    });
    assert_eq!(parse(&data).unwrap().provider_cost.unwrap().used, 0.0);
}

#[test]
fn pay_as_you_go_does_not_invent_limits_or_monthly_budgets() {
    let data = json!({"credits":{"purchasedCredits":3,"freeCredits":1}});
    let snapshot = parse(&data).unwrap();
    assert!(snapshot.windows.is_empty());
    assert_eq!(snapshot.provider_cost.unwrap().used, 4.0);
}

#[test]
fn invalid_and_zero_caps_are_not_shown_as_healthy_windows() {
    let mut data = credits();
    data["windowLimits"]["fiveHour"]["cap"] = json!(0);
    data["windowLimits"]["weekly"]["used"] = json!(-1);
    assert!(parse(&data).unwrap().windows.is_empty());
}

#[test]
fn exhausted_limits_are_clamped_and_unknown_resets_stay_unknown() {
    let mut data = credits();
    data["windowLimits"]["fiveHour"]["used"] = json!(40);
    data["windowLimits"]["fiveHour"]["resetAt"] = json!("not-a-date");
    let snapshot = parse(&data).unwrap();
    assert_eq!(snapshot.windows[0].used_percent, 100.0);
    assert!(snapshot.windows[0].reset_at.is_none());
}

#[test]
fn invalid_extra_pools_are_not_silently_reported_as_available() {
    for name in ["purchasedCredits", "freeCredits"] {
        for invalid in [json!(null), json!("unknown"), json!(-1)] {
            let mut data = credits();
            data["credits"][name] = invalid;
            data.as_object_mut().unwrap().remove("windowLimits");
            assert!(matches!(
                parse(&data),
                Err(CommandCodeError::InvalidResponse)
            ));
        }
    }
}

#[test]
fn missing_extra_credit_pools_are_not_misreported_as_zero_balance() {
    for data in [
        json!({}),
        json!({"credits":{}}),
        json!({"credits":{"monthlyCredits":20}}),
        json!({"credits":{"purchasedCredits":0}}),
        json!({"credits":{"freeCredits":0}}),
    ] {
        assert!(matches!(
            parse(&data),
            Err(CommandCodeError::InvalidResponse)
        ));
    }
}

#[test]
fn overflowing_extra_credit_pools_are_rejected() {
    let data = json!({
        "credits":{"purchasedCredits":f64::MAX,"freeCredits":f64::MAX}
    });
    assert!(matches!(
        parse(&data),
        Err(CommandCodeError::InvalidResponse)
    ));
}

#[test]
fn invalid_or_missing_extra_pools_preserve_valid_rolling_quotas() {
    for name in ["purchasedCredits", "freeCredits"] {
        for invalid in [json!(null), json!("unknown"), json!(-1)] {
            let mut data = credits();
            data["credits"][name] = invalid;
            let snapshot = parse(&data).unwrap();
            assert!(snapshot.provider_cost.is_none());
            assert_eq!(snapshot.windows.len(), 2);
        }
        let mut data = credits();
        data["credits"].as_object_mut().unwrap().remove(name);
        let snapshot = parse(&data).unwrap();
        assert!(snapshot.provider_cost.is_none());
        assert_eq!(snapshot.windows.len(), 2);
    }
}

#[test]
fn valid_quota_survives_a_missing_credit_object_and_invalid_sibling_window() {
    let mut data = credits();
    data.as_object_mut().unwrap().remove("credits");
    data["windowLimits"]["weekly"]["cap"] = json!(0);
    let snapshot = parse(&data).unwrap();
    assert!(snapshot.provider_cost.is_none());
    assert_eq!(snapshot.windows.len(), 1);
    assert_eq!(snapshot.windows[0].label, "5 Hour");
}

#[test]
fn unlimited_or_unspecified_plans_do_not_show_nominal_quota_bars() {
    for limited in [json!(false), json!(null), json!("true")] {
        let mut data = credits();
        data["windowLimits"]["limited"] = limited;
        let snapshot = parse(&data).unwrap();
        assert!(snapshot.windows.is_empty());
        assert_eq!(snapshot.provider_cost.unwrap().used, 4.0);
    }
    let mut data = credits();
    data["windowLimits"]
        .as_object_mut()
        .unwrap()
        .remove("limited");
    assert!(parse(&data).unwrap().windows.is_empty());
}

#[test]
fn unusable_balances_and_disabled_or_invalid_windows_are_an_error() {
    let mut data = credits();
    data["credits"]["purchasedCredits"] = json!(-1);
    data["windowLimits"]["limited"] = json!(false);
    assert!(matches!(
        parse(&data),
        Err(CommandCodeError::InvalidResponse)
    ));
    data["windowLimits"]["limited"] = json!(true);
    data["windowLimits"]["fiveHour"]["cap"] = json!(0);
    data["windowLimits"]["weekly"]["cap"] = json!(0);
    assert!(matches!(
        parse(&data),
        Err(CommandCodeError::InvalidResponse)
    ));
}

#[test]
fn extra_balance_survives_snapshot_serialization() {
    let snapshot = parse(&credits()).unwrap();
    let serialized = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(serialized["provider_cost"]["used"], json!(4.0));
    let restored: UsageSnapshot = serde_json::from_value(serialized).unwrap();
    assert_eq!(restored, snapshot);
}
