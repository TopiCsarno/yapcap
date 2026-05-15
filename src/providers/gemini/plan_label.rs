// SPDX-License-Identifier: MPL-2.0

pub const PLAN_FREE: &str = "Free";
pub const PLAN_PRO: &str = "Pro";
pub const PLAN_WORKSPACE: &str = "Workspace";
pub const PLAN_LEGACY: &str = "Legacy";
pub const PLAN_FALLBACK: &str = "Plan";

#[must_use]
pub fn plan_label(tier_id: &str, hd_present: bool) -> &'static str {
    match tier_id {
        "free-tier" => PLAN_FREE,
        "standard-tier" if hd_present => PLAN_WORKSPACE,
        "standard-tier" => PLAN_PRO,
        "legacy-tier" => PLAN_LEGACY,
        _ => PLAN_FALLBACK,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_documented_combinations() {
        let cases: &[(&str, bool, &str)] = &[
            ("free-tier", false, PLAN_FREE),
            ("free-tier", true, PLAN_FREE),
            ("standard-tier", false, PLAN_PRO),
            ("standard-tier", true, PLAN_WORKSPACE),
            ("legacy-tier", false, PLAN_LEGACY),
            ("legacy-tier", true, PLAN_LEGACY),
            ("future-enterprise-tier", false, PLAN_FALLBACK),
            ("", false, PLAN_FALLBACK),
        ];
        for (tier, hd, expected) in cases {
            assert_eq!(
                plan_label(tier, *hd),
                *expected,
                "tier={tier:?} hd_present={hd}"
            );
        }
    }
}
