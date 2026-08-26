use std::path::{Path, PathBuf};

use chrono::Utc;
use tauri::State;
use uuid::Uuid;

use super::instance::{insert_instance, write_instance_json};
use crate::filesystem::sanitize_dir_name;
use crate::importer;
use crate::models::{DetectedServerInfo, ImportInstanceRequest, ImportSource, Instance, ServerStatus};
use crate::AppState;

/// Marker prefix the frontend checks for to distinguish "an instance with
/// this name already exists" from any other failure, so it can offer an
/// explicit overwrite confirmation instead of just showing a generic error.
pub const INSTANCE_EXISTS_PREFIX: &str = "INSTANCE_EXISTS:";

/// Reads a ZIP or folder's contents and reports what it looks like, without
/// copying or extracting anything. This is wizard Step 3/4 - the user
/// reviews this before anything touches the instances directory.
#[tauri::command]
pub async fn analyze_import(source: ImportSource) -> Result<DetectedServerInfo, String> {
    tauri::async_runtime::spawn_blocking(move || match source {
        ImportSource::Zip { path } => {
            let path = PathBuf::from(path);
            importer::detect_from_zip(&path).map_err(|e| format!("Failed to read ZIP file: {e}"))
        }
        ImportSource::Folder { path } => {
            let path = PathBuf::from(path);
            if !path.is_dir() {
                return Err("Selected path is not a folder".to_string());
            }
            Ok(importer::detect_from_dir(&path))
        }
    })
    .await
    .map_err(|e| format!("Import analysis task failed: {e}"))?
}

/// Copies/extracts the source into a new instance directory and registers
/// it, using `analyze_import`'s detection (re-run against the real copied
/// files) merged with whatever the user overrode during review.
#[tauri::command]
pub async fn import_instance(
    state: State<'_, AppState>,
    source: ImportSource,
    request: ImportInstanceRequest,
) -> Result<Instance, String> {
    let name = request.name.trim().to_string();
    if name.is_empty() {
        return Err("Instance name cannot be empty".to_string());
    }

    let dir_name = sanitize_dir_name(&name);
    let instance_dir = state.paths.instances_dir.join(&dir_name);

    if instance_dir.exists() {
        if !request.overwrite {
            // Never overwrite without the user explicitly confirming first.
            return Err(format!("{INSTANCE_EXISTS_PREFIX}{dir_name}"));
        }
        tokio::fs::remove_dir_all(&instance_dir)
            .await
            .map_err(|e| format!("Failed to remove existing instance folder: {e}"))?;
    }

    let server_dir = instance_dir.join("server");
    tokio::fs::create_dir_all(&server_dir)
        .await
        .map_err(|e| format!("Failed to create instance directory: {e}"))?;
    tokio::fs::create_dir_all(instance_dir.join("logs"))
        .await
        .map_err(|e| format!("Failed to create logs directory: {e}"))?;

    if let Err(e) = copy_source_into(source, server_dir.clone()).await {
        let _ = tokio::fs::remove_dir_all(&instance_dir).await;
        return Err(e);
    }

    let detected = {
        let server_dir = server_dir.clone();
        tauri::async_runtime::spawn_blocking(move || importer::detect_from_dir(&server_dir))
            .await
            .map_err(|e| format!("Detection task failed: {e}"))?
    };

    // Force ModpackPilot's own sane defaults onto the imported pack (most
    // importantly: whitelist off, so the pack's own `white-list=true`
    // doesn't lock everyone out on first boot). Best-effort - a pack with
    // no server.properties yet is fine, and a failure here shouldn't sink
    // an otherwise-successful import.
    if let Err(e) = super::server_properties::apply_import_defaults(&server_dir).await {
        tracing::warn!("Couldn't apply default server properties on import: {e}");
    }

    let min_ram_mb = request.min_ram_mb.unwrap_or(2048);
    let max_ram_mb = request.max_ram_mb.unwrap_or(4096);
    if min_ram_mb <= 0 || max_ram_mb <= 0 || min_ram_mb > max_ram_mb {
        let _ = tokio::fs::remove_dir_all(&instance_dir).await;
        return Err("Minimum RAM must be positive and not exceed maximum RAM".to_string());
    }

    let instance = Instance {
        id: Uuid::new_v4().to_string(),
        name,
        minecraft_version: request.minecraft_version.or(detected.minecraft_version),
        loader: request.loader.unwrap_or(detected.loader),
        loader_version: request.loader_version.or(detected.loader_version),
        java_installation_id: None,
        min_ram_mb,
        max_ram_mb,
        server_directory: instance_dir.to_string_lossy().to_string(),
        server_jar: detected.server_jar,
        launch_mode: if detected.server_jar_is_argfile { "argfile" } else { "jar" }.to_string(),
        jvm_args: Vec::new(),
        server_args: Vec::new(),
        status: ServerStatus::Stopped,
        auto_start: false,
        auto_restart: false,
        created_at: Utc::now(),
        last_launched_at: None,
        modrinth_project_id: None,
        modrinth_project_title: None,
        modrinth_version_id: None,
        restart_schedule: None,
        backup_schedule: None,
        backup_keep_last: 0,
        update_policy: "off".to_string(),
    };

    if let Err(e) = insert_instance(&state, &instance).await {
        let _ = tokio::fs::remove_dir_all(&instance_dir).await;
        return Err(e);
    }

    if let Err(e) = write_instance_json(&instance_dir, &instance).await {
        tracing::warn!("Instance {} imported, but instance.json failed: {e}", instance.id);
    }

    Ok(instance)
}

