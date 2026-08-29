use std::path::Path;

use chrono::Utc;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use super::import::INSTANCE_EXISTS_PREFIX;
use super::diagnostics::detect_world_folder_name;
use super::instance::{fetch_instance, insert_instance, write_instance_json};
use super::server::resolve_java_path;
use crate::filesystem::sanitize_dir_name;
use crate::models::{
    FtbImportRequest, FtbPack, FtbUpdateCheck, FtbVersionManifest, FtbVersionPreview,
    FtbVersionSummary, Instance, ServerLoader, ServerStatus,
};
use crate::server::{FtbInstallProgressPayload, FTB_INSTALL_PROGRESS_EVENT};
use crate::{ftb, loader, AppState};

/// Searches FTB's public modpacks by name, for the import wizard's pack
/// picker.
#[tauri::command]
pub async fn search_ftb_packs(query: String) -> Result<Vec<FtbPack>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    ftb::search_packs(query.trim()).await
}

/// The list of modpacks shown before anyone has typed a search term:
/// FTB's featured packs, then its most-installed ones.
#[tauri::command]
pub async fn browse_ftb_packs() -> Result<Vec<FtbPack>, String> {
    ftb::browse_packs().await
}
/// Loads one pack, including its version list (newest first, which is not
/// the order FTB returns them in).
#[tauri::command]
pub async fn get_ftb_pack(pack_id: i64) -> Result<FtbPack, String> {
    let mut pack = ftb::get_pack(pack_id).await?;
    pack.versions = pack.versions_newest_first();
    Ok(pack)
}

/// Reports what installing a version would do - Minecraft version, loader,
/// mod count, total download size - without downloading anything.
///
/// This is the FTB equivalent of `analyze_import`: the user reviews it
/// before any files are written.
#[tauri::command]
pub async fn analyze_ftb_version(
    pack_id: i64,
    version_id: i64,
) -> Result<FtbVersionPreview, String> {
    let manifest = ftb::get_version(pack_id, version_id).await?;
    Ok(ftb::preview(&manifest))
}

