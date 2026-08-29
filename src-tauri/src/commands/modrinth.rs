use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use chrono::Utc;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use super::diagnostics::detect_world_folder_name;
use super::import::INSTANCE_EXISTS_PREFIX;
use super::instance::{fetch_instance, insert_instance, write_instance_json};
use super::server::resolve_java_path;
use crate::filesystem::sanitize_dir_name;
use crate::models::{
    AppliedPack, Instance, ModpackUpdateCheck, ModrinthImportRequest, ModrinthSearchHit,
    ModrinthVersion, ModrinthVersionPreview, ServerLoader, ServerStatus,
};
use crate::server::{FtbInstallProgressPayload, FTB_INSTALL_PROGRESS_EVENT};
use crate::AppState;
use crate::{loader, modrinth};

/// Searches Modrinth's modpacks for the linking UI - lets a user pick a
/// project by name instead of needing to already know its ID/slug.
#[tauri::command]
pub async fn search_modrinth_projects(query: String) -> Result<Vec<ModrinthSearchHit>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    modrinth::search_projects(query.trim()).await
}

/// The list of modpacks shown before anyone has typed a search term: the
/// most downloaded ones on Modrinth.
#[tauri::command]
pub async fn browse_modrinth_packs() -> Result<Vec<ModrinthSearchHit>, String> {
    modrinth::browse_projects().await
}

/// Every published version of a Modrinth project, newest first.
///
/// Unlike `list_modpack_versions`, this takes a project ID directly rather
/// than an instance, so the import wizard can offer versions for a pack
/// that hasn't been installed yet.
#[tauri::command]
pub async fn list_modrinth_project_versions(
    project_id: String,
) -> Result<Vec<ModrinthVersion>, String> {
    modrinth::get_project_versions(&project_id).await
}

/// Reports what installing a Modrinth version would do - Minecraft version,
/// loader, mod count, download size - without installing anything.
///
/// The Modrinth counterpart of `analyze_ftb_version`.
#[tauri::command]
pub async fn analyze_modrinth_version(
    version_id: String,
) -> Result<ModrinthVersionPreview, String> {
    Ok(modrinth::preview(&modrinth::get_version(&version_id).await?))
}

