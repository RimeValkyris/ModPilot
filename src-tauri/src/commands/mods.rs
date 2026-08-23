use std::path::Path;

use tauri::State;

use super::instance::fetch_instance;
use crate::models::ModInfo;
use crate::AppState;

/// Rejects a mod filename containing anything that could escape the mods
/// folder - the frontend only ever sends back names this command's own
/// `list_mods` produced, but this is what actually enforces that.
fn validate_mod_file_name(name: &str) -> Result<(), String> {
    let safe = !name.is_empty() && !name.contains('/') && !name.contains('\\') && !name.contains("..");
    if safe {
        Ok(())
    } else {
        Err("Invalid mod file name".to_string())
    }
}

async fn mods_dir_for(state: &State<'_, AppState>, id: &str) -> Result<std::path::PathBuf, String> {
    let instance = fetch_instance(state, id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;
    Ok(Path::new(&instance.server_directory).join("server").join("mods"))
}

/// Lists the instance's mods folder. A `.jar.disabled` file is a mod
/// `toggle_mod` has switched off - Minecraft loaders only load `.jar`, so
/// renaming it is a simple, loader-agnostic way to disable a mod without
/// deleting it.
#[tauri::command]
pub async fn list_mods(state: State<'_, AppState>, id: String) -> Result<Vec<ModInfo>, String> {
    let mods_dir = mods_dir_for(&state, &id).await?;

    let mut entries = match tokio::fs::read_dir(&mods_dir).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("Failed to read mods folder: {e}")),
    };

    let mut mods = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| format!("Failed to read mods folder: {e}"))?
    {
        let file_name = entry.file_name().to_string_lossy().to_string();
        let enabled = file_name.ends_with(".jar");
        if !enabled && !file_name.ends_with(".jar.disabled") {
            continue;
        }
        let Ok(metadata) = entry.metadata().await else {
            continue;
        };

        let display_name = file_name.strip_suffix(".disabled").unwrap_or(&file_name).to_string();

        mods.push(ModInfo {
            file_name,
            display_name,
            enabled,
            size_bytes: metadata.len(),
        });
    }

    mods.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    Ok(mods)
}

/// Flips a mod between enabled (`.jar`) and disabled (`.jar.disabled`).
#[tauri::command]
pub async fn toggle_mod(state: State<'_, AppState>, id: String, file_name: String) -> Result<(), String> {
    validate_mod_file_name(&file_name)?;
    let mods_dir = mods_dir_for(&state, &id).await?;
    let current = mods_dir.join(&file_name);

    let new_name = if let Some(base) = file_name.strip_suffix(".disabled") {
        base.to_string()
    } else {
        format!("{file_name}.disabled")
    };

    tokio::fs::rename(&current, mods_dir.join(&new_name))
        .await
        .map_err(|e| format!("Failed to toggle mod: {e}"))?;

    Ok(())
}

/// Permanently deletes a mod file.
#[tauri::command]
pub async fn delete_mod(state: State<'_, AppState>, id: String, file_name: String) -> Result<(), String> {
    validate_mod_file_name(&file_name)?;
    let mods_dir = mods_dir_for(&state, &id).await?;

    tokio::fs::remove_file(mods_dir.join(&file_name))
        .await
        .map_err(|e| format!("Failed to delete mod: {e}"))?;

    Ok(())
}
