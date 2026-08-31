use serde::Serialize;

/// How much of the volume an instance lives on is in use.
///
/// The volume rather than the instance folder: see
/// [`crate::server::stats::sample_disk`] for why.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsage {
    pub used_percent: f32,
    pub used_bytes: u64,
    pub total_bytes: u64,
    /// Shown as a subtitle so an operator with several drives can tell which
    /// one the number refers to.
    pub mount_point: String,
}

/// What a Server List Ping got back.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerPing {
    pub latency_ms: u32,
    /// `None` when the server answered but omitted the field, which some
    /// player-count-hiding plugins do deliberately.
    pub players_online: Option<u32>,
    pub players_max: Option<u32>,
}

/// A point-in-time snapshot of a running instance, as the dashboard shows it.
///
/// The fields divide by where they come from, and each optional one is
/// optional for a reason - a metric that can't be measured right now is
/// reported as absent so the UI can say "unavailable" instead of drawing a
/// convincing zero:
///
/// - `cpu_percent`, `memory_mb`, `uptime_seconds` come off the OS process.
/// - `disk` is a property of the volume, sampled from the filesystem.
/// - `ping` needs the server to answer a Server List Ping, which it won't
///   while it's still booting.
/// - `tps` needs the server to support a tick-rate command at all.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceUsage {
    pub is_running: bool,
    pub cpu_percent: f32,
    pub memory_mb: f64,
    pub uptime_seconds: i64,
    pub disk: Option<DiskUsage>,
    pub ping: Option<ServerPing>,
    pub tps: Option<f32>,
    /// The console-derived roster's size, used when the ping didn't report a
    /// player count (or hasn't answered yet).
    pub players_tracked: u32,
}

impl ResourceUsage {
    pub fn not_running() -> Self {
        Self {
            is_running: false,
            cpu_percent: 0.0,
            memory_mb: 0.0,
            uptime_seconds: 0,
            disk: None,
            ping: None,
            tps: None,
            players_tracked: 0,
        }
    }
}
