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

/// The result of reading a backup archive all the way through - see
/// `importer::verify_zip`. Reported rather than reduced to a boolean so the
/// operator can sanity-check the numbers against the world they expect
/// ("47 files" for a world that should have thousands is its own warning).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupVerification {
    pub name: String,
    pub file_count: u64,
    pub uncompressed_bytes: u64,
}

/// What a restore actually did on disk.
///
/// `displaced_world` names the folder the previous world was moved aside
/// to, if there was one. A restore never deletes the world it replaces, so
/// the UI can tell the operator exactly where their old save went and that
/// reclaiming the space is their call to make.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOutcome {
    pub restored_from: String,
    pub displaced_world: Option<String>,
    pub file_count: u64,
}
