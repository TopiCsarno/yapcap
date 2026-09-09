// SPDX-License-Identifier: MPL-2.0

use crate::model::{ProviderId, UsageSnapshot};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const METADATA_FILE: &str = "metadata.json";
const TOKENS_FILE: &str = "tokens.json";
const SNAPSHOT_FILE: &str = "snapshot.json";
static ACCOUNT_ID_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderAccountRef {
    pub provider: ProviderId,
    pub account_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderAccountMetadata {
    pub account_id: String,
    pub provider: ProviderId,
    pub email: String,
    pub provider_account_id: Option<String>,
    pub organization_id: Option<String>,
    pub organization_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gemini_last_tier_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gemini_last_cloudaicompanion_project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub antigravity_last_tier_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderAccountTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
    pub scope: Vec<String>,
    pub token_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewProviderAccount {
    pub provider: ProviderId,
    pub email: String,
    pub provider_account_id: Option<String>,
    pub organization_id: Option<String>,
    pub organization_name: Option<String>,
    pub tokens: ProviderAccountTokens,
    pub snapshot: Option<UsageSnapshot>,
}

#[derive(Debug, Clone)]
pub struct StoredProviderAccount {
    pub account_ref: ProviderAccountRef,
    pub account_dir: PathBuf,
    pub metadata: ProviderAccountMetadata,
}

#[derive(Debug, Clone)]
pub struct ProviderAccountStorage {
    root: PathBuf,
}

impl ProviderAccountStorage {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn account_dir(&self, account_id: &str) -> Result<PathBuf, AccountStorageError> {
        validate_account_id(account_id)?;
        Ok(self.root.join(account_id))
    }

    /// # Errors
    ///
    /// Returns an error when the account directory cannot be created or any account-owned JSON
    /// file cannot be encoded or written.
    pub fn create_account(
        &self,
        account: NewProviderAccount,
    ) -> Result<StoredProviderAccount, AccountStorageError> {
        let account_id = Self::new_account_id(account.provider);
        self.write_account(account_id, account, None)
    }

    /// # Errors
    ///
    /// Returns an error when the account directory cannot be created or any account-owned JSON
    /// file cannot be encoded or written.
    pub fn replace_account(
        &self,
        account_id: String,
        account: NewProviderAccount,
    ) -> Result<StoredProviderAccount, AccountStorageError> {
        let created_at = self.load_metadata(&account_id).ok().map(|m| m.created_at);
        self.write_account(account_id, account, created_at)
    }

    pub fn replace_account_with_created_at(
        &self,
        account_id: String,
        account: NewProviderAccount,
        created_at: DateTime<Utc>,
    ) -> Result<StoredProviderAccount, AccountStorageError> {
        self.write_account(account_id, account, Some(created_at))
    }

    fn write_account(
        &self,
        account_id: String,
        account: NewProviderAccount,
        created_at: Option<DateTime<Utc>>,
    ) -> Result<StoredProviderAccount, AccountStorageError> {
        let account_dir = self.ensure_account_dir(&account_id)?;
        for file_name in [METADATA_FILE, TOKENS_FILE] {
            checked_account_file_path(&account_dir, file_name)?;
        }
        if account.snapshot.is_some() {
            checked_account_file_path(&account_dir, SNAPSHOT_FILE)?;
        }
        let now = Utc::now();
        let metadata = ProviderAccountMetadata {
            account_id: account_id.clone(),
            provider: account.provider,
            email: account.email,
            provider_account_id: account.provider_account_id,
            organization_id: account.organization_id,
            organization_name: account.organization_name,
            created_at: created_at.unwrap_or(now),
            updated_at: now,
            gemini_last_tier_id: None,
            gemini_last_cloudaicompanion_project: None,
            antigravity_last_tier_id: None,
        };

        self.write_json_file(&account_id, METADATA_FILE, &metadata)?;
        self.write_json_file(&account_id, TOKENS_FILE, &account.tokens)?;
        if let Some(snapshot) = account.snapshot {
            self.write_json_file(&account_id, SNAPSHOT_FILE, &snapshot)?;
        }

        Ok(StoredProviderAccount {
            account_ref: ProviderAccountRef {
                provider: metadata.provider,
                account_id,
            },
            account_dir,
            metadata,
        })
    }

    fn ensure_account_dir(&self, account_id: &str) -> Result<PathBuf, AccountStorageError> {
        let account_dir = self.account_dir(account_id)?;
        self.ensure_root()?;
        match fs::symlink_metadata(&account_dir) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(AccountStorageError::RefuseSymlink { path: account_dir });
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(AccountStorageError::NotDirectory { path: account_dir });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&account_dir).map_err(|source| AccountStorageError::CreateDir {
                    path: account_dir.clone(),
                    source,
                })?;
            }
            Err(source) => {
                return Err(AccountStorageError::ReadFile {
                    path: account_dir,
                    source,
                });
            }
        }
        set_private_dir_permissions(&account_dir)?;
        self.checked_existing_account_dir(account_id)?
            .ok_or(AccountStorageError::MissingAccountDirectory { path: account_dir })
    }

    /// # Errors
    ///
    /// Returns an error when `metadata.json` cannot be read or parsed.
    pub fn load_metadata(
        &self,
        account_id: &str,
    ) -> Result<ProviderAccountMetadata, AccountStorageError> {
        self.read_json_file(account_id, METADATA_FILE)
    }

    /// # Errors
    ///
    /// Returns an error when `tokens.json` cannot be read or parsed.
    pub fn load_tokens(
        &self,
        account_id: &str,
    ) -> Result<ProviderAccountTokens, AccountStorageError> {
        self.read_json_file(account_id, TOKENS_FILE)
    }

    /// # Errors
    ///
    /// Returns an error when `snapshot.json` exists but cannot be read or parsed.
    pub fn load_snapshot(
        &self,
        account_id: &str,
    ) -> Result<Option<UsageSnapshot>, AccountStorageError> {
        self.read_optional_json_file(account_id, SNAPSHOT_FILE)
    }

    /// # Errors
    ///
    /// Returns an error when the metadata cannot be encoded or written.
    pub fn save_metadata(
        &self,
        account_id: &str,
        metadata: &ProviderAccountMetadata,
    ) -> Result<(), AccountStorageError> {
        self.write_json_file(account_id, METADATA_FILE, metadata)
    }

    /// # Errors
    ///
    /// Returns an error when the tokens cannot be encoded or written.
    pub fn save_tokens(
        &self,
        account_id: &str,
        tokens: &ProviderAccountTokens,
    ) -> Result<(), AccountStorageError> {
        self.write_json_file(account_id, TOKENS_FILE, tokens)
    }

    /// # Errors
    ///
    /// Returns an error when the snapshot cannot be encoded or written.
    pub fn save_snapshot(
        &self,
        account_id: &str,
        snapshot: &UsageSnapshot,
    ) -> Result<(), AccountStorageError> {
        self.write_json_file(account_id, SNAPSHOT_FILE, snapshot)
    }

    /// # Errors
    ///
    /// Returns an error when the account directory is a symlink or cannot be deleted.
    pub fn delete_account(&self, account_id: &str) -> Result<bool, AccountStorageError> {
        let Some(account_dir) = self.checked_existing_account_dir(account_id)? else {
            return Ok(false);
        };
        for file_name in [METADATA_FILE, TOKENS_FILE, SNAPSHOT_FILE, "api_key.txt"] {
            checked_account_file_path(&account_dir, file_name)?;
        }
        fs::remove_dir_all(&account_dir).map_err(|source| AccountStorageError::DeleteDir {
            path: account_dir,
            source,
        })?;
        Ok(true)
    }

    fn required_existing_account_dir(
        &self,
        account_id: &str,
    ) -> Result<PathBuf, AccountStorageError> {
        self.checked_existing_account_dir(account_id)?
            .ok_or_else(|| AccountStorageError::MissingAccountDirectory {
                path: self.root.join(account_id),
            })
    }

    fn checked_existing_account_dir(
        &self,
        account_id: &str,
    ) -> Result<Option<PathBuf>, AccountStorageError> {
        let account_dir = self.account_dir(account_id)?;
        let metadata = match fs::symlink_metadata(&account_dir) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(AccountStorageError::ReadFile {
                    path: account_dir,
                    source,
                });
            }
        };
        if metadata.file_type().is_symlink() {
            return Err(AccountStorageError::RefuseSymlink { path: account_dir });
        }
        let root = self.canonical_root()?;
        let canonical_account_dir =
            account_dir
                .canonicalize()
                .map_err(|source| AccountStorageError::ResolvePath {
                    path: account_dir.clone(),
                    source,
                })?;
        if !canonical_account_dir.starts_with(&root) {
            return Err(AccountStorageError::OutsideStorageRoot {
                path: canonical_account_dir,
            });
        }
        set_private_dir_permissions(&account_dir)?;
        Ok(Some(account_dir))
    }

    fn canonical_root(&self) -> Result<PathBuf, AccountStorageError> {
        match fs::symlink_metadata(&self.root) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(AccountStorageError::RefuseSymlink {
                    path: self.root.clone(),
                });
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(AccountStorageError::NotDirectory {
                    path: self.root.clone(),
                });
            }
            Ok(_) => {}
            Err(source) => {
                return Err(AccountStorageError::ResolvePath {
                    path: self.root.clone(),
                    source,
                });
            }
        }
        self.root
            .canonicalize()
            .map_err(|source| AccountStorageError::ResolvePath {
                path: self.root.clone(),
                source,
            })
    }

    fn new_account_id(provider: ProviderId) -> String {
        let prefix = match provider {
            ProviderId::Codex => "codex",
            ProviderId::Claude => "claude",
            ProviderId::Cursor => "cursor",
            ProviderId::Gemini => "gemini",
            ProviderId::Copilot => "copilot",
            ProviderId::Minimax => "minimax",
            ProviderId::Kimi => "kimi",
            ProviderId::Antigravity => "antigravity",
            ProviderId::OpenCodeGo => "opencode_go",
            ProviderId::Grok => "grok",
        };
        let millis = Utc::now().timestamp_millis();
        let sequence = ACCOUNT_ID_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        format!("{prefix}-{millis}-{}-{sequence}", std::process::id())
    }

    pub fn write_json_file<T: Serialize>(
        &self,
        account_id: &str,
        file_name: &str,
        value: &T,
    ) -> Result<(), AccountStorageError> {
        let account_dir = self.ensure_account_dir(account_id)?;
        let path = checked_account_file_path(&account_dir, file_name)?;
        let payload =
            serde_json::to_vec_pretty(value).map_err(|source| AccountStorageError::EncodeFile {
                path: path.clone(),
                source,
            })?;
        write_checked_file(&path, &payload)
    }

    pub fn validate_account_files_for_write(
        &self,
        account_id: &str,
        file_names: &[&str],
    ) -> Result<(), AccountStorageError> {
        let account_dir = self.ensure_account_dir(account_id)?;
        for file_name in file_names {
            checked_account_file_path(&account_dir, file_name)?;
        }
        Ok(())
    }

    pub fn read_json_file<T: for<'de> Deserialize<'de>>(
        &self,
        account_id: &str,
        file_name: &str,
    ) -> Result<T, AccountStorageError> {
        let account_dir = self.required_existing_account_dir(account_id)?;
        let path = checked_account_file_path(&account_dir, file_name)?;
        let raw = read_checked_file(&path)?;
        serde_json::from_slice(&raw)
            .map_err(|source| AccountStorageError::ParseFile { path, source })
    }

    pub fn read_optional_json_file<T: for<'de> Deserialize<'de>>(
        &self,
        account_id: &str,
        file_name: &str,
    ) -> Result<Option<T>, AccountStorageError> {
        let Some(account_dir) = self.checked_existing_account_dir(account_id)? else {
            return Ok(None);
        };
        let path = checked_account_file_path(&account_dir, file_name)?;
        match read_checked_file(&path) {
            Ok(raw) => serde_json::from_slice(&raw)
                .map(Some)
                .map_err(|source| AccountStorageError::ParseFile { path, source }),
            Err(AccountStorageError::ReadFile { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    pub fn write_text_file(
        &self,
        account_id: &str,
        file_name: &str,
        value: &str,
    ) -> Result<(), AccountStorageError> {
        let account_dir = self.ensure_account_dir(account_id)?;
        let path = checked_account_file_path(&account_dir, file_name)?;
        write_checked_file(&path, value.as_bytes())
    }

    pub fn read_text_file(
        &self,
        account_id: &str,
        file_name: &str,
    ) -> Result<String, AccountStorageError> {
        let account_dir = self.required_existing_account_dir(account_id)?;
        let path = checked_account_file_path(&account_dir, file_name)?;
        String::from_utf8(read_checked_file(&path)?).map_err(|source| {
            AccountStorageError::ReadFile {
                path,
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, source),
            }
        })
    }

    fn ensure_root(&self) -> Result<(), AccountStorageError> {
        match fs::symlink_metadata(&self.root) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(AccountStorageError::RefuseSymlink {
                    path: self.root.clone(),
                });
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(AccountStorageError::NotDirectory {
                    path: self.root.clone(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir_all(&self.root).map_err(|source| {
                    AccountStorageError::CreateDir {
                        path: self.root.clone(),
                        source,
                    }
                })?;
                return self.ensure_root();
            }
            Err(source) => {
                return Err(AccountStorageError::ReadFile {
                    path: self.root.clone(),
                    source,
                });
            }
        }
        set_private_dir_permissions(&self.root)
    }
}

#[derive(Debug, Error)]
pub enum AccountStorageError {
    #[error("account ID must be one normal path component")]
    InvalidAccountId,
    #[error("failed to create account directory {path}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read account file {path}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse account file {path}")]
    ParseFile {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to encode account file {path}")]
    EncodeFile {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to write account file {path}")]
    WriteFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("refusing to delete symlinked account directory {path}")]
    RefuseSymlink { path: PathBuf },
    #[error("account directory is not a directory: {path}")]
    NotDirectory { path: PathBuf },
    #[error("account file is not a regular file: {path}")]
    NotFile { path: PathBuf },
    #[error("account directory does not exist: {path}")]
    MissingAccountDirectory { path: PathBuf },
    #[error("failed to resolve account storage path {path}")]
    ResolvePath {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("account directory resolves outside the storage root: {path}")]
    OutsideStorageRoot { path: PathBuf },
    #[error("failed to delete account directory {path}")]
    DeleteDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to set permissions on {path}")]
    SetPermissions {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl AccountStorageError {
    #[must_use]
    pub fn is_missing(&self) -> bool {
        match self {
            Self::MissingAccountDirectory { .. } => true,
            Self::ReadFile { source, .. } => source.kind() == std::io::ErrorKind::NotFound,
            _ => false,
        }
    }
}

pub fn validate_account_id(account_id: &str) -> Result<(), AccountStorageError> {
    if account_id.is_empty()
        || account_id.contains(['/', '\\'])
        || Path::new(account_id).is_absolute()
    {
        return Err(AccountStorageError::InvalidAccountId);
    }
    let mut components = Path::new(account_id).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(AccountStorageError::InvalidAccountId);
    }
    Ok(())
}

pub fn validated_account_dir(
    root: &Path,
    account_id: &str,
) -> Result<PathBuf, AccountStorageError> {
    validate_account_id(account_id)?;
    Ok(root.join(account_id))
}

fn checked_account_file_path(
    account_dir: &Path,
    file_name: &str,
) -> Result<PathBuf, AccountStorageError> {
    validate_account_id(file_name)?;
    let path = account_dir.join(file_name);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(AccountStorageError::RefuseSymlink { path })
        }
        Ok(metadata) if !metadata.is_file() => Err(AccountStorageError::NotFile { path }),
        Ok(_) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(path),
        Err(source) => Err(AccountStorageError::ReadFile { path, source }),
    }
}

fn write_checked_file(path: &Path, contents: &[u8]) -> Result<(), AccountStorageError> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW).mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|source| AccountStorageError::WriteFile {
            path: path.to_path_buf(),
            source,
        })?;
    #[cfg(unix)]
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|source| AccountStorageError::SetPermissions {
            path: path.to_path_buf(),
            source,
        })?;
    std::io::Write::write_all(&mut file, contents).map_err(|source| {
        AccountStorageError::WriteFile {
            path: path.to_path_buf(),
            source,
        }
    })?;
    Ok(())
}

