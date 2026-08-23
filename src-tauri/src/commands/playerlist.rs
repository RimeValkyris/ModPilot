use std::path::Path;

use serde_json::Value;
use tauri::State;

use super::instance::fetch_instance;
use crate::AppState;

/// The only filenames this command is allowed to touch - Minecraft's own
/// per-player list files. `file` is a plain string over IPC, so this is
/// what actually prevents it from being pointed at an arbitrary path.
const ALLOWED_FILES: &[&str] = &["whitelist.json", "ops.json", "banned-players.json"];

fn validate_file(file: &str) -> Result<(), String> {
    if ALLOWED_FILES.contains(&file) {
        Ok(())
    } else {
        Err(format!("Unsupported player list file \"{file}\""))
    }
}

/// Reads one of an instance's player-list JSON files (whitelist, ops, or
/// banned-players), returning an empty array if the file doesn't exist yet -
/// a fresh server hasn't necessarily created any of these.
#[tauri::command]
pub async fn read_player_list(state: State<'_, AppState>, id: String, file: String) -> Result<Value, String> {
    validate_file(&file)?;
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let path = Path::new(&instance.server_directory).join("server").join(&file);
    match tokio::fs::read_to_string(&path).await {
        Ok(contents) => {
            serde_json::from_str(&contents).map_err(|e| format!("Failed to parse {file}: {e}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Array(Vec::new())),
        Err(e) => Err(format!("Failed to read {file}: {e}")),
    }
}

/// Overwrites one of an instance's player-list JSON files with `entries`
/// (expected to be a JSON array, validated by the caller's own schema).
///
/// Editing these while the server is running works the same as editing
/// them by hand always has - most changes only take effect on the next
/// start, or after the corresponding in-game `/reload`.
#[tauri::command]
pub async fn write_player_list(
    state: State<'_, AppState>,
    id: String,
    file: String,
    entries: Value,
) -> Result<(), String> {
    validate_file(&file)?;
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let path = Path::new(&instance.server_directory).join("server").join(&file);
    let json = serde_json::to_string_pretty(&entries)
        .map_err(|e| format!("Failed to serialize {file}: {e}"))?;

    tokio::fs::write(&path, json)
        .await
        .map_err(|e| format!("Failed to write {file}: {e}"))?;

    Ok(())
}
