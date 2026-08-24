use std::collections::HashMap;
use std::path::Path;

use tauri::State;

use super::instance::fetch_instance;
use crate::AppState;

/// The only keys the Configuration tab's form edits. Restricting to a known
/// list (rather than accepting arbitrary key/value pairs from the frontend)
/// means a bug or bad input can't rewrite something else in the file.
const EDITABLE_KEYS: &[&str] = &[
    "motd",
    "max-players",
    "difficulty",
    "gamemode",
    "pvp",
    "online-mode",
    "white-list",
    "hardcore",
    "view-distance",
    "spawn-protection",
];

fn is_editable(key: &str) -> bool {
    EDITABLE_KEYS.contains(&key)
}

/// Reads just the editable keys' current values out of `server.properties`.
/// A key that isn't present in the file yet (e.g. before the server has
/// ever started once to generate its defaults) is simply absent from the map.
#[tauri::command]
pub async fn read_server_properties(
    state: State<'_, AppState>,
    id: String,
) -> Result<HashMap<String, String>, String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let path = Path::new(&instance.server_directory)
        .join("server")
        .join("server.properties");

    let contents = match tokio::fs::read_to_string(&path).await {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => return Err(format!("Failed to read server.properties: {e}")),
    };

    let mut values = HashMap::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            if is_editable(key.trim()) {
                values.insert(key.trim().to_string(), value.trim().to_string());
            }
        }
    }

    Ok(values)
}

/// Updates only the given keys in `server.properties`, leaving every other
/// line (comments, ordering, keys this form doesn't know about) untouched.
/// A key that doesn't already exist in the file is appended; the file is
/// created if it doesn't exist yet.
///
/// Like most server.properties edits, most of these only take effect on
/// the next start - this doesn't attempt to push a live `/reload` or
/// restart anything.
#[tauri::command]
pub async fn write_server_properties(
    state: State<'_, AppState>,
    id: String,
    updates: HashMap<String, String>,
) -> Result<(), String> {
    for key in updates.keys() {
        if !is_editable(key) {
            return Err(format!("\"{key}\" is not an editable setting"));
        }
    }

    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let path = Path::new(&instance.server_directory)
        .join("server")
        .join("server.properties");

    let existing = tokio::fs::read_to_string(&path).await.unwrap_or_default();
    let mut remaining = updates.clone();
    let mut lines: Vec<String> = Vec::new();

    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            lines.push(line.to_string());
            continue;
        }
        match trimmed.split_once('=') {
            Some((key, _)) if remaining.contains_key(key.trim()) => {
                let key = key.trim().to_string();
                let value = remaining.remove(&key).unwrap();
                lines.push(format!("{key}={value}"));
            }
            _ => lines.push(line.to_string()),
        }
    }

    // Anything not already present in the file gets appended.
    for (key, value) in remaining {
        lines.push(format!("{key}={value}"));
    }

    tokio::fs::write(&path, lines.join("\n") + "\n")
        .await
        .map_err(|e| format!("Failed to write server.properties: {e}"))?;

    Ok(())
}
