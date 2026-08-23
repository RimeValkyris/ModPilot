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
