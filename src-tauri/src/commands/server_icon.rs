use std::path::Path;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use tauri::State;

use super::instance::fetch_instance;
use crate::AppState;

/// Vanilla Minecraft only ever shows `server-icon.png` if it's exactly this
/// size - anything else is silently ignored by the client, which would
/// make "I set an icon and nothing happened" a confusing, undiagnosable
/// dead end without this check.
const REQUIRED_SIZE: u32 = 64;
const MAX_ICON_BYTES: u64 = 5 * 1024 * 1024;

/// Reads a PNG's width/height straight out of its IHDR chunk - the file
/// signature (8 bytes) is followed by a 4-byte length, 4-byte "IHDR" type,
/// then 4 bytes width + 4 bytes height, all big-endian.
fn read_png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    const PNG_SIGNATURE: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.len() < 24 || &bytes[0..8] != PNG_SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some((width, height))
}

/// Sets an instance's `server-icon.png` - the image players see in their
/// multiplayer server list. Must be a 64x64 PNG or vanilla Minecraft just
/// won't display it.
#[tauri::command]
pub async fn set_server_icon(state: State<'_, AppState>, id: String, source_path: String) -> Result<(), String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let metadata = tokio::fs::metadata(&source_path)
        .await
        .map_err(|e| format!("Failed to read image file: {e}"))?;
    if metadata.len() > MAX_ICON_BYTES {
        return Err("Image is too large".to_string());
    }

    let bytes = tokio::fs::read(&source_path)
        .await
        .map_err(|e| format!("Failed to read image file: {e}"))?;

    let (width, height) = read_png_dimensions(&bytes)
        .ok_or_else(|| "server-icon.png must be a PNG file".to_string())?;
    if width != REQUIRED_SIZE || height != REQUIRED_SIZE {
        return Err(format!(
            "Minecraft requires a {REQUIRED_SIZE}x{REQUIRED_SIZE} PNG - this image is {width}x{height}"
        ));
    }

    let dest = Path::new(&instance.server_directory)
        .join("server")
        .join("server-icon.png");
    tokio::fs::write(&dest, bytes)
        .await
        .map_err(|e| format!("Failed to save server icon: {e}"))?;

    Ok(())
}

#[tauri::command]
pub async fn clear_server_icon(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let path = Path::new(&instance.server_directory)
        .join("server")
        .join("server-icon.png");
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Failed to remove server icon: {e}")),
    }
}

#[tauri::command]
pub async fn read_server_icon(state: State<'_, AppState>, id: String) -> Result<Option<String>, String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let path = Path::new(&instance.server_directory)
        .join("server")
        .join("server-icon.png");

    match tokio::fs::read(&path).await {
        Ok(bytes) => Ok(Some(format!("data:image/png;base64,{}", BASE64.encode(bytes)))),
        Err(_) => Ok(None),
    }
}
