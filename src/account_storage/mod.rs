// SPDX-License-Identifier: MPL-2.0

use crate::model::{ProviderId, UsageSnapshot};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

const METADATA_FILE: &str = "metadata.json";
const TOKENS_FILE: &str = "tokens.json";
const SNAPSHOT_FILE: &str = "snapshot.json";

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

    #[must_use]
    pub fn account_dir(&self, account_id: &str) -> PathBuf {
        self.root.join(account_id)
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

    fn write_account(
        &self,
        account_id: String,
        account: NewProviderAccount,
        created_at: Option<DateTime<Utc>>,
    ) -> Result<StoredProviderAccount, AccountStorageError> {
        let account_dir = self.account_dir(&account_id);
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
        };

        fs::create_dir_all(&account_dir).map_err(|source| AccountStorageError::CreateDir {
            path: account_dir.clone(),
            source,
        })?;
        write_json(&account_dir.join(METADATA_FILE), &metadata)?;
        write_json(&account_dir.join(TOKENS_FILE), &account.tokens)?;
        if let Some(snapshot) = account.snapshot {
            write_json(&account_dir.join(SNAPSHOT_FILE), &snapshot)?;
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

    /// # Errors
    ///
    /// Returns an error when `metadata.json` cannot be read or parsed.
    pub fn load_metadata(
        &self,
        account_id: &str,
    ) -> Result<ProviderAccountMetadata, AccountStorageError> {
        read_json(&self.account_dir(account_id).join(METADATA_FILE))
    }

    /// # Errors
    ///
    /// Returns an error when `tokens.json` cannot be read or parsed.
    pub fn load_tokens(
        &self,
        account_id: &str,
    ) -> Result<ProviderAccountTokens, AccountStorageError> {
        read_json(&self.account_dir(account_id).join(TOKENS_FILE))
    }

    /// # Errors
    ///
    /// Returns an error when `snapshot.json` exists but cannot be read or parsed.
    pub fn load_snapshot(
        &self,
        account_id: &str,
    ) -> Result<Option<UsageSnapshot>, AccountStorageError> {
        let path = self.account_dir(account_id).join(SNAPSHOT_FILE);
        if path.exists() {
            read_json(&path).map(Some)
        } else {
            Ok(None)
        }
    }

    /// # Errors
    ///
    /// Returns an error when the metadata cannot be encoded or written.
    pub fn save_metadata(
        &self,
        account_id: &str,
        metadata: &ProviderAccountMetadata,
    ) -> Result<(), AccountStorageError> {
        write_json(&self.account_dir(account_id).join(METADATA_FILE), metadata)
    }

    /// # Errors
    ///
    /// Returns an error when the tokens cannot be encoded or written.
    pub fn save_tokens(
        &self,
        account_id: &str,
        tokens: &ProviderAccountTokens,
    ) -> Result<(), AccountStorageError> {
        write_json(&self.account_dir(account_id).join(TOKENS_FILE), tokens)
    }

    /// # Errors
    ///
    /// Returns an error when the snapshot cannot be encoded or written.
    pub fn save_snapshot(
        &self,
        account_id: &str,
        snapshot: &UsageSnapshot,
    ) -> Result<(), AccountStorageError> {
        write_json(&self.account_dir(account_id).join(SNAPSHOT_FILE), snapshot)
    }

    /// # Errors
    ///
    /// Returns an error when the account directory is a symlink or cannot be deleted.
    pub fn delete_account(&self, account_id: &str) -> Result<bool, AccountStorageError> {
        let account_dir = self.account_dir(account_id);
        if !account_dir.exists() {
            return Ok(false);
        }
        if account_dir
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err(AccountStorageError::RefuseSymlink {
                path: account_dir.clone(),
            });
        }
        fs::remove_dir_all(&account_dir).map_err(|source| AccountStorageError::DeleteDir {
            path: account_dir,
            source,
        })?;
        Ok(true)
    }

    fn new_account_id(provider: ProviderId) -> String {
        let prefix = match provider {
            ProviderId::Codex => "codex",
            ProviderId::Claude => "claude",
            ProviderId::Cursor => "cursor",
        };
        let millis = Utc::now().timestamp_millis();
        format!("{prefix}-{millis}-{}", std::process::id())
    }
}

#[derive(Debug, Error)]
pub enum AccountStorageError {
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
    #[error("failed to delete account directory {path}")]
    DeleteDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), AccountStorageError> {
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

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, AccountStorageError> {
    let raw = fs::read_to_string(path).map_err(|source| AccountStorageError::ReadFile {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&raw).map_err(|source| AccountStorageError::ParseFile {
        path: path.to_path_buf(),
        source,
    })
}