/// Installs a Modrinth modpack version as a new instance.
///
/// The same shape as `import_ftb_instance`, and for the same reason: a
/// `.mrpack` lists mods but never ships the loader's server, so the files
/// are downloaded first and `crate::loader` installs the server on top.
/// The whole instance directory is rolled back on any failure.
#[tauri::command]
pub async fn import_modrinth_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ModrinthImportRequest,
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

    // Everything that can fail over the network happens before the
    // filesystem is touched, so a Modrinth outage can't leave a half-made
    // instance folder behind.
    let project = modrinth::get_project(&request.project_id).await?;
    let version = modrinth::get_version(&request.version_id).await?;
    let preview = modrinth::preview(&version);

    // Resolve Java up front: Forge/NeoForge can't be installed without it,
    // and finding that out after a multi-gigabyte download would be a
    // miserable way to learn it.
    let java_path = if loader::is_supported(preview.loader) && preview.loader != ServerLoader::Fabric
    {
        Some(
            resolve_java_path(
                &state.db,
                None,
                preview.minecraft_version.as_deref(),
                preview.loader,
            )
            .await?,
        )
    } else {
        None
    };

    let dir_name = sanitize_dir_name(&name);
    let instance_dir = state.paths.instances_dir.join(&dir_name);

    if instance_dir.exists() {
        if !request.overwrite {
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

    let result = install_into(&app, &instance_dir, &server_dir, &version, java_path.as_deref()).await;

    let (applied, installed) = match result {
        Ok(outcome) => outcome,
        Err(e) => {
            let _ = tokio::fs::remove_dir_all(&instance_dir).await;
            return Err(e);
        }
    };

    // Force ModpackPilot's own defaults onto the pack's server.properties -
    // most importantly whitelist off, so the pack's own `white-list=true`
    // doesn't lock everyone out on first boot.
    if let Err(e) = super::server_properties::apply_import_defaults(&server_dir).await {
        tracing::warn!("Couldn't apply default server properties on Modrinth import: {e}");
    }

    let instance = Instance {
        id: Uuid::new_v4().to_string(),
        name,
        minecraft_version: applied
            .minecraft_version
            .clone()
            .or_else(|| preview.minecraft_version.clone()),
        loader: applied.loader,
        loader_version: applied.loader_version.clone(),
        java_installation_id: None,
        min_ram_mb,
        max_ram_mb,
        server_directory: instance_dir.to_string_lossy().to_string(),
        server_jar: installed.as_ref().map(|i| i.server_jar.clone()),
        launch_mode: installed
            .as_ref()
            .map(|i| i.launch_mode.clone())
            .unwrap_or_else(|| "jar".to_string()),
        jvm_args: Vec::new(),
        server_args: Vec::new(),
        status: ServerStatus::Stopped,
        auto_start: false,
        auto_restart: false,
        created_at: Utc::now(),
        last_launched_at: None,
        // Linked from the start, so "Check for Updates" works without the
        // operator having to go and link it by hand afterwards.
        modrinth_project_id: Some(project.id.clone()),
        modrinth_project_title: Some(project.title.clone()),
        modrinth_version_id: Some(version.id.clone()),
        ftb_pack_id: None,
        ftb_pack_name: None,
        ftb_version_id: None,
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
        tracing::warn!(
            "Instance {} installed, but instance.json failed: {e}",
            instance.id
        );
    }

    Ok(instance)
}

/// Downloads the pack and installs its loader, reporting progress as it
/// goes. Split out so the caller can roll the whole instance directory back
/// on any failure in here.
///
/// Returns what the pack turned out to need alongside what got installed:
/// the exact loader build only appears in the `.mrpack`'s manifest, so it
/// isn't known until this has run.
async fn install_into(
    app: &AppHandle,
    instance_dir: &Path,
    server_dir: &Path,
    version: &ModrinthVersion,
    java_path: Option<&str>,
) -> Result<(AppliedPack, Option<loader::InstalledLoader>), String> {
    let emitter = app.clone();
    // The real totals arrive with the first progress callback - they come
    // from the pack manifest, which hasn't been read yet.
    emit_progress(app, "downloading", 0, 0, None);

    // Remembered so the loader step, which is the slow half, can keep
    // showing a full bar instead of snapping back to 0%.
    let file_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&file_count);

    // A brand-new instance has no world to protect, which is what makes
    // installing and updating the same operation here.
    let applied = modrinth::apply_update(instance_dir, version, "world", move |done, total| {
        counter.store(total, Ordering::Relaxed);
        emit_progress(&emitter, "downloading", done, total, None);
    })
    .await?;

    let total_files = file_count.load(Ordering::Relaxed);

    if !loader::is_supported(applied.loader) {
        // Not fatal: the pack itself is installed and usable once someone
        // puts a server jar in place. The warning already told them.
        tracing::warn!(
            "Modrinth pack installed, but its loader ({}) can't be installed automatically",
            applied.loader.as_str(),
        );
        return Ok((applied, None));
    }

    let Some(loader_version) = applied.loader_version.clone() else {
        return Ok((applied, None));
    };

    emit_progress(
        app,
        "installing-loader",
        total_files,
        total_files,
        Some(format!(
            "Installing {} {loader_version}",
            applied.loader.as_str()
        )),
    );

    let installed = loader::install(
        server_dir,
        applied.loader,
        &loader_version,
        applied.minecraft_version.as_deref(),
        java_path.unwrap_or("java"),
    )
    .await?;

    emit_progress(app, "done", total_files, total_files, None);

    Ok((applied, Some(installed)))
}

/// Reuses the FTB install-progress event: the import wizard renders one
/// progress bar regardless of which source the pack came from, so a second
/// event name would just be two names for the same thing.
fn emit_progress(app: &AppHandle, phase: &str, done: usize, total: usize, detail: Option<String>) {
    let _ = app.emit(
        FTB_INSTALL_PROGRESS_EVENT,
        FtbInstallProgressPayload {
            phase: phase.to_string(),
            done,
            total,
            detail,
        },
    );
}

/// Links an instance to a Modrinth project so "Check for Updates" has
/// something to check against. Validates the project actually exists
/// before saving anything.
#[tauri::command]
pub async fn link_modrinth_project(
    state: State<'_, AppState>,
    id: String,
    project_id: String,
) -> Result<Instance, String> {
    fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let project = modrinth::get_project(&project_id).await?;

    sqlx::query(
        "UPDATE instances SET modrinth_project_id = ?, modrinth_project_title = ?, modrinth_version_id = NULL
         WHERE id = ?",
    )
    .bind(&project.id)
    .bind(&project.title)
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(|e| format!("Failed to save Modrinth link: {e}"))?;

    fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())
}

#[tauri::command]
pub async fn unlink_modrinth_project(state: State<'_, AppState>, id: String) -> Result<Instance, String> {
    sqlx::query(
        "UPDATE instances SET modrinth_project_id = NULL, modrinth_project_title = NULL, modrinth_version_id = NULL
         WHERE id = ?",
    )
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(|e| format!("Failed to remove Modrinth link: {e}"))?;

    fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())
}

