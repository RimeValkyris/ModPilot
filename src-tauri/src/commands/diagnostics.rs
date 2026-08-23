use tauri::State;

use super::instance::fetch_instance;
use crate::AppState;

/// Returns the app-level logs directory, for an "Open Logs Folder" button.
#[tauri::command]
pub fn get_app_logs_dir(state: State<'_, AppState>) -> String {
    state.paths.logs_dir.to_string_lossy().to_string()
}

/// Copies ModpackPilot's own log (including any captured panic - see the
/// `std::panic::set_hook` in `lib.rs`) to a location the user picked via a
/// native save dialog, so a crash report can actually leave the machine.
#[tauri::command]
pub async fn export_app_log(state: State<'_, AppState>, dest_path: String) -> Result<(), String> {
    let source = state
        .paths
        .logs_dir
        .join(format!("modpackpilot.log.{}", chrono::Utc::now().format("%Y-%m-%d")));

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

/// Resolves one of an instance's well-known folders for the Files tab's
/// "Open X Folder" buttons. The path is always returned even if the
/// folder doesn't exist yet (e.g. `mods/` on a vanilla server) - opening it
/// is left to the frontend, which surfaces that failure to the user rather
/// than this command silently guessing whether it "should" exist.
#[tauri::command]
pub async fn get_instance_subfolder(
    state: State<'_, AppState>,
    id: String,
    folder: String,
) -> Result<String, String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let server_dir = std::path::Path::new(&instance.server_directory).join("server");

    let path = match folder.as_str() {
        "server" => server_dir,
        "mods" => server_dir.join("mods"),
        "config" => server_dir.join("config"),
        "world" => server_dir.join(detect_world_folder_name(&server_dir).await),
        other => return Err(format!("Unknown folder \"{other}\"")),
    };

    Ok(path.to_string_lossy().to_string())
}

/// Reads `level-name` out of `server.properties` if present, otherwise
/// falls back to Minecraft's own default world folder name.
pub(crate) async fn detect_world_folder_name(server_dir: &std::path::Path) -> String {
    const DEFAULT: &str = "world";

    let Ok(contents) = tokio::fs::read_to_string(server_dir.join("server.properties")).await
    else {
        return DEFAULT.to_string();
    };

    contents
        .lines()
        .find_map(|line| line.strip_prefix("level-name="))
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| DEFAULT.to_string())
}

/// Copies an instance's `logs/latest.log` to a user-chosen location -
/// the equivalent crash-log export for a server that crashed, rather than
/// ModpackPilot itself.
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
