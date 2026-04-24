// SPDX-License-Identifier: MPL-2.0

use crate::config::{ManagedCursorAccountConfig, paths};
use crate::providers::cursor::types::CursorManagedAccountFile;
use std::fs;
use std::path::{Path, PathBuf};

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

pub fn account_metadata_path(root: &Path) -> PathBuf {
    root.join("account.json")
}

pub fn session_dir(root: &Path) -> PathBuf {
    root.join("session")
}

pub fn profile_dir(root: &Path) -> PathBuf {
    root.join("profile")
}

pub fn imported_cookie_header_path(root: &Path) -> PathBuf {
    session_dir(root).join("cookie_header")
}

#[cfg(debug_assertions)]
pub fn debug_expired_cookie_override_path(root: &Path) -> PathBuf {
    session_dir(root).join("debug_expired_cookie_header")
}

pub fn create_private_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path)
        .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
    set_private_dir_permissions(path)?;
    Ok(())
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = fs::Permissions::from_mode(0o700);
    fs::set_permissions(path, permissions)
        .map_err(|error| format!("failed to secure {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

pub fn remove_dir_if_exists(path: &Path) -> Result<(), String> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("failed to remove {}: {error}", path.display())),
    }
}

pub fn write_account_metadata(account: &ManagedCursorAccountConfig) -> Result<(), String> {
    create_private_dir(&account.account_root)?;
    create_private_dir(&session_dir(&account.account_root))?;
    let file = CursorManagedAccountFile {
        email: account.email.clone(),
        label: account.label.clone(),
        credential_source: account.credential_source,
        browser: account.browser,
        display_name: account.display_name.clone(),
        plan: account.plan.clone(),
        created_at: account.created_at,
        updated_at: account.updated_at,
        last_authenticated_at: account.last_authenticated_at,
    };
    let json = serde_json::to_vec_pretty(&file).map_err(|error| {
        format!(
            "failed to encode {}: {error}",
            account_metadata_path(&account.account_root).display()
        )
    })?;
    fs::write(account_metadata_path(&account.account_root), json).map_err(|error| {
        format!(
            "failed to write {}: {error}",
            account_metadata_path(&account.account_root).display()
        )
    })
}

pub fn write_imported_account(
    account: &ManagedCursorAccountConfig,
    cookie_header: &str,
) -> Result<(), String> {
    remove_managed_profile(&profile_dir(&account.account_root));
    create_private_dir(&account.account_root)?;
    create_private_dir(&session_dir(&account.account_root))?;
    fs::write(
        imported_cookie_header_path(&account.account_root),
        cookie_header.trim(),
    )
    .map_err(|error| {
        format!(
            "failed to write {}: {error}",
            imported_cookie_header_path(&account.account_root).display()
        )
    })?;
    write_account_metadata(account)
}

pub fn remove_managed_profile(profile_root: &Path) {
    let root = paths().cursor_accounts_dir;
    let Ok(root) = root.canonicalize() else {
        return;
    };
    let Ok(metadata) = fs::symlink_metadata(profile_root) else {
        return;
    };
    if metadata.file_type().is_symlink() {
        tracing::warn!(path = %profile_root.display(), "refusing to delete symlinked cursor account profile");
        return;
    }
    let Ok(resolved) = profile_root.canonicalize() else {
        return;
    };
    if !resolved.starts_with(&root) {
        return;
    }
    if let Err(error) = fs::remove_dir_all(&resolved) {
        tracing::warn!(path = %resolved.display(), error = %error, "failed to delete cursor account profile");
    }
}

pub struct PendingDirGuard {
    pub path: PathBuf,
}

impl Drop for PendingDirGuard {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.path) {
            tracing::debug!(path = %self.path.display(), error = %error, "failed to remove pending Cursor account dir");
        }
    }
}
