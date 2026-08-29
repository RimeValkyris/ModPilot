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

/// Emitted when a running server's player roster changes (someone joined
/// or left), so the UI updates live instead of polling.
pub const PLAYERS_EVENT: &str = "instance-players-changed";

/// Emitted when a running server trips a sustained resource threshold -
/// see `server::alerts`.
pub const RESOURCE_ALERT_EVENT: &str = "instance-resource-alert";

/// Emitted when a linked instance has a newer Modrinth version available -
/// see `server::autoupdate`.
pub const MODPACK_UPDATE_EVENT: &str = "instance-modpack-update-available";

/// Emitted while an FTB modpack is being installed. Unlike an mrpack's
/// few dozen files, an FTB pack routinely downloads 2000+, so the install
/// reports progress rather than blocking silently for several minutes.
pub const FTB_INSTALL_PROGRESS_EVENT: &str = "ftb-install-progress";

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayersChangedPayload {
    pub instance_id: String,
    pub players: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceAlertPayload {
    pub instance_id: String,
    pub kind: &'static str,
    pub message: String,
}

/// `phase` is "downloading" | "installing-loader" | "done" - the loader
/// step has no per-file progress of its own, so it carries `detail` text
/// instead of a moving count.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FtbInstallProgressPayload {
    pub phase: String,
    pub done: usize,
    pub total: usize,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModpackUpdateAvailablePayload {
    pub instance_id: String,
    pub version_name: String,
    pub version_number: String,
    /// True when the instance policy is "auto" and installation has begun.
    pub installing: bool,
}
