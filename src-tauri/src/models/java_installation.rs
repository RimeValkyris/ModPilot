use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A detected Java runtime installation available on the host machine.
///
/// Populated by the Java manager (Phase 4). The model is defined now so the
/// `instances` table can reference `java_installations(id)` from Phase 1.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct JavaInstallation {
    pub id: String,
    pub version: String,
    pub vendor: Option<String>,
    pub path: String,
    pub architecture: String,
    /// SQLite has no native bool; stored/read as 0 or 1.
    pub is_default: i64,
    pub detected_at: DateTime<Utc>,
}
