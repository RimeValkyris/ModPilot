use std::path::Path;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use tauri::State;

use super::instance::fetch_instance;
use crate::AppState;

const ALLOWED_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif"];
/// Wallpapers are decorative; there's no reason to let someone point this at
/// a multi-hundred-MB file and have ModForge base64-encode it into memory
/// on every read.
const MAX_WALLPAPER_BYTES: u64 = 10 * 1024 * 1024;

fn mime_for_extension(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "application/octet-stream",
    }
}

/// Copies an image the user picked into the instance's own directory as its
/// wallpaper, replacing any previous one.
#[tauri::command]
pub async fn set_instance_wallpaper(
    state: State<'_, AppState>,
    id: String,
    source_path: String,
) -> Result<(), String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let source = Path::new(&source_path);
    let ext = source
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .filter(|e| ALLOWED_EXTENSIONS.contains(&e.as_str()))
        .ok_or_else(|| "Unsupported image type. Use PNG, JPEG, WEBP, or GIF.".to_string())?;

    let metadata = tokio::fs::metadata(source)
        .await
        .map_err(|e| format!("Failed to read image file: {e}"))?;
    if metadata.len() > MAX_WALLPAPER_BYTES {
        return Err("Image is too large (max 10 MB)".to_string());
    }

    // Remove any previously-set wallpaper first, in case its extension differs.
    remove_existing_wallpaper(&instance.server_directory).await;

    let dest = Path::new(&instance.server_directory).join(format!("wallpaper.{ext}"));
    tokio::fs::copy(source, &dest)
        .await
        .map_err(|e| format!("Failed to copy image: {e}"))?;

    sqlx::query("UPDATE instances SET wallpaper_path = ? WHERE id = ?")
        .bind(dest.to_string_lossy().to_string())
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| format!("Failed to save wallpaper: {e}"))?;

    Ok(())
}

/// Removes an instance's wallpaper, if it has one.
#[tauri::command]
pub async fn clear_instance_wallpaper(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    remove_existing_wallpaper(&instance.server_directory).await;

    sqlx::query("UPDATE instances SET wallpaper_path = NULL WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| format!("Failed to clear wallpaper: {e}"))?;

    Ok(())
}

/// Reads an instance's wallpaper as a data URI the frontend can drop
/// straight into an `<img>`/CSS `background-image`, or `None` if unset.
#[tauri::command]
pub async fn read_instance_wallpaper(state: State<'_, AppState>, id: String) -> Result<Option<String>, String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let Some(path) = instance.wallpaper_path else {
        return Ok(None);
    };

    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(_) => return Ok(None), // File missing/moved - treat as "no wallpaper" rather than erroring.
    };

    let ext = Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_lowercase();

    Ok(Some(format!(
        "data:{};base64,{}",
        mime_for_extension(&ext),
        BASE64.encode(bytes)
    )))
}

async fn remove_existing_wallpaper(server_directory: &str) {
    for ext in ALLOWED_EXTENSIONS {
        let path = Path::new(server_directory).join(format!("wallpaper.{ext}"));
        let _ = tokio::fs::remove_file(path).await;
    }
}
