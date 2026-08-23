use tauri::State;

use crate::models::{Instance, InstanceRow};
use crate::AppState;

/// Lists every server instance ModForge knows about, newest first.
///
/// Instance creation/import isn't implemented yet (Phases 2-3), so this
/// currently just reflects whatever rows exist in `instances` - an empty
/// vec until then.
#[tauri::command]
pub async fn list_instances(state: State<'_, AppState>) -> Result<Vec<Instance>, String> {
    let rows = sqlx::query_as::<_, InstanceRow>(
        "SELECT id, name, minecraft_version, loader, loader_version, java_installation_id,
                min_ram_mb, max_ram_mb, server_directory, server_jar, jvm_args, server_args,
                status, auto_start, auto_restart, created_at, last_launched_at
         FROM instances
         ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| format!("Failed to load instances: {e}"))?;

    Ok(rows.into_iter().map(Instance::from).collect())
}
