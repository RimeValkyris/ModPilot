use std::path::Path;

use tauri::State;

use super::diagnostics::detect_world_folder_name;
use super::instance::fetch_instance;
use crate::models::{Instance, ModpackUpdateCheck, ModrinthSearchHit, ModrinthVersion};
use crate::modrinth;
use crate::AppState;

/// Searches Modrinth's modpacks for the linking UI - lets a user pick a
/// project by name instead of needing to already know its ID/slug.
#[tauri::command]
pub async fn search_modrinth_projects(query: String) -> Result<Vec<ModrinthSearchHit>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    modrinth::search_projects(query.trim()).await
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
#[tauri::command]
pub async fn apply_modpack_update(
    state: State<'_, AppState>,
    id: String,
    version_id: String,
) -> Result<Instance, String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    if instance.modrinth_project_id.is_none() {
        return Err("This instance isn't linked to a Modrinth project".to_string());
    }

    let version = modrinth::get_version(&version_id).await?;
    let instance_dir = Path::new(&instance.server_directory);
    let world_folder_name = detect_world_folder_name(&instance_dir.join("server")).await;

    modrinth::apply_update(instance_dir, &version, &world_folder_name).await?;

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

/// Sets how this instance handles newly published Modrinth versions:
/// `"off"`, `"notify"`, or `"auto"` (see `server::autoupdate`).
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
        if instance.modrinth_project_id.is_none() {
            return Err("Link a Modrinth project first - there's nothing to check for updates against".to_string());
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
