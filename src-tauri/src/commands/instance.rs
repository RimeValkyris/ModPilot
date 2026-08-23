use std::path::{Path, PathBuf};

use chrono::Utc;
use tauri::State;
use uuid::Uuid;

use crate::filesystem::sanitize_dir_name;
use crate::models::{CreateInstanceRequest, Instance, InstanceRow, ServerLoader, ServerStatus};
use crate::AppState;

const INSTANCE_COLUMNS: &str = "id, name, minecraft_version, loader, loader_version, java_installation_id,
     min_ram_mb, max_ram_mb, server_directory, server_jar, jvm_args, server_args,
     status, auto_start, auto_restart, created_at, last_launched_at";

/// Lists every server instance ModForge knows about, newest first.
#[tauri::command]
pub async fn list_instances(state: State<'_, AppState>) -> Result<Vec<Instance>, String> {
    let rows = sqlx::query_as::<_, InstanceRow>(&format!(
        "SELECT {INSTANCE_COLUMNS} FROM instances ORDER BY created_at DESC"
    ))
    .fetch_all(&state.db)
    .await
    .map_err(|e| format!("Failed to load instances: {e}"))?;

    Ok(rows.into_iter().map(Instance::from).collect())
}

/// Creates a new server instance: a DB row plus its `server/` and `logs/`
/// directories under the configured instances directory.
///
/// Manual creation only (Phase 2) - importing existing server files (Phase 3)
/// will populate the directory from a ZIP/folder instead of starting empty.
#[tauri::command]
pub async fn create_instance(
    state: State<'_, AppState>,
    request: CreateInstanceRequest,
) -> Result<Instance, String> {
    let name = request.name.trim().to_string();
    if name.is_empty() {
        return Err("Instance name cannot be empty".to_string());
    }

    let min_ram_mb = request.min_ram_mb.unwrap_or(2048);
    let max_ram_mb = request.max_ram_mb.unwrap_or(4096);
    if min_ram_mb <= 0 || max_ram_mb <= 0 || min_ram_mb > max_ram_mb {
        return Err("Minimum RAM must be positive and not exceed maximum RAM".to_string());
    }

    let dir_name = sanitize_dir_name(&name);
    let server_directory = state.paths.instances_dir.join(&dir_name);

    // Never silently overwrite an existing instance's files.
    if server_directory.exists() {
        return Err(format!(
            "An instance folder named \"{dir_name}\" already exists. Choose a different name."
        ));
    }

    tokio::fs::create_dir_all(server_directory.join("server"))
        .await
        .map_err(|e| format!("Failed to create instance directory: {e}"))?;
    tokio::fs::create_dir_all(server_directory.join("logs"))
        .await
        .map_err(|e| format!("Failed to create logs directory: {e}"))?;

    let instance = Instance {
        id: Uuid::new_v4().to_string(),
        name,
        minecraft_version: request.minecraft_version,
        loader: request.loader.unwrap_or(ServerLoader::Unknown),
        loader_version: request.loader_version,
        java_installation_id: None,
        min_ram_mb,
        max_ram_mb,
        server_directory: server_directory.to_string_lossy().to_string(),
        server_jar: None,
        jvm_args: Vec::new(),
        server_args: Vec::new(),
        status: ServerStatus::Stopped,
        auto_start: false,
        auto_restart: false,
        created_at: Utc::now(),
        last_launched_at: None,
    };

    if let Err(e) = insert_instance(&state, &instance).await {
        // Roll back the directories we just created so a failed create
        // doesn't leave an orphaned, DB-less folder behind.
        let _ = tokio::fs::remove_dir_all(&server_directory).await;
        return Err(e);
    }

    if let Err(e) = write_instance_json(Path::new(&instance.server_directory), &instance).await {
        tracing::warn!("Instance {} created, but instance.json failed: {e}", instance.id);
    }

    Ok(instance)
}

