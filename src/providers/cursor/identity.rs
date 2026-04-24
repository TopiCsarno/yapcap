// SPDX-License-Identifier: MPL-2.0

const MANAGED_ACCOUNT_PREFIX: &str = "cursor-managed:";

pub fn managed_account_id(storage_id: &str) -> String {
    format!("{MANAGED_ACCOUNT_PREFIX}{storage_id}")
}

pub fn managed_config_id(account_id: &str) -> Option<&str> {
    account_id.strip_prefix(MANAGED_ACCOUNT_PREFIX)
}

pub fn find_managed_account<'a>(
    accounts: &'a [crate::config::ManagedCursorAccountConfig],
    runtime_account_id: &str,
) -> Option<&'a crate::config::ManagedCursorAccountConfig> {
    let key = managed_config_id(runtime_account_id)?;
    accounts
        .iter()
        .find(|account| (!account.id.is_empty() && account.id == key) || account.email == key)
}

pub fn normalized_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_account_id_round_trips() {
        let id = "cursor-deadbeef00000001";
        assert_eq!(managed_config_id(&managed_account_id(id)), Some(id));
    }
}
