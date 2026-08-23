use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A detected Java runtime installation available on the host machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaInstallation {
    pub id: String,
    pub version: String,
    pub vendor: Option<String>,
    pub path: String,
    pub architecture: String,
    pub is_default: bool,
    pub detected_at: DateTime<Utc>,
}

/// Raw row shape as stored in SQLite - see [`super::instance::InstanceRow`]
/// for why this is kept separate from [`JavaInstallation`] (SQLite has no
/// native bool).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct JavaInstallationRow {
    pub id: String,
    pub version: String,
    pub vendor: Option<String>,
    pub path: String,
    pub architecture: String,
    pub is_default: i64,
    pub detected_at: DateTime<Utc>,
}

impl From<JavaInstallationRow> for JavaInstallation {
    fn from(row: JavaInstallationRow) -> Self {
        JavaInstallation {
            id: row.id,
            version: row.version,
            vendor: row.vendor,
            path: row.path,
            architecture: row.architecture,
            is_default: row.is_default != 0,
            detected_at: row.detected_at,
        }
    }
}

/// A Java runtime found on disk, before it has an `id`/`detected_at` -
/// what the `java` detection module produces, and what gets upserted into
/// the `java_installations` table.
#[derive(Debug, Clone)]
pub struct DetectedJava {
    pub version: String,
    pub vendor: Option<String>,
    pub path: String,
    pub architecture: String,
}