/// Installs an FTB pack version as a new instance.
///
/// Unlike `import_instance`, there is no local source to copy: the files
/// are downloaded from FTB, and then the mod loader's own server is
/// installed on top (FTB never ships it - see `crate::loader`). That second
/// step is the part FTB's `serverinstall_*.exe` exists to do.
///
/// The whole thing rolls back on failure, so a half-downloaded pack never
/// leaves an unusable instance sitting in the list.
#[tauri::command]
pub async fn import_ftb_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    request: FtbImportRequest,
) -> Result<Instance, String> {
    let FtbImportRequest { pack_id, version_id, overwrite, .. } = request;

    let name = request.name.trim().to_string();
    if name.is_empty() {
        return Err("Instance name cannot be empty".to_string());
    }

    let min_ram_mb = request.min_ram_mb.unwrap_or(2048);
    let max_ram_mb = request.max_ram_mb.unwrap_or(4096);
    if min_ram_mb <= 0 || max_ram_mb <= 0 || min_ram_mb > max_ram_mb {
        return Err("Minimum RAM must be positive and not exceed maximum RAM".to_string());
    }

    // Fetch the manifest before touching the filesystem: a typo'd version
    // or an FTB outage should fail before an instance folder exists.
    let pack = ftb::get_pack(pack_id).await?;
    let manifest = ftb::get_version(pack_id, version_id).await?;
    let preview = ftb::preview(&manifest);

    // Resolve Java up front too. Forge/NeoForge can't be installed without
    // it, and finding that out *after* a multi-gigabyte download would be
    // a miserable way to learn it.
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
        if !overwrite {
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

    let result = install_into(
        &app,
        &instance_dir,
        &server_dir,
        &manifest,
        &preview,
        java_path.as_deref(),
    )
    .await;

    let installed = match result {
        Ok(installed) => installed,
        Err(e) => {
            let _ = tokio::fs::remove_dir_all(&instance_dir).await;
            return Err(e);
        }
    };

    // Force ModpackPilot's own defaults onto the pack's server.properties -
    // most importantly whitelist off, so the pack's own `white-list=true`
    // doesn't lock everyone out on first boot. Same treatment a ZIP import
    // gets, and best-effort for the same reason.
    if let Err(e) = super::server_properties::apply_import_defaults(&server_dir).await {
        tracing::warn!("Couldn't apply default server properties on FTB import: {e}");
    }

    let instance = Instance {
        id: Uuid::new_v4().to_string(),
        name,
        minecraft_version: preview.minecraft_version.clone(),
        loader: preview.loader,
        loader_version: preview.loader_version.clone(),
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
        modrinth_project_id: None,
        modrinth_project_title: None,
        modrinth_version_id: None,
        ftb_pack_id: Some(pack.id),
        ftb_pack_name: Some(pack.name.clone()),
        ftb_version_id: Some(manifest.id),
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
        tracing::warn!("Instance {} installed, but instance.json failed: {e}", instance.id);
    }

    Ok(instance)
}

/// Downloads the pack and installs its loader, reporting progress as it
/// goes. Split out so the caller can roll the whole instance directory back
/// on any failure in here.
async fn install_into(
    app: &AppHandle,
    instance_dir: &Path,
    server_dir: &Path,
    manifest: &FtbVersionManifest,
    preview: &FtbVersionPreview,
    java_path: Option<&str>,
) -> Result<Option<loader::InstalledLoader>, String> {
    let emitter = app.clone();
    emit_progress(app, "downloading", 0, preview.total_files, None);

    // A brand-new instance has no world to protect, but the parameter is
    // what makes this same call safe to reuse for an in-place update.
    ftb::install_files(instance_dir, manifest, "world", move |done, total| {
        emit_progress(&emitter, "downloading", done, total, None);
    })
    .await?;

    if !loader::is_supported(preview.loader) {
        // Not fatal: the pack itself is installed and usable once someone
        // puts a server jar in place. The warning already told them.
        tracing::warn!(
            "FTB pack installed, but its loader ({}) can't be installed automatically",
            preview.loader.as_str(),
        );
        return Ok(None);
    }

    let Some(loader_version) = preview.loader_version.as_deref() else {
        return Ok(None);
    };

    emit_progress(
        app,
        "installing-loader",
        preview.total_files,
        preview.total_files,
        Some(format!(
            "Installing {} {loader_version}",
            preview.loader.as_str()
        )),
    );

    let installed = loader::install(
        server_dir,
        preview.loader,
        loader_version,
        preview.minecraft_version.as_deref(),
        java_path.unwrap_or("java"),
    )
    .await?;

    emit_progress(
        app,
        "done",
        preview.total_files,
        preview.total_files,
        None,
    );

    Ok(Some(installed))
}

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


/// Links an existing instance to an FTB pack, so "Check for Updates" has
/// something to check against.
///
/// Needed for instances that weren't installed through ModpackPilot - an
/// FTB pack imported from a ZIP, or one installed with FTB's own
/// `serverinstall_*.exe` before the operator moved it here.
#[tauri::command]
pub async fn link_ftb_pack(
    state: State<'_, AppState>,
    id: String,
    pack_id: i64,
) -> Result<Instance, String> {
    fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let pack = ftb::get_pack(pack_id).await?;

    // The installed files aren't known to correspond to any particular
    // version, so the version is left NULL rather than guessed - that's the
    // difference between "linked but never checked" and "up to date".
    sqlx::query(
        "UPDATE instances SET ftb_pack_id = ?, ftb_pack_name = ?, ftb_version_id = NULL
         WHERE id = ?",
    )
    .bind(pack.id)
    .bind(&pack.name)
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(|e| format!("Failed to save FTB link: {e}"))?;

    fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())
}

#[tauri::command]
pub async fn unlink_ftb_pack(state: State<'_, AppState>, id: String) -> Result<Instance, String> {
    sqlx::query(
        "UPDATE instances SET ftb_pack_id = NULL, ftb_pack_name = NULL, ftb_version_id = NULL
         WHERE id = ?",
    )
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(|e| format!("Failed to remove FTB link: {e}"))?;

    fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())
}

/// Checks whether a linked instance's FTB pack has a newer version.
/// Downloads and changes nothing - that's `apply_ftb_update`.
#[tauri::command]
pub async fn check_ftb_update(
    state: State<'_, AppState>,
    id: String,
) -> Result<FtbUpdateCheck, String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let pack_id = instance
        .ftb_pack_id
        .ok_or_else(|| "This instance isn't linked to an FTB modpack".to_string())?;

    let pack = ftb::get_pack(pack_id).await?;
    let loader = match instance.loader {
        ServerLoader::Unknown => None,
        other => Some(other.as_str()),
    };
    let latest = ftb::pick_latest_for_instance(
        &pack.versions,
        loader,
        instance.minecraft_version.as_deref(),
    );

    Ok(FtbUpdateCheck {
        has_update: latest
            .as_ref()
            .is_some_and(|v| Some(v.id) != instance.ftb_version_id),
        current_version_id: instance.ftb_version_id,
        latest_version: latest,
    })
}