/// Checks whether a linked instance's project has a newer version than
/// what's installed. Doesn't download or change anything - that's
/// `apply_modpack_update`. `latest_version` comes back `None` (not an
/// error) if the project has versions but none match this instance's
/// loader/Minecraft version - `list_modpack_versions` lets the operator
/// browse and pick one manually in that case.
#[tauri::command]
pub async fn check_modpack_update(state: State<'_, AppState>, id: String) -> Result<ModpackUpdateCheck, String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let project_id = instance
        .modrinth_project_id
        .as_deref()
        .ok_or_else(|| "This instance isn't linked to a Modrinth project".to_string())?;

    let versions = modrinth::get_project_versions(project_id).await?;
    let loader = match instance.loader {
        crate::models::ServerLoader::Unknown => None,
        other => Some(other.as_str()),
    };
    let latest =
        modrinth::pick_latest_for_instance(&versions, loader, instance.minecraft_version.as_deref());

    Ok(ModpackUpdateCheck {
        has_update: latest.is_some_and(|v| Some(v.id.as_str()) != instance.modrinth_version_id.as_deref()),
        current_version_id: instance.modrinth_version_id.clone(),
        latest_version: latest.cloned(),
    })
}

/// Every published version of a linked instance's project, newest first -
/// for manually picking a version to install when no auto-matched
/// "latest" is available (or the operator wants a different one anyway).
#[tauri::command]
pub async fn list_modpack_versions(state: State<'_, AppState>, id: String) -> Result<Vec<ModrinthVersion>, String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let project_id = instance
        .modrinth_project_id
        .ok_or_else(|| "This instance isn't linked to a Modrinth project".to_string())?;

    modrinth::get_project_versions(&project_id).await
}

/// Downloads and installs a specific Modrinth version onto an instance -
/// see `modrinth::apply_update` for exactly what does and doesn't get
/// touched (the world, whitelist/ops/bans, and server.properties are
/// always left alone).
///
/// A world backup is taken first and the instance must be stopped, for the
/// same reasons `update_instance_from_source` requires both: replacing mod
/// jars under a live server corrupts it, and a pack update can leave an
/// existing world unloadable. Backups made here are never touched by
/// scheduled-backup retention, so the way back from a bad update can't be
/// pruned out from under the operator.
#[tauri::command]
pub async fn apply_modpack_update(
    state: State<'_, AppState>,
    id: String,
    version_id: String,
) -> Result<Instance, String> {
    if state.processes.is_running(&id).await {
        return Err("Stop the instance before updating it".to_string());
    }

    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    if instance.modrinth_project_id.is_none() {
        return Err("This instance isn't linked to a Modrinth project".to_string());
    }

    let version = modrinth::get_version(&version_id).await?;
    let instance_dir = Path::new(&instance.server_directory);
    let server_dir = instance_dir.join("server");
    let world_folder_name = detect_world_folder_name(&server_dir).await;

    back_up_world_before_update(&state, &id, &server_dir, &world_folder_name).await?;

    // No progress reporting here: the update UI shows a single spinner
    // rather than a file counter, unlike the import wizard.
    modrinth::apply_update(instance_dir, &version, &world_folder_name, |_, _| {}).await?;

    sqlx::query("UPDATE instances SET modrinth_version_id = ? WHERE id = ?")
        .bind(&version.id)
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| format!("Update installed, but failed to record the new version: {e}"))?;

    fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())
}

/// Takes the safety backup that precedes any in-place pack update.
///
/// Skipped when there's no world yet - a never-started instance has nothing
/// to lose, and failing an update over that would be nonsense.
pub(crate) async fn back_up_world_before_update(
    state: &State<'_, AppState>,
    id: &str,
    server_dir: &Path,
    world_folder_name: &str,
) -> Result<(), String> {
    if !server_dir.join(world_folder_name).is_dir() {
        return Ok(());
    }
    super::backup::create_world_backup(state.clone(), id.to_string())
        .await
        .map(|_| ())
        .map_err(|e| format!("Aborted - couldn't back up the world first: {e}"))
}

/// Sets how this instance handles newly published modpack versions:
/// `"off"`, `"notify"`, or `"auto"` (see `server::autoupdate`).
///
/// Shared by both update sources - an instance linked to either Modrinth or
/// FTB has something to check against, and the policy means the same thing
/// either way.
#[tauri::command]
pub async fn set_update_policy(
    state: State<'_, AppState>,
    id: String,
    policy: String,
) -> Result<Instance, String> {
    if !matches!(policy.as_str(), "off" | "notify" | "auto") {
        return Err(format!("Unknown update policy: \"{policy}\""));
    }

    // "notify"/"auto" are meaningless without something to check against,
    // and silently accepting them would look like it worked.
    if policy != "off" {
        let instance = fetch_instance(&state, &id)
            .await?
            .ok_or_else(|| "Instance not found".to_string())?;
        if instance.modrinth_project_id.is_none() && instance.ftb_pack_id.is_none() {
            return Err(
                "Link a Modrinth project or an FTB modpack first - there's nothing to check for \
                 updates against"
                    .to_string(),
            );
        }
    }

    let result = sqlx::query("UPDATE instances SET update_policy = ? WHERE id = ?")
        .bind(&policy)
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| format!("Failed to save update policy: {e}"))?;

    if result.rows_affected() == 0 {
        return Err("Instance not found".to_string());
    }

    fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())
}