/// Updates an existing instance in place from a local ZIP or folder, for
/// packs that aren't on Modrinth (CurseForge-only, private, or homemade)
/// and so have no update source to check against.
///
/// This is the manual counterpart to `apply_modpack_update`, and it keeps
/// the same guarantees: the world, `server.properties`, whitelist/ops/bans
/// and `eula.txt` are never overwritten, files the previous pack installed
/// but this one doesn't are cleaned up, and anything added by hand is left
/// alone (see `crate::packs`).
///
/// A world backup is taken first and the instance must be stopped -
/// replacing mod jars under a live server corrupts it, and a pack update
/// can leave an existing world unloadable.
#[tauri::command]
pub async fn update_instance_from_source(
    state: State<'_, AppState>,
    id: String,
    source: ImportSource,
) -> Result<Instance, String> {
    if state.processes.is_running(&id).await {
        return Err("Stop the instance before updating it from a file".to_string());
    }

    let instance = super::instance::fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let instance_dir = PathBuf::from(&instance.server_directory);
    let server_dir = instance_dir.join("server");
    let world_folder_name = super::diagnostics::detect_world_folder_name(&server_dir).await;

    // Back up before touching anything - but only if there's actually a
    // world to lose. A never-started instance has none, and failing the
    // update over that would be nonsense.
    if server_dir.join(&world_folder_name).is_dir() {
        super::backup::create_world_backup(state.clone(), id.clone())
            .await
            .map_err(|e| format!("Aborted - couldn't back up the world first: {e}"))?;
    }

    // Stage into the instance folder rather than a system temp dir so the
    // copy stays on the same volume (a multi-GB pack crossing drives is
    // slow) and any leftovers are obvious and self-cleaning.
    let staging = instance_dir.join(".pack-staging");
    let _ = tokio::fs::remove_dir_all(&staging).await;
    tokio::fs::create_dir_all(&staging)
        .await
        .map_err(|e| format!("Failed to prepare staging folder: {e}"))?;

    let staged = copy_source_into(source, staging.clone()).await;
    let result = match staged {
        Ok(()) => {
            crate::packs::apply_staged_pack(&instance_dir, &server_dir, &staging, &world_folder_name)
                .await
                .map(|_| ())
        }
        Err(e) => Err(e),
    };
    let _ = tokio::fs::remove_dir_all(&staging).await;
    result?;

    // The installed content no longer corresponds to whatever Modrinth
    // version was recorded, so clear it rather than let "Check for Updates"
    // compare against a version that isn't what's on disk any more. The
    // project link itself is kept - it's still the same modpack.
    sqlx::query("UPDATE instances SET modrinth_version_id = NULL WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| format!("Update applied, but failed to clear the recorded version: {e}"))?;

    super::instance::fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())
}

async fn copy_source_into(source: ImportSource, dest: PathBuf) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || match source {
        ImportSource::Zip { path } => {
            let path = PathBuf::from(path);
            importer::extract_zip_safely(&path, &dest)
                .map(|_warnings| ())
                .map_err(|e| format!("Failed to extract ZIP: {e}"))
        }
        ImportSource::Folder { path } => {
            let path = Path::new(&path);
            if !path.is_dir() {
                return Err("Selected path is not a folder".to_string());
            }
            importer::copy_dir_recursive(path, &dest).map_err(|e| format!("Failed to copy server files: {e}"))
        }
    })
    .await
    .map_err(|e| format!("Import task failed: {e}"))?
}
