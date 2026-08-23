use std::io::SeekFrom;

use tauri::State;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use super::instance::fetch_instance;
use crate::AppState;

/// Caps how much of a (potentially huge, long-running) log file gets read
/// into memory for the console's initial scrollback - only the tail
/// matters for "what's been happening", not the entire session history.
const MAX_TAIL_BYTES: u64 = 512 * 1024;

/// Reads the tail of an instance's `logs/latest.log`, so opening its
/// console shows recent output immediately instead of starting blank until
/// the next line arrives.
#[tauri::command]
pub async fn read_latest_log(state: State<'_, AppState>, id: String) -> Result<String, String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let log_path = std::path::Path::new(&instance.server_directory)
        .join("logs")
        .join("latest.log");

    let mut file = match tokio::fs::File::open(&log_path).await {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(e) => return Err(format!("Failed to open log file: {e}")),
    };

    let len = file
        .metadata()
        .await
        .map_err(|e| format!("Failed to read log file: {e}"))?
        .len();

    if len > MAX_TAIL_BYTES {
        file.seek(SeekFrom::Start(len - MAX_TAIL_BYTES))
            .await
            .map_err(|e| format!("Failed to read log file: {e}"))?;
    }

    // Read as bytes rather than `read_to_string`: seeking to an arbitrary
    // byte offset for the tail can land mid multi-byte UTF-8 character.
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)
        .await
        .map_err(|e| format!("Failed to read log file: {e}"))?;

    Ok(String::from_utf8_lossy(&buf).into_owned())
}
