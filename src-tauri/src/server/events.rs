use serde::Serialize;

use crate::models::ServerStatus;

/// Emitted whenever an instance's lifecycle state changes. The console and
/// dashboard both listen for this rather than polling `list_instances`.
pub const STATUS_EVENT: &str = "instance-status-changed";

/// Emitted for every line of stdout/stderr a running server produces.
pub const LOG_EVENT: &str = "instance-log";

/// Emitted once when a "starting" instance goes quiet for too long - see
/// `server::process`'s startup watchdog. Global (backed by the actual child
/// process's real activity, not local console UI state), so it fires
/// regardless of which page is open when it happens.
pub const STUCK_STARTING_EVENT: &str = "instance-stuck-starting";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusChangedPayload {
    pub instance_id: String,
    pub status: ServerStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StuckStartingPayload {
    pub instance_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLinePayload {
    pub instance_id: String,
    pub stream: &'static str,
    pub line: String,
}
