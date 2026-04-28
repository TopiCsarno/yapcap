// SPDX-License-Identifier: MPL-2.0

use crate::config::Config;
use crate::providers::cursor::identity::{managed_account_id, normalized_email};
use std::path::{Path, PathBuf};

pub fn ambient_active_account_id(config: &Config) -> Option<String> {
    let db_path = cursor_state_db_path()?;
    if !db_path.exists() {
        return None;
    }
    let email = read_cached_email(&db_path)?;
    let email_key = normalized_email(&email);
    config
        .cursor_managed_accounts
        .iter()
        .find(|account| normalized_email(&account.email) == email_key)
        .map(|account| managed_account_id(&account.id))
}

fn cursor_state_db_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".config/Cursor/User/globalStorage"))
}

fn cursor_state_db_path() -> Option<PathBuf> {
    cursor_state_db_dir().map(|dir| dir.join("state.vscdb"))
}

fn read_cached_email(db_path: &Path) -> Option<String> {
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    conn.query_row(
        "SELECT value FROM ItemTable WHERE key = 'cursorAuth/cachedEmail'",
        [],
        |row| row.get(0),
    )
    .ok()
}
