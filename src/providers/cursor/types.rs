// SPDX-License-Identifier: MPL-2.0

use crate::config::{Browser, CursorCredentialSource};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CursorManagedAccountFile {
    pub email: String,
    pub label: String,
    pub credential_source: CursorCredentialSource,
    pub browser: Option<Browser>,
    pub display_name: Option<String>,
    pub plan: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_authenticated_at: Option<DateTime<Utc>>,
}
