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

    if move_existing {
        let old_dir = state.paths.instances_dir.clone();
        let new_path = new_path.to_path_buf();
        if old_dir.is_dir() && old_dir != new_path {
            tauri::async_runtime::spawn_blocking(move || move_directory_contents(&old_dir, &new_path))
                .await
                .map_err(|e| format!("Move task failed: {e}"))?
                .map_err(|e| format!("Failed to move existing instances: {e}"))?;
        }
    }

    set_setting(&state.db, "instances_dir", &new_dir).await
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
                importer::copy_dir_recursive(&entry.path(), &target)?;
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
