use std::path::Path;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use tauri::State;

use super::instance::fetch_instance;
use crate::AppState;

/// Cosmetic only - this is what ModpackPilot itself shows on the instance
/// card/detail header, unrelated to Minecraft's own `server-icon.png` (which
/// is locked to an exact 64x64 PNG by the game client). Any reasonable image
/// works here since it's just decoration, so it's kept generous.
const MAX_AVATAR_BYTES: u64 = 8 * 1024 * 1024;
const ALLOWED_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif"];

fn mime_for_extension(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "image/png",
    }
}

/// Finds the current `avatar.*` file in an instance's root directory, if
/// any. Stored at the instance root (a sibling of `server/`, `logs/`) rather
/// than inside `server/` - it's ModpackPilot's own metadata, not a Minecraft
/// server file, and should never be confused with one or shipped alongside
/// the actual server files.
async fn find_existing_avatar(server_directory: &str) -> Option<std::path::PathBuf> {
    let mut entries = tokio::fs::read_dir(server_directory).await.ok()?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.file_stem().and_then(|s| s.to_str()) == Some("avatar") {
            return Some(path);
        }
    }
    None
}

/// Sets the instance's profile picture, shown throughout ModpackPilot's own
/// UI. Replaces any previously set avatar, including one with a different
/// extension.
#[tauri::command]
pub async fn set_instance_avatar(
    state: State<'_, AppState>,
    id: String,
    source_path: String,
) -> Result<(), String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let ext = Path::new(&source_path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .filter(|e| ALLOWED_EXTENSIONS.contains(&e.as_str()))
        .ok_or_else(|| "Image must be a PNG, JPG, WEBP, or GIF file".to_string())?;

    let metadata = tokio::fs::metadata(&source_path)
        .await
        .map_err(|e| format!("Failed to read image file: {e}"))?;
    if metadata.len() > MAX_AVATAR_BYTES {
        return Err("Image is too large (max 8MB)".to_string());
    }

    let bytes = tokio::fs::read(&source_path)
        .await
        .map_err(|e| format!("Failed to read image file: {e}"))?;

    if let Some(old) = find_existing_avatar(&instance.server_directory).await {
        let _ = tokio::fs::remove_file(old).await;
    }

    let dest = Path::new(&instance.server_directory).join(format!("avatar.{ext}"));
    tokio::fs::write(&dest, bytes)
        .await
        .map_err(|e| format!("Failed to save profile picture: {e}"))?;

    Ok(())
}

#[tauri::command]
pub async fn clear_instance_avatar(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    if let Some(path) = find_existing_avatar(&instance.server_directory).await {
        tokio::fs::remove_file(path)
            .await
            .map_err(|e| format!("Failed to remove profile picture: {e}"))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn read_instance_avatar(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<String>, String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let Some(path) = find_existing_avatar(&instance.server_directory).await else {
        return Ok(None);
    };
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_string();

    match tokio::fs::read(&path).await {
        Ok(bytes) => Ok(Some(format!(
            "data:{};base64,{}",
            mime_for_extension(&ext),
            BASE64.encode(bytes)
        ))),
        Err(_) => Ok(None),
    }
}
