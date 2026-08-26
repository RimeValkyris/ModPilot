use std::collections::HashMap;

use sysinfo::System;
use tauri::State;

use crate::models::ResourceUsage;
use crate::AppState;

/// Reports CPU/RAM/uptime for a running instance's server process, or an
/// all-zero "not running" snapshot if it isn't currently running.
#[tauri::command]
pub async fn get_resource_usage(state: State<'_, AppState>, id: String) -> Result<ResourceUsage, String> {
    let Some((pid, started_at)) = state.processes.running_info(&id).await else {
        return Ok(ResourceUsage::not_running());
    };

    Ok(state.resource_monitor.sample(pid, started_at).await)
}

/// Same as `get_resource_usage`, but for every running instance in one
/// call - the frontend polls this once for the whole app rather than once
/// per visible instance card, which matters once several servers are
/// running simultaneously.
#[tauri::command]
pub async fn get_all_resource_usage(
    state: State<'_, AppState>,
) -> Result<HashMap<String, ResourceUsage>, String> {
    let snapshot = state.processes.running_snapshot().await;
    let mut result = HashMap::with_capacity(snapshot.len());
    for (id, pid, started_at) in snapshot {
        let usage = state.resource_monitor.sample(pid, started_at).await;
        result.insert(id, usage);
    }
    Ok(result)
}

/// Total physical RAM installed on this machine, in MB - used to show a
/// real "you have N GB available" hint next to an instance's RAM sliders
/// instead of leaving the operator to guess a safe value.
#[tauri::command]
pub fn get_system_memory_mb() -> u64 {
    let mut system = System::new();
    system.refresh_memory();
    system.total_memory() / (1024 * 1024)
}

/// Who is currently online on a running server, from the console-derived
/// roster (see `server::players`). Empty for a stopped instance.
#[tauri::command]
pub async fn list_online_players(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<String>, String> {
    Ok(state.players.list(&id).await)
}
