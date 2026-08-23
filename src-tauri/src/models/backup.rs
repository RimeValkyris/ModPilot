use chrono::{DateTime, Utc};
use serde::Serialize;

/// A saved snapshot of an instance's world folder.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldBackup {
    pub name: String,
    pub size_bytes: u64,
    pub created_at: DateTime<Utc>,
}
