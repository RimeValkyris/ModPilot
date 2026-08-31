use chrono::{DateTime, Utc};
use sysinfo::{Pid, System};
use tokio::sync::Mutex;

use crate::models::ResourceUsage;

/// Wraps a single [`System`] that's refreshed per-query rather than
/// recreated each time - `sysinfo` computes CPU percentage from the delta
/// between two refreshes of the same process, so reusing one `System`
/// across calls is what makes the numbers meaningful instead of always 0.
#[derive(Default)]
pub struct ResourceMonitor {
    system: Mutex<System>,
}

impl ResourceMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Samples CPU/memory for `pid` (if given) and computes uptime from
    /// `started_at`. Returns "not running" if there's no PID to sample -
    /// e.g. the process record exists but the OS handle was never captured.
    pub async fn sample(&self, pid: Option<u32>, started_at: DateTime<Utc>) -> ResourceUsage {
        let Some(pid) = pid else {
            return ResourceUsage::not_running();
        };
        let sysinfo_pid = Pid::from_u32(pid);

        let mut system = self.system.lock().await;
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[sysinfo_pid]), true);

        let Some(process) = system.process(sysinfo_pid) else {
            return ResourceUsage::not_running();
        };

        let num_cpus = system.cpus().len().max(1) as f32;
        let cpu_percent = process.cpu_usage() / num_cpus;
        let memory_mb = process.memory() as f64 / (1024.0 * 1024.0);
        let uptime_seconds = (Utc::now() - started_at).num_seconds().max(0);

        // The process-derived fields only. Disk, ping and TPS are filled in
        // by `commands::monitor::enrich`, which has the instance context
        // this sampler deliberately doesn't need.
        ResourceUsage {
            is_running: true,
            cpu_percent,
            memory_mb,
            uptime_seconds,
            ..ResourceUsage::not_running()
        }
    }
}
