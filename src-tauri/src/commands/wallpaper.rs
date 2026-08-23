use std::path::{Path, PathBuf};

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

/// Validates and copies an arbitrary image into `dest_dir` as `wallpaper.<ext>`,
/// replacing any previous wallpaper there (regardless of its extension).
/// Shared by both per-instance and app-wide wallpaper commands.
async fn copy_image_as_wallpaper(dest_dir: &Path, source_path: &str) -> Result<PathBuf, String> {
    let source = Path::new(source_path);
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

    remove_existing_wallpaper(dest_dir).await;

    let dest = dest_dir.join(format!("wallpaper.{ext}"));
    tokio::fs::copy(source, &dest)
        .await
        .map_err(|e| format!("Failed to copy image: {e}"))?;

    Ok(dest)
}

async fn remove_existing_wallpaper(dir: &Path) {
    for ext in ALLOWED_EXTENSIONS {
        let _ = tokio::fs::remove_file(dir.join(format!("wallpaper.{ext}"))).await;
    }
}

async fn read_wallpaper_as_data_uri(path: &Path) -> Option<String> {
    let bytes = tokio::fs::read(path).await.ok()?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_lowercase();
    Some(format!(
        "data:{};base64,{}",
        mime_for_extension(&ext),
        BASE64.encode(bytes)
    ))
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

    let dest = copy_image_as_wallpaper(Path::new(&instance.server_directory), &source_path).await?;

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

    remove_existing_wallpaper(Path::new(&instance.server_directory)).await;

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

    Ok(read_wallpaper_as_data_uri(Path::new(&path)).await)
}

/// Sets ModForge's own app-wide background (as opposed to a per-instance
/// one), stored under the app data directory rather than any instance.
#[tauri::command]
pub async fn set_app_wallpaper(state: State<'_, AppState>, source_path: String) -> Result<(), String> {
    copy_image_as_wallpaper(&state.paths.app_data_dir, &source_path).await?;
    Ok(())
}

#[tauri::command]
pub async fn clear_app_wallpaper(state: State<'_, AppState>) -> Result<(), String> {
    remove_existing_wallpaper(&state.paths.app_data_dir).await;
    Ok(())
}

#[tauri::command]
pub async fn read_app_wallpaper(state: State<'_, AppState>) -> Result<Option<String>, String> {
    for ext in ALLOWED_EXTENSIONS {
        let path = state.paths.app_data_dir.join(format!("wallpaper.{ext}"));
        if path.is_file() {
            return Ok(read_wallpaper_as_data_uri(&path).await);
        }
    }
    Ok(None)
}