/// Every published version of a linked instance's pack, newest first.
#[tauri::command]
pub async fn list_ftb_versions(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<FtbVersionSummary>, String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let pack_id = instance
        .ftb_pack_id
        .ok_or_else(|| "This instance isn't linked to an FTB modpack".to_string())?;

    Ok(ftb::get_pack(pack_id).await?.versions_newest_first())
}

/// Installs a specific FTB version onto an existing instance.
///
/// The world, `server.properties`, and the player lists are never touched
/// (see `crate::packs`), and files the previous version installed that this
/// one no longer lists are removed. A world backup is taken first, and the
/// pack is staged in full before anything is committed, so a failed
/// download leaves the instance exactly as it was.
///
/// Unlike a Modrinth update, this may also have to reinstall the *loader*:
/// an FTB pack bumping its NeoForge build between versions is routine, and
/// leaving the old server jar in place would run the new mods against the
/// loader they were not built for.
#[tauri::command]
pub async fn apply_ftb_update(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    version_id: i64,
) -> Result<Instance, String> {
    if state.processes.is_running(&id).await {
        return Err("Stop the instance before updating it".to_string());
    }

    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let pack_id = instance
        .ftb_pack_id
        .ok_or_else(|| "This instance isn't linked to an FTB modpack".to_string())?;

    let manifest = ftb::get_version(pack_id, version_id).await?;
    let preview = ftb::preview(&manifest);

    let instance_dir = Path::new(&instance.server_directory).to_path_buf();
    let server_dir = instance_dir.join("server");
    let world_folder_name = detect_world_folder_name(&server_dir).await;

    super::modrinth::back_up_world_before_update(&state, &id, &server_dir, &world_folder_name)
        .await?;

    let total = preview.total_files;
    let emitter = app.clone();
    emit_progress(&app, "downloading", 0, total, None);
    ftb::install_files(&instance_dir, &manifest, &world_folder_name, move |done, total| {
        emit_progress(&emitter, "downloading", done, total, None);
    })
    .await?;

    // Reinstall the loader only when the pack actually moved to a different
    // one - a loader install is slow, and re-running it for an unchanged
    // version would be pure waste.
    let loader_changed = preview.loader != instance.loader
        || (preview.loader_version.is_some() && preview.loader_version != instance.loader_version);

    let installed = if loader_changed && loader::is_supported(preview.loader) {
        let Some(loader_version) = preview.loader_version.as_deref() else {
            return Err("This version doesn't say which loader build it needs".to_string());
        };

        let java_path = if preview.loader == ServerLoader::Fabric {
            "java".to_string()
        } else {
            resolve_java_path(
                &state.db,
                instance.java_installation_id.as_deref(),
                preview.minecraft_version.as_deref(),
                preview.loader,
            )
            .await?
        };

        emit_progress(
            &app,
            "installing-loader",
            total,
            total,
            Some(format!("Installing {} {loader_version}", preview.loader.as_str())),
        );

        Some(
            loader::install(
                &server_dir,
                preview.loader,
                loader_version,
                preview.minecraft_version.as_deref(),
                &java_path,
            )
            .await?,
        )
    } else {
        None
    };

    emit_progress(&app, "done", total, total, None);

    sqlx::query(
        "UPDATE instances SET ftb_version_id = ?, minecraft_version = ?, loader = ?,
                              loader_version = ?, server_jar = COALESCE(?, server_jar),
                              launch_mode = COALESCE(?, launch_mode)
         WHERE id = ?",
    )
    .bind(manifest.id)
    .bind(&preview.minecraft_version)
    .bind(preview.loader.as_str())
    .bind(&preview.loader_version)
    .bind(installed.as_ref().map(|i| i.server_jar.clone()))
    .bind(installed.as_ref().map(|i| i.launch_mode.clone()))
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(|e| format!("Update installed, but failed to record the new version: {e}"))?;

    let updated = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    if let Err(e) = write_instance_json(&instance_dir, &updated).await {
        tracing::warn!("Instance {id} updated, but instance.json failed: {e}");
    }

    Ok(updated)
}