fn read_checked_file(path: &Path) -> Result<Vec<u8>, AccountStorageError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(AccountStorageError::RefuseSymlink {
                path: path.to_path_buf(),
            });
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(AccountStorageError::NotFile {
                path: path.to_path_buf(),
            });
        }
        Ok(_) => {}
        Err(source) => {
            return Err(AccountStorageError::ReadFile {
                path: path.to_path_buf(),
                source,
            });
        }
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(path)
        .map_err(|source| AccountStorageError::ReadFile {
            path: path.to_path_buf(),
            source,
        })?;
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)
        .map_err(|source| AccountStorageError::ReadFile {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(contents)
}

/// # Errors
///
/// Returns an error when the directory cannot be created or its permissions cannot be set to
/// `0o700`.
pub fn create_private_dir(path: &Path) -> Result<(), AccountStorageError> {
    fs::create_dir_all(path).map_err(|source| AccountStorageError::CreateDir {
        path: path.to_path_buf(),
        source,
    })?;
    set_private_dir_permissions(path)
}

/// # Errors
///
/// Returns an error when the file's permissions cannot be set to `0o600`.
pub fn set_private_file_permissions(path: &Path) -> Result<(), AccountStorageError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
            AccountStorageError::SetPermissions {
                path: path.to_path_buf(),
                source,
            }
        })
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> Result<(), AccountStorageError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|source| {
        AccountStorageError::SetPermissions {
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &Path) -> Result<(), AccountStorageError> {
    Ok(())
}

/// # Errors
///
/// Returns an error when `value` cannot be encoded or `path` cannot be written.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), AccountStorageError> {
    let payload =
        serde_json::to_vec_pretty(value).map_err(|source| AccountStorageError::EncodeFile {
            path: path.to_path_buf(),
            source,
        })?;
    fs::write(path, payload).map_err(|source| AccountStorageError::WriteFile {
        path: path.to_path_buf(),
        source,
    })
}

/// # Errors
///
/// Returns an error when `path` cannot be read or its contents cannot be parsed.
pub fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, AccountStorageError> {
    let raw = fs::read_to_string(path).map_err(|source| AccountStorageError::ReadFile {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&raw).map_err(|source| AccountStorageError::ParseFile {
        path: path.to_path_buf(),
        source,
    })
}
