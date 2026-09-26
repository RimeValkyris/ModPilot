use sqlx::SqlitePool;
use tauri::{AppHandle, State};

use crate::importer;
use crate::AppState;

/// Generic key-value app settings (theme, and whatever else needs a
/// persisted preference that isn't tied to a specific instance). Backed by
/// the `application_settings` table from the very first migration.
#[tauri::command]
pub async fn get_app_setting(state: State<'_, AppState>, key: String) -> Result<Option<String>, String> {
    get_setting(&state.db, &key).await
}

#[tauri::command]
pub async fn set_app_setting(state: State<'_, AppState>, key: String, value: String) -> Result<(), String> {
    set_setting(&state.db, &key, &value).await
}

pub(crate) async fn get_setting(db: &SqlitePool, key: &str) -> Result<Option<String>, String> {
    sqlx::query_scalar("SELECT value FROM application_settings WHERE key = ?")
        .bind(key)
        .fetch_optional(db)
        .await
        .map_err(|e| format!("Failed to read setting: {e}"))
}

pub(crate) async fn set_setting(db: &SqlitePool, key: &str, value: &str) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO application_settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(db)
    .await
    .map_err(|e| format!("Failed to save setting: {e}"))?;
    Ok(())
}

/// Convenience for a settings row that's really just an on/off flag,
/// stored as the string "true"/"false".
pub(crate) async fn get_setting_bool(db: &SqlitePool, key: &str, default: bool) -> bool {
    match get_setting(db, key).await {
        Ok(Some(v)) => v == "true",
        _ => default,
    }
}

/// Returns the instances directory ModpackPilot is currently configured to
/// use (which may be a user override from `set_instances_dir`, applied at
/// the last app startup).
#[tauri::command]
pub fn get_instances_dir(state: State<'_, AppState>) -> String {
    state.paths.instances_dir.to_string_lossy().to_string()
}

/// An `instances` folder beside ModpackPilot's own executable - "portable"
/// mode, keeping server files with the app instead of in the per-user
/// app-data directory.
///
/// Only a suggestion: whether it's actually usable depends on where the app
/// was installed, which `set_instances_dir`'s writability probe is what
/// really decides. A default NSIS/MSI install lands in Program Files, which
/// a standard user can't write to.
#[tauri::command]
pub fn get_portable_instances_dir() -> Result<String, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("Couldn't locate the ModpackPilot executable: {e}"))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "Couldn't determine ModpackPilot's own folder".to_string())?;
    Ok(dir.join("instances").to_string_lossy().to_string())
}

/// Records a new instances directory and, if requested, moves everything
/// already there into it. Takes effect on the next launch - `AppState`'s
/// paths are fixed for the process lifetime, so this deliberately doesn't
/// try to relocate a live, in-use directory out from under running state.
#[tauri::command]
pub async fn set_instances_dir(
    state: State<'_, AppState>,
    new_dir: String,
    move_existing: bool,
) -> Result<(), String> {
    let new_path = std::path::Path::new(&new_dir);
    tokio::fs::create_dir_all(new_path)
        .await
        .map_err(|e| format!("Failed to create directory: {e}"))?;

    // Creating the directory isn't proof it can be written into - a
    // Program Files subfolder is the common case, and on Windows that can
    // fail (or get UAC-virtualized elsewhere) only once something actually
    // writes. Finding that out midway through copying a multi-gigabyte
    // modpack is far worse than finding out now.
    let probe = new_path.join(".modpackpilot-write-test");
    tokio::fs::write(&probe, b"")
        .await
        .map_err(|_| {
            format!(
                "ModpackPilot can't write to \"{new_dir}\". If that's inside Program Files, \
                 Windows blocks normal apps from writing there - pick a folder in your user \
                 directory (Documents, Desktop) instead."
            )
        })?;
    let _ = tokio::fs::remove_file(&probe).await;

    if move_existing {
        let old_dir = state.paths.instances_dir.clone();
        let new_path = new_path.to_path_buf();
        let same = match (old_dir.canonicalize(), new_path.canonicalize()) {
            (Ok(old), Ok(new)) => {
                // Moving a folder into its own subfolder (or the reverse)
                // would have the move chase its own output.
                if old != new && (new.starts_with(&old) || old.starts_with(&new)) {
                    return Err(
                        "The new folder can't be inside the current instances folder, or contain it."
                            .to_string(),
                    );
                }
                old == new
            }
            _ => old_dir == new_path,
        };
        if old_dir.is_dir() && !same {
            // A running server has its files open; moving them out from
            // under it fails partway on Windows and corrupts a world
            // elsewhere.
            if state.processes.any_running().await {
                return Err("Stop all running servers before moving instances.".to_string());
            }

            let (from, to) = (old_dir.clone(), new_path.clone());
            let moved = tauri::async_runtime::spawn_blocking(move || move_directory_contents(&from, &to))
                .await
                .map_err(|e| format!("Move task failed: {e}"));

            // Every instance row stores its absolute folder, so without
            // this they all keep pointing at the now-empty old location.
            // Runs even after a partial failure, so whatever did move is
            // still found.
            repoint_instance_directories(&state.db, &old_dir, &new_path).await?;
            moved?.map_err(|e| format!("Failed to move existing instances: {e}"))?;
        }
    }

    set_setting(&state.db, "instances_dir", &new_dir).await
}

/// Rewrites `server_directory` for every instance that lived under
/// `old_dir` and now exists at the same relative location under `new_dir`.
/// Instances kept somewhere else entirely, or that a failed move left
/// behind, are left alone.
async fn repoint_instance_directories(
    db: &SqlitePool,
    old_dir: &std::path::Path,
    new_dir: &std::path::Path,
) -> Result<(), String> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT id, server_directory FROM instances")
        .fetch_all(db)
        .await
        .map_err(|e| format!("Moved the instances, but couldn't update their locations: {e}"))?;

    for (id, directory) in rows {
        let Ok(relative) = std::path::Path::new(&directory).strip_prefix(old_dir) else {
            continue;
        };
        let moved = new_dir.join(relative);
        if !moved.is_dir() || std::path::Path::new(&directory).is_dir() {
            continue;
        }
        let moved = moved.to_string_lossy().to_string();
        sqlx::query("UPDATE instances SET server_directory = ? WHERE id = ?")
            .bind(&moved)
            .bind(&id)
            .execute(db)
            .await
            .map_err(|e| format!("Moved the instances, but couldn't update their locations: {e}"))?;
    }
    Ok(())
}

/// Moves every top-level entry from `src` into `dest`. Tries a plain rename
/// first (instant, same-drive); falls back to copy-then-delete for a move
/// across drives, which `fs::rename` can't do on Windows.
fn move_directory_contents(src: &std::path::Path, dest: &std::path::Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dest.join(entry.file_name());
        if std::fs::rename(entry.path(), &target).is_err() {
            if entry.file_type()?.is_dir() {
                importer::copy_dir_verbatim(&entry.path(), &target)?;
                std::fs::remove_dir_all(entry.path())?;
            } else {
                std::fs::copy(entry.path(), &target)?;
                std::fs::remove_file(entry.path())?;
            }
        }
    }
    Ok(())
}

/// Exits ModpackPilot immediately. Used by the close-guard once the user has
/// confirmed stopping any running servers (or there were none to stop).
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}
