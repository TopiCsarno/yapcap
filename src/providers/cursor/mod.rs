// SPDX-License-Identifier: MPL-2.0

mod debug;
mod discovery;
mod identity;
mod login;
mod maintenance;
mod refresh;
mod shared;
mod storage;
mod types;

pub use login::{CursorLoginEvent, CursorLoginState, CursorLoginStatus, LOGIN_URL, prepare};
pub use refresh::{fetch, fetch_at};

pub use identity::{find_managed_account, managed_account_id};

#[cfg(debug_assertions)]
pub(crate) use debug::simulate_expired_cookie_accounts;
pub(crate) use discovery::{discover_accounts, discover_browser_accounts};
pub(crate) use identity::{managed_config_id, normalized_email};
pub(crate) use maintenance::{cleanup_pending_dirs, sync_managed_accounts, upsert_managed_account};
pub(crate) use storage::remove_managed_profile;

#[cfg(debug_assertions)]
pub(crate) use debug::expired_cookie_debug_enabled;
