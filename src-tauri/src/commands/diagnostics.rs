use tauri::State;

use super::instance::fetch_instance;
use crate::AppState;

/// Returns the app-level logs directory, for an "Open Logs Folder" button.
#[tauri::command]
pub fn get_app_logs_dir(state: State<'_, AppState>) -> String {
    state.paths.logs_dir.to_string_lossy().to_string()
}

/// Copies ModForge's own log (including any captured panic - see the
/// `std::panic::set_hook` in `lib.rs`) to a location the user picked via a
/// native save dialog, so a crash report can actually leave the machine.
#[tauri::command]
pub async fn export_app_log(state: State<'_, AppState>, dest_path: String) -> Result<(), String> {
    let source = state
        .paths
        .logs_dir
        .join(format!("modforge.log.{}", chrono::Utc::now().format("%Y-%m-%d")));

    tokio::fs::copy(&source, &dest_path)
        .await
        .map_err(|e| format!("Failed to export log: {e}"))?;

    Ok(())
}

/// Returns an instance's `logs/` directory, for an "Open Logs Folder" button.
#[tauri::command]
pub async fn get_instance_logs_dir(state: State<'_, AppState>, id: String) -> Result<String, String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    Ok(std::path::Path::new(&instance.server_directory)
        .join("logs")
        .to_string_lossy()
        .to_string())
}

/// Copies an instance's `logs/latest.log` to a user-chosen location -
/// the equivalent crash-log export for a server that crashed, rather than
/// ModForge itself.
#[tauri::command]
pub async fn export_instance_log(
    state: State<'_, AppState>,
    id: String,
    dest_path: String,
) -> Result<(), String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let source = std::path::Path::new(&instance.server_directory)
        .join("logs")
        .join("latest.log");

    tokio::fs::copy(&source, &dest_path)
        .await
        .map_err(|e| format!("Failed to export log: {e}"))?;

    Ok(())
}
