use serde::Serialize;

/// A point-in-time snapshot of a running instance's process resource usage.
/// Deliberately does not include TPS or player count - neither can be read
/// from the OS process, and ModpackPilot doesn't parse the server's own metrics
/// well enough yet to claim them reliably.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceUsage {
    pub is_running: bool,
    pub cpu_percent: f32,
    pub memory_mb: f64,
    pub uptime_seconds: i64,
}

impl ResourceUsage {
    pub fn not_running() -> Self {
        Self {
            is_running: false,
            cpu_percent: 0.0,
            memory_mb: 0.0,
            uptime_seconds: 0,
        }
    }
}
