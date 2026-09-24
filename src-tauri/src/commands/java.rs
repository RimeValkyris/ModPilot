use chrono::Utc;
use tauri::State;
use uuid::Uuid;

use crate::java;
use crate::models::{JavaInstallation, JavaInstallationRow};
use crate::AppState;

const JAVA_COLUMNS: &str = "id, version, vendor, path, architecture, is_default, detected_at";

/// Reads whatever Java installations are already known, without rescanning
/// the system. Used on page load so opening the Java page doesn't always
/// force a fresh `java -version` sweep.
#[tauri::command]
pub async fn list_java_installations(state: State<'_, AppState>) -> Result<Vec<JavaInstallation>, String> {
    let rows = sqlx::query_as::<_, JavaInstallationRow>(&format!(
        "SELECT {JAVA_COLUMNS} FROM java_installations
         ORDER BY is_default DESC, detected_at DESC, version DESC"
    ))
    .fetch_all(&state.db)
    .await
    .map_err(|e| format!("Failed to load Java installations: {e}"))?;

    Ok(rows.into_iter().map(JavaInstallation::from).collect())
}

/// Rescans the system for Java installations and upserts each one found
/// (keyed by executable path) into the database, then returns the full,
/// refreshed list.
///
/// A "not found this time" installation is otherwise never removed here -
/// a Java install on a removable/network drive shouldn't silently vanish
/// from an instance's settings just because it wasn't mounted during a
/// scan. `prune_stale_installations` below is the one exception: it clears
/// out rows that are provably never coming back, not just "not found
/// right now".
#[tauri::command]
pub async fn detect_java_installations(state: State<'_, AppState>) -> Result<Vec<JavaInstallation>, String> {
    refresh_java_installations(&state.db).await?;
    list_java_installations(state).await
}

/// Scans the host and persists the discovered installations for workflows
/// that need Java resolution without going through the Java page first.
pub(crate) async fn refresh_java_installations(db: &sqlx::SqlitePool) -> Result<(), String> {
    let found = tauri::async_runtime::spawn_blocking(java::detect_java_installations)
        .await
        .map_err(|e| format!("Java detection task failed: {e}"))?;

    save_detected_java_installations(db, found).await
}

pub(crate) async fn save_detected_java_installations(
    db: &sqlx::SqlitePool,
    found: Vec<crate::models::DetectedJava>,
) -> Result<(), String> {
    for detected in found {
        sqlx::query(
            "INSERT INTO java_installations (id, version, vendor, path, architecture, is_default, detected_at)
             VALUES (?, ?, ?, ?, ?, 0, ?)
             ON CONFLICT(path) DO UPDATE SET
                version = excluded.version,
                vendor = excluded.vendor,
                architecture = excluded.architecture,
                detected_at = excluded.detected_at",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&detected.version)
        .bind(&detected.vendor)
        .bind(&detected.path)
        .bind(&detected.architecture)
        .bind(Utc::now())
        .execute(db)
        .await
        .map_err(|e| format!("Failed to save detected Java installation: {e}"))?;
    }

    prune_stale_installations(db).await
}

/// Removes DB rows for Java installations that are provably invalid: the
/// path no longer points to a real file, or it's an Oracle `javapath`
/// redirector (see `is_oracle_path_redirector`) - a hard-linked duplicate
/// of a real install, not a distinct one. Both can only ever be leftovers
/// from before this filter existed, or from an actual uninstall, so unlike
/// a removable drive just not being mounted right now, these are never
/// coming back - it's safe to clear them automatically instead of making
/// every affected user find Settings -> Danger Zone -> "Forget all Java
/// installations" themselves.
async fn prune_stale_installations(db: &sqlx::SqlitePool) -> Result<(), String> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT id, path FROM java_installations")
        .fetch_all(db)
        .await
        .map_err(|e| format!("Failed to read Java installations: {e}"))?;

    for (id, path) in rows {
        let is_stale = java::is_oracle_path_redirector(std::path::Path::new(&path))
            || !std::path::Path::new(&path).is_file();
        if is_stale {
            sqlx::query("DELETE FROM java_installations WHERE id = ?")
                .bind(&id)
                .execute(db)
                .await
                .map_err(|e| format!("Failed to remove stale Java installation: {e}"))?;
        }
    }
    Ok(())
}

/// Marks one Java installation as the default ModpackPilot suggests for new
/// instances. Purely a UI convenience - it doesn't change any existing
/// instance's `java_installation_id`.
#[tauri::command]
pub async fn set_default_java(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| format!("Failed to start transaction: {e}"))?;

    sqlx::query("UPDATE java_installations SET is_default = 0")
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Failed to clear previous default: {e}"))?;

    let result = sqlx::query("UPDATE java_installations SET is_default = 1 WHERE id = ?")
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Failed to set default Java: {e}"))?;

    if result.rows_affected() == 0 {
        return Err("Java installation not found".to_string());
    }

    tx.commit()
        .await
        .map_err(|e| format!("Failed to save default Java: {e}"))?;

    Ok(())
}

/// Forgets every detected Java installation. Any instance referencing one
/// has its `java_installation_id` cleared automatically (the `ON DELETE
/// SET NULL` foreign key from the initial migration) - nothing about the
/// instance itself is touched, it just needs Java re-assigned afterward.
#[tauri::command]
pub async fn reset_java_installations(state: State<'_, AppState>) -> Result<(), String> {
    sqlx::query("DELETE FROM java_installations")
        .execute(&state.db)
        .await
        .map_err(|e| format!("Failed to reset Java installations: {e}"))?;
    Ok(())
}
