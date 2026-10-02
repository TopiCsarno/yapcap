// SPDX-License-Identifier: MPL-2.0

use crate::account_storage::ProviderAccountStorage;
use std::path::Path;

pub const API_KEY_FILE: &str = "api_key.txt";

pub fn write_api_key(account_id: &str, api_key: &str) -> Result<(), String> {
    write_api_key_at(
        &crate::config::paths().openrouter_accounts_dir,
        account_id,
        api_key,
    )
}

pub fn load_api_key(account_id: &str) -> Result<String, String> {
    load_api_key_at(&crate::config::paths().openrouter_accounts_dir, account_id)
}

pub(crate) fn write_api_key_at(root: &Path, account_id: &str, api_key: &str) -> Result<(), String> {
    ProviderAccountStorage::new(root)
        .write_text_file(account_id, API_KEY_FILE, api_key)
        .map_err(|error| error.to_string())
}

pub(crate) fn load_api_key_at(root: &Path, account_id: &str) -> Result<String, String> {
    ProviderAccountStorage::new(root)
        .read_text_file(account_id, API_KEY_FILE)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn writes_under_the_managed_root() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("openrouter-accounts");
        write_api_key_at(&root, "openrouter-1", "test-key").unwrap();
        assert_eq!(load_api_key_at(&root, "openrouter-1").unwrap(), "test-key");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_account_and_api_key_paths() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let root = temp.path().join("openrouter-accounts");
        let outside = temp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        let target = outside.join("api_key.txt");
        fs::write(&target, "unchanged").unwrap();
        fs::create_dir_all(&root).unwrap();
        symlink(&outside, root.join("openrouter-1")).unwrap();

        assert!(write_api_key_at(&root, "openrouter-1", "replacement").is_err());
        assert!(load_api_key_at(&root, "openrouter-1").is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "unchanged");

        fs::remove_file(root.join("openrouter-1")).unwrap();
        fs::create_dir(root.join("openrouter-1")).unwrap();
        symlink(&target, root.join("openrouter-1").join(API_KEY_FILE)).unwrap();

        assert!(write_api_key_at(&root, "openrouter-1", "replacement").is_err());
        assert!(load_api_key_at(&root, "openrouter-1").is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "unchanged");
    }
}
