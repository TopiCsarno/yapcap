// SPDX-License-Identifier: MPL-2.0

use crate::config::paths;
use chrono::Utc;
use std::path::PathBuf;

pub(crate) fn stable_storage_id_from_normalized_email(email: &str) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0100_0000_01b3;
    let mut hash = OFFSET;
    for byte in email.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("cursor-{hash:016x}")
}

pub fn managed_account_dir(id: &str) -> PathBuf {
    paths().cursor_accounts_dir.join(id)
}

pub fn new_account_id() -> String {
    let millis = Utc::now().timestamp_millis();
    format!("cursor-{millis}-{}", std::process::id())
}
