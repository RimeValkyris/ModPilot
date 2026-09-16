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

/// Emitted when auto-restart gives up on an instance that keeps crashing -
/// see `server::process::maybe_auto_restart` and `server::CrashTracker`.
///
/// Raised regardless of the OS-notification setting: that setting governs
/// whether the desktop is interrupted, not whether ModpackPilot's own UI is
/// allowed to know its server stopped trying to come back.
pub const CRASH_LOOP_EVENT: &str = "instance-crash-loop";

/// Emitted when a linked instance has a newer Modrinth version available -
/// see `server::autoupdate`.
pub const MODPACK_UPDATE_EVENT: &str = "instance-modpack-update-available";

/// Emitted while an FTB modpack is being installed. Unlike an mrpack's
/// few dozen files, an FTB pack routinely downloads 2000+, so the install
/// reports progress rather than blocking silently for several minutes.
pub const FTB_INSTALL_PROGRESS_EVENT: &str = "ftb-install-progress";

/// Emitted for each line a Forge/NeoForge installer prints while the manual
/// "Install Forge/NeoForge Server" step runs. The installer reports no
/// percentage of its own, so the step text is what tells an operator the
/// several-minute wait is still moving.
pub const LOADER_INSTALL_PROGRESS_EVENT: &str = "loader-install-progress";



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
pub struct CrashLoopPayload {
    pub instance_id: String,
    pub instance_name: String,
    /// How many consecutive crashes were seen before giving up.
    pub crash_count: u32,
    pub message: String,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderInstallProgressPayload {
    pub instance_id: String,
    /// The installer's current output line, e.g. "Downloading library ...".
    pub step: String,
}
