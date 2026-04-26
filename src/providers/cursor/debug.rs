// SPDX-License-Identifier: MPL-2.0

#[cfg(debug_assertions)]
use crate::config::ManagedCursorAccountConfig;
#[cfg(debug_assertions)]
use crate::providers::cursor::storage::{
    create_private_dir, debug_expired_cookie_override_path, session_dir,
};
#[cfg(debug_assertions)]
use std::fs;

#[cfg(debug_assertions)]
const DEBUG_CURSOR_EXPIRED_COOKIE_ENV: &str = "YAPCAP_DEBUG_CURSOR_EXPIRED_COOKIE";

#[cfg(debug_assertions)]
pub fn expired_cookie_debug_enabled() -> bool {
    std::env::var(DEBUG_CURSOR_EXPIRED_COOKIE_ENV)
        .is_ok_and(|value| debug_env_value_enabled(&value))
}

#[cfg(debug_assertions)]
fn debug_env_value_enabled(value: &str) -> bool {
    let value = value.trim();
    !(value == "0"
        || value.eq_ignore_ascii_case("false")
        || value.eq_ignore_ascii_case("no")
        || value.eq_ignore_ascii_case("off"))
}

#[cfg(debug_assertions)]
pub fn simulate_expired_cookie_accounts(accounts: &[ManagedCursorAccountConfig]) {
    for account in accounts {
        let session_root = session_dir(&account.account_root);
        if create_private_dir(&session_root).is_err() {
            continue;
        }
        let _ = fs::write(
            debug_expired_cookie_override_path(&account.account_root),
            "WorkosCursorSessionToken=expired-debug-session",
        );
    }
}

#[cfg(test)]
#[cfg(debug_assertions)]
mod tests {
    use super::*;
    use crate::test_support;

    #[test]
    fn expired_cookie_debug_flag_accepts_common_true_and_false_values() {
        let _guard = test_support::env_lock();
        unsafe {
            std::env::set_var(DEBUG_CURSOR_EXPIRED_COOKIE_ENV, "true");
        }
        assert!(expired_cookie_debug_enabled());

        unsafe {
            std::env::set_var(DEBUG_CURSOR_EXPIRED_COOKIE_ENV, "off");
        }
        assert!(!expired_cookie_debug_enabled());

        unsafe {
            std::env::remove_var(DEBUG_CURSOR_EXPIRED_COOKIE_ENV);
        }
        assert!(!expired_cookie_debug_enabled());
    }
}
