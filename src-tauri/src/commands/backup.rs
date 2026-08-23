use std::path::Path;

use chrono::Utc;
use tauri::State;

use super::diagnostics::detect_world_folder_name;
use super::instance::fetch_instance;
use crate::importer;
use crate::models::WorldBackup;
use crate::AppState;

/// Rejects a backup filename that isn't exactly what `list_world_backups`
/// would have produced - prevents a crafted name (e.g. containing `..` or a
/// path separator) from making `restore`/`delete` touch anything outside
/// the instance's own `backups/` folder.
fn validate_backup_name(name: &str) -> Result<(), String> {
    let safe = !name.is_empty()
        && name.ends_with(".zip")
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains("..");
    if safe {
        Ok(())
    } else {
        Err("Invalid backup name".to_string())
    }
}

async fn backups_dir_for(state: &State<'_, AppState>, id: &str) -> Result<std::path::PathBuf, String> {
    let instance = fetch_instance(state, id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;
    Ok(Path::new(&instance.server_directory).join("backups"))
}

async fn world_dir_for(state: &State<'_, AppState>, id: &str) -> Result<std::path::PathBuf, String> {
    let instance = fetch_instance(state, id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;
    let server_dir = Path::new(&instance.server_directory).join("server");
    Ok(server_dir.join(detect_world_folder_name(&server_dir).await))
}

/// Zips the instance's current world folder into `backups/<world>-<timestamp>.zip`.
#[tauri::command]
pub async fn create_world_backup(state: State<'_, AppState>, id: String) -> Result<String, String> {
    let world_dir = world_dir_for(&state, &id).await?;
    if !world_dir.is_dir() {
        return Err("This instance doesn't have a world folder yet".to_string());
    }

    let backups_dir = backups_dir_for(&state, &id).await?;
    tokio::fs::create_dir_all(&backups_dir)
        .await
        .map_err(|e| format!("Failed to create backups folder: {e}"))?;

    let world_name = world_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "world".to_string());
    let file_name = format!("{world_name}-{}.zip", Utc::now().format("%Y%m%d-%H%M%S"));
    let dest = backups_dir.join(&file_name);

    tauri::async_runtime::spawn_blocking(move || importer::create_zip_from_dir(&world_dir, &dest))
        .await
        .map_err(|e| format!("Backup task failed: {e}"))?
        .map_err(|e| format!("Failed to create backup: {e}"))?;

    Ok(file_name)
}

/// Lists an instance's saved world backups, newest first.
#[tauri::command]
pub async fn list_world_backups(state: State<'_, AppState>, id: String) -> Result<Vec<WorldBackup>, String> {
    let backups_dir = backups_dir_for(&state, &id).await?;

    let mut entries = match tokio::fs::read_dir(&backups_dir).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("Failed to read backups folder: {e}")),
    };

    let mut backups = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| format!("Failed to read backups folder: {e}"))?
    {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".zip") {
            continue;
        }
        let Ok(metadata) = entry.metadata().await else {
            continue;
        };
        let created_at = metadata
            .modified()
            .ok()
            .map(chrono::DateTime::<Utc>::from)
            .unwrap_or_else(Utc::now);

        backups.push(WorldBackup {
            name,
            size_bytes: metadata.len(),
            created_at,
        });
    }

    backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(backups)
}

/// Replaces the instance's current world folder with the contents of a
/// backup. Refuses to run while the instance is started - restoring out
/// from under a live server would corrupt it - and the frontend must
/// confirm this with the user first, since it overwrites the current world.
#[tauri::command]
pub async fn restore_world_backup(
    state: State<'_, AppState>,
    id: String,
    backup_name: String,
) -> Result<(), String> {
    validate_backup_name(&backup_name)?;

    if state.processes.is_running(&id).await {
        return Err("Stop the instance before restoring a backup".to_string());
    }

    let backup_path = backups_dir_for(&state, &id).await?.join(&backup_name);
    if !backup_path.is_file() {
        return Err("Backup not found".to_string());
    }
    let world_dir = world_dir_for(&state, &id).await?;

    tauri::async_runtime::spawn_blocking(move || {
        if world_dir.exists() {
            std::fs::remove_dir_all(&world_dir)?;
        }
        std::fs::create_dir_all(&world_dir)?;
        importer::extract_zip_safely(&backup_path, &world_dir).map(|_warnings| ())
    })
    .await
    .map_err(|e| format!("Restore task failed: {e}"))?
    .map_err(|e| format!("Failed to restore backup: {e}"))?;

    Ok(())
}

/// Permanently deletes a saved backup.
#[tauri::command]
pub async fn delete_world_backup(
    state: State<'_, AppState>,
    id: String,
    backup_name: String,
) -> Result<(), String> {
    validate_backup_name(&backup_name)?;

    let backup_path = backups_dir_for(&state, &id).await?.join(&backup_name);
    tokio::fs::remove_file(&backup_path)
        .await
        .map_err(|e| format!("Failed to delete backup: {e}"))?;

    Ok(())
}
