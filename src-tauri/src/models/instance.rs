use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The mod loader / server flavor an instance runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerLoader {
    Vanilla,
    Forge,
    NeoForge,
    Fabric,
    Quilt,
    #[default]
    Unknown,
}

impl ServerLoader {
    pub fn as_str(&self) -> &'static str {
        match self {
            ServerLoader::Vanilla => "vanilla",
            ServerLoader::Forge => "forge",
            ServerLoader::NeoForge => "neoforge",
            ServerLoader::Fabric => "fabric",
            ServerLoader::Quilt => "quilt",
            ServerLoader::Unknown => "unknown",
        }
    }
}

impl From<&str> for ServerLoader {
    fn from(value: &str) -> Self {
        match value {
            "vanilla" => ServerLoader::Vanilla,
            "forge" => ServerLoader::Forge,
            "neoforge" => ServerLoader::NeoForge,
            "fabric" => ServerLoader::Fabric,
            "quilt" => ServerLoader::Quilt,
            _ => ServerLoader::Unknown,
        }
    }
}

/// Lifecycle state of a server instance's process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Crashed,
}

impl ServerStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ServerStatus::Stopped => "stopped",
            ServerStatus::Starting => "starting",
            ServerStatus::Running => "running",
            ServerStatus::Stopping => "stopping",
            ServerStatus::Crashed => "crashed",
        }
    }
}

impl From<&str> for ServerStatus {
    fn from(value: &str) -> Self {
        match value {
            "starting" => ServerStatus::Starting,
            "running" => ServerStatus::Running,
            "stopping" => ServerStatus::Stopping,
            "crashed" => ServerStatus::Crashed,
            _ => ServerStatus::Stopped,
        }
    }
}

/// A Minecraft server instance managed by ModpackPilot.
///
/// This is the shape exposed to the frontend. It is assembled from
/// [`InstanceRow`], which mirrors the raw SQLite columns.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub minecraft_version: Option<String>,
    pub loader: ServerLoader,
    pub loader_version: Option<String>,
    pub java_installation_id: Option<String>,
    pub min_ram_mb: i64,
    pub max_ram_mb: i64,
    pub server_directory: String,
    pub server_jar: Option<String>,
    /// How `server_jar` should be launched: `"jar"` runs it directly
    /// (`java -jar <server_jar>`); `"argfile"` means `server_jar` is
    /// actually the path to a modern Forge/NeoForge `@`-argfile (see
    /// `crate::server::spawn_server_process`).
    pub launch_mode: String,
    pub jvm_args: Vec<String>,
    pub server_args: Vec<String>,
    pub status: ServerStatus,
    pub auto_start: bool,
    pub auto_restart: bool,
    pub created_at: DateTime<Utc>,
    pub last_launched_at: Option<DateTime<Utc>>,
    /// Set once an instance is linked to a Modrinth project via "Check for
    /// Updates" - `None` means it's not linked (e.g. imported from a plain
    /// ZIP/folder with no update source to check against).
    pub modrinth_project_id: Option<String>,
    pub modrinth_project_title: Option<String>,
    /// The version currently installed, if known. Distinguishes "linked but
    /// never checked" from "checked and up to date".
    pub modrinth_version_id: Option<String>,
    /// Automation schedules, `None` when disabled. See
    /// `migrations/0006_scheduling.sql` for the text format.
    pub restart_schedule: Option<String>,
    pub backup_schedule: Option<String>,
    /// How many world backups a scheduled backup keeps; 0 keeps all.
    pub backup_keep_last: i64,
    /// "off" | "notify" | "auto" - see migrations/0007_update_policy.sql.
    pub update_policy: String,
}

/// Raw row shape as stored in SQLite. Kept separate from [`Instance`] because
/// SQLite has no native enum/array/bool types - those are encoded as TEXT/INTEGER
/// and decoded here.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct InstanceRow {
    pub id: String,
    pub name: String,
    pub minecraft_version: Option<String>,
    pub loader: String,
    pub loader_version: Option<String>,
    pub java_installation_id: Option<String>,
    pub min_ram_mb: i64,
    pub max_ram_mb: i64,
    pub server_directory: String,
    pub server_jar: Option<String>,
    pub launch_mode: String,
    pub jvm_args: String,
    pub server_args: String,
    pub status: String,
    pub auto_start: i64,
    pub auto_restart: i64,
    pub created_at: DateTime<Utc>,
    pub last_launched_at: Option<DateTime<Utc>>,
    pub modrinth_project_id: Option<String>,
    pub modrinth_project_title: Option<String>,
    pub modrinth_version_id: Option<String>,
    pub restart_schedule: Option<String>,
    pub backup_schedule: Option<String>,
    pub backup_keep_last: i64,
    pub update_policy: String,
}

/// Input for creating a new instance manually (Phase 2). The importer
/// (Phase 3) will have its own request shape, since detected values there
/// come from the imported files rather than a form.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInstanceRequest {
    pub name: String,
    pub minecraft_version: Option<String>,
    pub loader: Option<ServerLoader>,
    pub loader_version: Option<String>,
    pub min_ram_mb: Option<i64>,
    pub max_ram_mb: Option<i64>,
}

/// Input for `update_instance_settings` (Phase 7 - the Configuration tab).
/// Every field is required in the request even though each maps to an
/// always-present column: the frontend always submits the full form, so
/// there's no ambiguity between "leave unchanged" and "clear it" to model.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInstanceSettingsRequest {
    pub server_jar: Option<String>,
    pub jvm_args: Vec<String>,
    pub server_args: Vec<String>,
    pub min_ram_mb: i64,
    pub max_ram_mb: i64,
    pub auto_start: bool,
    pub auto_restart: bool,
}

impl From<InstanceRow> for Instance {
    fn from(row: InstanceRow) -> Self {
        Instance {
            id: row.id,
            name: row.name,
            minecraft_version: row.minecraft_version,
            loader: ServerLoader::from(row.loader.as_str()),
            loader_version: row.loader_version,
            java_installation_id: row.java_installation_id,
            min_ram_mb: row.min_ram_mb,
            max_ram_mb: row.max_ram_mb,
            server_directory: row.server_directory,
            server_jar: row.server_jar,
            launch_mode: row.launch_mode,
            jvm_args: serde_json::from_str(&row.jvm_args).unwrap_or_default(),
            server_args: serde_json::from_str(&row.server_args).unwrap_or_default(),
            status: ServerStatus::from(row.status.as_str()),
            auto_start: row.auto_start != 0,
            auto_restart: row.auto_restart != 0,
            created_at: row.created_at,
            last_launched_at: row.last_launched_at,
            modrinth_project_id: row.modrinth_project_id,
            modrinth_project_title: row.modrinth_project_title,
            modrinth_version_id: row.modrinth_version_id,
            restart_schedule: row.restart_schedule,
            backup_schedule: row.backup_schedule,
            backup_keep_last: row.backup_keep_last,
            update_policy: row.update_policy,
        }
    }
}
