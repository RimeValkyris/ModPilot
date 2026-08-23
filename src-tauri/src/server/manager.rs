use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::io::AsyncWriteExt;
use tokio::process::ChildStdin;
use tokio::sync::{mpsc, Mutex};

/// Everything needed to interact with one instance's live server process.
/// Owned by [`ProcessManager`] for as long as the process is running.
pub struct RunningProcess {
    pub pid: Option<u32>,
    pub stdin: ChildStdin,
    /// Signals the watcher task (see `process::spawn_server_process`) to
    /// force-kill the child. Sending on this is force-stop; a graceful stop
    /// instead writes `stop\n` to `stdin` and lets the server shut itself
    /// down.
    pub kill_tx: mpsc::Sender<()>,
    /// Set before a stop is requested (graceful or forced) so the exit
    /// watcher can tell "the user stopped this" apart from "this crashed" -
    /// both look identical from the outside (the process just exits).
    pub stop_requested: Arc<AtomicBool>,
    pub launch_history_id: String,
    pub started_at: DateTime<Utc>,
}

/// Tracks every instance's live server process, keyed by instance id.
/// Lives in [`crate::AppState`] for the app's lifetime.
#[derive(Default)]
pub struct ProcessManager {
    processes: Mutex<HashMap<String, RunningProcess>>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn is_running(&self, instance_id: &str) -> bool {
        self.processes.lock().await.contains_key(instance_id)
    }

    pub async fn any_running(&self) -> bool {
        !self.processes.lock().await.is_empty()
    }

    pub async fn running_ids(&self) -> Vec<String> {
        self.processes.lock().await.keys().cloned().collect()
    }

    pub async fn insert(&self, instance_id: String, process: RunningProcess) {
        self.processes.lock().await.insert(instance_id, process);
    }

    pub async fn remove(&self, instance_id: &str) -> Option<RunningProcess> {
        self.processes.lock().await.remove(instance_id)
    }

    /// Writes a line to a running instance's stdin (used for both the
    /// graceful `stop` command and arbitrary console commands).
    pub async fn write_line(&self, instance_id: &str, line: &str) -> Result<(), String> {
        let mut processes = self.processes.lock().await;
        let process = processes
            .get_mut(instance_id)
            .ok_or_else(|| "Instance is not running".to_string())?;

        process
            .stdin
            .write_all(format!("{line}\n").as_bytes())
            .await
            .map_err(|e| format!("Failed to send command: {e}"))?;
        process
            .stdin
            .flush()
            .await
            .map_err(|e| format!("Failed to send command: {e}"))?;

        Ok(())
    }

    /// Marks the process as intentionally being stopped and returns the
    /// pieces needed to actually stop it, without holding the lock while
    /// doing I/O.
    pub async fn mark_stopping(&self, instance_id: &str) -> Result<Arc<AtomicBool>, String> {
        let processes = self.processes.lock().await;
        let process = processes
            .get(instance_id)
            .ok_or_else(|| "Instance is not running".to_string())?;
        process.stop_requested.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(process.stop_requested.clone())
    }

    pub async fn kill_sender(&self, instance_id: &str) -> Result<mpsc::Sender<()>, String> {
        let processes = self.processes.lock().await;
        processes
            .get(instance_id)
            .map(|p| p.kill_tx.clone())
            .ok_or_else(|| "Instance is not running".to_string())
    }

    /// Returns `(pid, started_at)` for a running instance, if it has one -
    /// used for resource monitoring (Phase 8).
    pub async fn running_info(&self, instance_id: &str) -> Option<(Option<u32>, DateTime<Utc>)> {
        let processes = self.processes.lock().await;
        processes.get(instance_id).map(|p| (p.pid, p.started_at))
    }

    /// Same as `running_info`, but for every running instance at once in a
    /// single lock acquisition - lets the frontend poll resource usage for
    /// all running instances with one IPC round trip instead of one per card.
    pub async fn running_snapshot(&self) -> Vec<(String, Option<u32>, DateTime<Utc>)> {
        let processes = self.processes.lock().await;
        processes
            .iter()
            .map(|(id, p)| (id.clone(), p.pid, p.started_at))
            .collect()
    }
}
