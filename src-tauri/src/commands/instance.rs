use std::path::{Path, PathBuf};

use chrono::Utc;
use tauri::State;
use uuid::Uuid;

use super::settings::get_setting;
use crate::filesystem::sanitize_dir_name;
use crate::importer;
use crate::models::{
    CreateInstanceRequest, Instance, InstanceRow, ServerLoader, ServerStatus,
    UpdateInstanceSettingsRequest,
};
use crate::AppState;

const INSTANCE_COLUMNS: &str = "id, name, minecraft_version, loader, loader_version, java_installation_id,
     min_ram_mb, max_ram_mb, server_directory, server_jar, launch_mode, jvm_args, server_args,
     status, auto_start, auto_restart, created_at, last_launched_at,
     modrinth_project_id, modrinth_project_title, modrinth_version_id,
     ftb_pack_id, ftb_pack_name, ftb_version_id,
     restart_schedule, backup_schedule, backup_keep_last, update_policy";

/// Lists every server instance ModpackPilot knows about, newest first.
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

    let default_min_ram = get_setting(&state.db, "default_min_ram_mb")
        .await?
        .and_then(|v| v.parse().ok())
        .unwrap_or(2048);
    let default_max_ram = get_setting(&state.db, "default_max_ram_mb")
        .await?
        .and_then(|v| v.parse().ok())
        .unwrap_or(4096);
    let min_ram_mb = request.min_ram_mb.unwrap_or(default_min_ram);
    let max_ram_mb = request.max_ram_mb.unwrap_or(default_max_ram);
    if min_ram_mb <= 0 || max_ram_mb <= 0 || min_ram_mb > max_ram_mb {
        return Err("Minimum RAM must be positive and not exceed maximum RAM".to_string());
    }

    let jvm_args: Vec<String> = get_setting(&state.db, "default_jvm_args")
        .await?
        .map(|v| v.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
        .unwrap_or_default();

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
        launch_mode: "jar".to_string(),
        jvm_args,
        server_args: Vec::new(),
        status: ServerStatus::Stopped,
        auto_start: false,
        auto_restart: false,
        created_at: Utc::now(),
        last_launched_at: None,
        modrinth_project_id: None,
        modrinth_project_title: None,
        modrinth_version_id: None,
        ftb_pack_id: None,
        ftb_pack_name: None,
        ftb_version_id: None,
        restart_schedule: None,
        backup_schedule: None,
        backup_keep_last: 0,
        update_policy: "off".to_string(),
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

/// Creates a new instance by copying an existing one's server files
/// (mods, config, world, everything under `server/`) and settings -
/// logs and backups are deliberately not copied, so the clone starts clean.
#[tauri::command]
pub async fn duplicate_instance(
    state: State<'_, AppState>,
    id: String,
    new_name: String,
) -> Result<Instance, String> {
    let source = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let new_name = new_name.trim().to_string();
    if new_name.is_empty() {
        return Err("Instance name cannot be empty".to_string());
    }

    let dir_name = sanitize_dir_name(&new_name);
    let new_dir = state.paths.instances_dir.join(&dir_name);
    if new_dir.exists() {
        return Err(format!(
            "An instance folder named \"{dir_name}\" already exists. Choose a different name."
        ));
    }

    let source_server_dir = Path::new(&source.server_directory).join("server");
    let new_server_dir = new_dir.join("server");
    tokio::fs::create_dir_all(&new_server_dir)
        .await
        .map_err(|e| format!("Failed to create instance directory: {e}"))?;
    tokio::fs::create_dir_all(new_dir.join("logs"))
        .await
        .map_err(|e| format!("Failed to create logs directory: {e}"))?;

    {
        let source_server_dir = source_server_dir.clone();
        let new_server_dir = new_server_dir.clone();
        tauri::async_runtime::spawn_blocking(move || {
            importer::copy_dir_recursive(&source_server_dir, &new_server_dir)
        })
        .await
        .map_err(|e| format!("Copy task failed: {e}"))?
        .map_err(|e| format!("Failed to copy server files: {e}"))?;
    }

    let instance = Instance {
        id: Uuid::new_v4().to_string(),
        name: new_name,
        minecraft_version: source.minecraft_version.clone(),
        loader: source.loader,
        loader_version: source.loader_version.clone(),
        java_installation_id: source.java_installation_id.clone(),
        min_ram_mb: source.min_ram_mb,
        max_ram_mb: source.max_ram_mb,
        server_directory: new_dir.to_string_lossy().to_string(),
        server_jar: source.server_jar.clone(),
        launch_mode: source.launch_mode.clone(),
        jvm_args: source.jvm_args.clone(),
        server_args: source.server_args.clone(),
        status: ServerStatus::Stopped,
        // Deliberately not carried over: a clone auto-starting alongside
        // its source the next time ModpackPilot opens would be surprising.
        auto_start: false,
        auto_restart: source.auto_restart,
        created_at: Utc::now(),
        last_launched_at: None,
        // Carried over deliberately, unlike auto_start above: the clone is
        // still the same modpack at the same version, so it should still
        // be checkable/updatable against the same Modrinth project.
        modrinth_project_id: source.modrinth_project_id.clone(),
        modrinth_project_title: source.modrinth_project_title.clone(),
        modrinth_version_id: source.modrinth_version_id.clone(),
        // Same reasoning for FTB: a clone is the same pack at the same
        // version, so it stays updatable against it.
        ftb_pack_id: source.ftb_pack_id,
        ftb_pack_name: source.ftb_pack_name.clone(),
        ftb_version_id: source.ftb_version_id,
        restart_schedule: source.restart_schedule.clone(),
        backup_schedule: source.backup_schedule.clone(),
        backup_keep_last: source.backup_keep_last,
        update_policy: source.update_policy.clone(),
    };

    if let Err(e) = insert_instance(&state, &instance).await {
        let _ = tokio::fs::remove_dir_all(&new_dir).await;
        return Err(e);
    }

    if let Err(e) = write_instance_json(&new_dir, &instance).await {
        tracing::warn!("Instance {} duplicated, but instance.json failed: {e}", instance.id);
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

/// Lists `.jar` files directly under an instance's `server/` folder, so the
/// Configuration tab can offer a picker instead of a free-text path.
#[tauri::command]
pub async fn list_server_jars(state: State<'_, AppState>, id: String) -> Result<Vec<String>, String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let server_dir = Path::new(&instance.server_directory).join("server");
    let mut entries = tokio::fs::read_dir(&server_dir)
        .await
        .map_err(|e| format!("Failed to read server directory: {e}"))?;

    let mut jars = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| format!("Failed to read server directory: {e}"))?
    {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.to_lowercase().ends_with(".jar") {
            jars.push(name);
        }
    }
    jars.sort();
    Ok(jars)
}

/// Updates the editable launch settings (Phase 7 Configuration tab): JAR,
/// JVM/server arguments, RAM, and auto-start/auto-restart. Java executable
/// selection has its own command (`set_instance_java`).
#[tauri::command]
pub async fn update_instance_settings(
    state: State<'_, AppState>,
    id: String,
    request: UpdateInstanceSettingsRequest,
) -> Result<Instance, String> {
    if request.min_ram_mb <= 0 || request.max_ram_mb <= 0 || request.min_ram_mb > request.max_ram_mb {
        return Err("Minimum RAM must be positive and not exceed maximum RAM".to_string());
    }

    let jvm_args = serde_json::to_string(&request.jvm_args).unwrap_or_else(|_| "[]".to_string());
    let server_args = serde_json::to_string(&request.server_args).unwrap_or_else(|_| "[]".to_string());

    // This form only ever offers a `.jar` to pick (see `list_server_jars`),
    // so a manual edit here always means "launch this as a plain jar" -
    // even if detection had previously set `launch_mode = 'argfile'` for a
    // modern Forge/NeoForge install.
    let result = sqlx::query(
        "UPDATE instances
         SET server_jar = ?, launch_mode = 'jar', jvm_args = ?, server_args = ?, min_ram_mb = ?, max_ram_mb = ?,
             auto_start = ?, auto_restart = ?
         WHERE id = ?",
    )
    .bind(&request.server_jar)
    .bind(jvm_args)
    .bind(server_args)
    .bind(request.min_ram_mb)
    .bind(request.max_ram_mb)
    .bind(request.auto_start as i64)
    .bind(request.auto_restart as i64)
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(|e| format!("Failed to update instance settings: {e}"))?;

    if result.rows_affected() == 0 {
        return Err("Instance not found".to_string());
    }

    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    if let Err(e) = write_instance_json(Path::new(&instance.server_directory), &instance).await {
        tracing::warn!("Instance {id} settings updated, but instance.json update failed: {e}");
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
                                 min_ram_mb, max_ram_mb, server_directory, server_jar, launch_mode, jvm_args, server_args,
                                 status, auto_start, auto_restart, created_at, last_launched_at,
                                 modrinth_project_id, modrinth_project_title, modrinth_version_id,
                                 ftb_pack_id, ftb_pack_name, ftb_version_id,
                                 restart_schedule, backup_schedule, backup_keep_last, update_policy)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
    .bind(&instance.launch_mode)
    .bind(jvm_args)
    .bind(server_args)
    .bind(instance.status.as_str())
    .bind(instance.auto_start as i64)
    .bind(instance.auto_restart as i64)
    .bind(instance.created_at)
    .bind(instance.last_launched_at)
    .bind(&instance.modrinth_project_id)
    .bind(&instance.modrinth_project_title)
    .bind(&instance.modrinth_version_id)
    .bind(instance.ftb_pack_id)
    .bind(&instance.ftb_pack_name)
    .bind(instance.ftb_version_id)
    .bind(&instance.restart_schedule)
    .bind(&instance.backup_schedule)
    .bind(instance.backup_keep_last)
    .bind(&instance.update_policy)
    .execute(&state.db)
    .await
    .map_err(|e| format!("Failed to save instance: {e}"))?;

    Ok(())
}

/// Writes a portable snapshot of the instance's metadata into its own
/// directory, per ModpackPilot's `instances/<name>/instance.json` layout.
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

/// Saves an instance's automation schedules. Validates the schedule strings
/// here rather than trusting the frontend, so a malformed value can never
/// reach the scheduler and silently never fire.
#[tauri::command]
pub async fn set_instance_schedules(
    state: State<'_, AppState>,
    id: String,
    restart_schedule: Option<String>,
    backup_schedule: Option<String>,
    backup_keep_last: i64,
) -> Result<Instance, String> {
    for (label, raw) in [("restart", &restart_schedule), ("backup", &backup_schedule)] {
        if let Some(raw) = raw.as_deref().filter(|r| !r.trim().is_empty()) {
            if crate::server::Schedule::parse(raw).is_none() {
                return Err(format!("Invalid {label} schedule: \"{raw}\""));
            }
        }
    }
    if backup_keep_last < 0 {
        return Err("Backup retention cannot be negative".to_string());
    }

    // Empty string and None both mean "disabled" - normalize so the
    // scheduler only ever has to check for NULL.
    let restart = restart_schedule.filter(|r| !r.trim().is_empty());
    let backup = backup_schedule.filter(|r| !r.trim().is_empty());

    let result = sqlx::query(
        "UPDATE instances SET restart_schedule = ?, backup_schedule = ?, backup_keep_last = ? WHERE id = ?",
    )
    .bind(&restart)
    .bind(&backup)
    .bind(backup_keep_last)
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(|e| format!("Failed to save schedules: {e}"))?;

    if result.rows_affected() == 0 {
        return Err("Instance not found".to_string());
    }

    fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())
}