/// Renames an instance's display name. The on-disk `server_directory` is
/// deliberately left unchanged - moving it could break a running process's
/// open file handles or a user's own external tooling pointed at the path.
#[tauri::command]
pub async fn rename_instance(
    state: State<'_, AppState>,
    id: String,
    new_name: String,
) -> Result<Instance, String> {
    let new_name = new_name.trim().to_string();
    if new_name.is_empty() {
        return Err("Instance name cannot be empty".to_string());
    }

    let result = sqlx::query("UPDATE instances SET name = ? WHERE id = ?")
        .bind(&new_name)
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| format!("Failed to rename instance: {e}"))?;

    if result.rows_affected() == 0 {
        return Err("Instance not found".to_string());
    }

    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    if let Err(e) = write_instance_json(Path::new(&instance.server_directory), &instance).await {
        tracing::warn!("Instance {id} renamed, but instance.json update failed: {e}");
    }

    Ok(instance)
}

/// Assigns (or clears, with `java_installation_id: None`) which detected
/// Java installation an instance launches with.
#[tauri::command]
pub async fn set_instance_java(
    state: State<'_, AppState>,
    id: String,
    java_installation_id: Option<String>,
) -> Result<Instance, String> {
    let result = sqlx::query("UPDATE instances SET java_installation_id = ? WHERE id = ?")
        .bind(&java_installation_id)
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| format!("Failed to set instance Java: {e}"))?;

    if result.rows_affected() == 0 {
        return Err("Instance not found".to_string());
    }

    fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())
}

/// Deletes an instance's DB record and its entire on-disk directory,
/// including the world, mods, and logs it contains.
///
/// The frontend must confirm this with the user first - this command does
/// not ask again and cannot be undone.
#[tauri::command]
pub async fn delete_instance(state: State<'_, AppState>, id: String) -> Result<(), String> {
    if state.processes.is_running(&id).await {
        return Err("Stop the instance before deleting it".to_string());
    }

    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    sqlx::query("DELETE FROM instances WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| format!("Failed to delete instance record: {e}"))?;

    let dir = PathBuf::from(&instance.server_directory);
    if dir.exists() {
        tokio::fs::remove_dir_all(&dir)
            .await
            .map_err(|e| format!("Instance record removed, but failed to delete its files: {e}"))?;
    }

    Ok(())
}

pub(crate) async fn fetch_instance(
    state: &State<'_, AppState>,
    id: &str,
) -> Result<Option<Instance>, String> {
    let row = sqlx::query_as::<_, InstanceRow>(&format!(
        "SELECT {INSTANCE_COLUMNS} FROM instances WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| format!("Failed to load instance: {e}"))?;

    Ok(row.map(Instance::from))
}

pub(crate) async fn insert_instance(
    state: &State<'_, AppState>,
    instance: &Instance,
) -> Result<(), String> {
    let jvm_args = serde_json::to_string(&instance.jvm_args).unwrap_or_else(|_| "[]".to_string());
    let server_args =
        serde_json::to_string(&instance.server_args).unwrap_or_else(|_| "[]".to_string());

    sqlx::query(
        "INSERT INTO instances (id, name, minecraft_version, loader, loader_version, java_installation_id,
                                 min_ram_mb, max_ram_mb, server_directory, server_jar, jvm_args, server_args,
                                 status, auto_start, auto_restart, created_at, last_launched_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&instance.id)
    .bind(&instance.name)
    .bind(&instance.minecraft_version)
    .bind(instance.loader.as_str())
    .bind(&instance.loader_version)
    .bind(&instance.java_installation_id)
    .bind(instance.min_ram_mb)
    .bind(instance.max_ram_mb)
    .bind(&instance.server_directory)
    .bind(&instance.server_jar)
    .bind(jvm_args)
    .bind(server_args)
    .bind(instance.status.as_str())
    .bind(instance.auto_start as i64)
    .bind(instance.auto_restart as i64)
    .bind(instance.created_at)
    .bind(instance.last_launched_at)
    .execute(&state.db)
    .await
    .map_err(|e| format!("Failed to save instance: {e}"))?;

    Ok(())
}

/// Writes a portable snapshot of the instance's metadata into its own
/// directory, per ModForge's `instances/<name>/instance.json` layout.
/// SQLite remains the source of truth the app reads from; this file exists
/// so an instance folder is self-describing if copied or inspected outside
/// the app.
pub(crate) async fn write_instance_json(
    server_directory: &Path,
    instance: &Instance,
) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(instance)
        .map_err(|e| std::io::Error::other(format!("Failed to serialize instance.json: {e}")))?;
    tokio::fs::write(server_directory.join("instance.json"), json).await
}
