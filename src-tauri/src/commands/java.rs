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
        "SELECT {JAVA_COLUMNS} FROM java_installations ORDER BY version DESC"
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
/// No installations are ever removed here, even if not found this time -
/// a Java install on a removable/network drive shouldn't silently vanish
/// from an instance's settings just because it wasn't mounted during a scan.
#[tauri::command]
pub async fn detect_java_installations(state: State<'_, AppState>) -> Result<Vec<JavaInstallation>, String> {
    let found = tauri::async_runtime::spawn_blocking(java::detect_java_installations)
        .await
        .map_err(|e| format!("Java detection task failed: {e}"))?;

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
        .execute(&state.db)
        .await
        .map_err(|e| format!("Failed to save detected Java installation: {e}"))?;
    }

    list_java_installations(state).await
}

/// Marks one Java installation as the default ModForge suggests for new
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
