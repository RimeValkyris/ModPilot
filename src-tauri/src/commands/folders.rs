//! Revealing ModpackPilot's managed folders and exporting log files.
//!
//! Was `commands::diagnostics`, which it never was - it opens folders and
//! copies log files out. The real diagnostic system now lives in
//! `crate::diagnostics` and `commands::diagnostics`, and having both under
//! one name was going to be a lasting source of confusion. The Tauri
//! command names are unchanged, so nothing on the frontend moved.

use tauri::State;

use super::instance::fetch_instance;
use crate::AppState;

/// Which of ModpackPilot's own folders to reveal in the file manager.
///
/// Deliberately an enum of known locations rather than a path: the
/// frontend never gets to name a path to open. Previously the UI fetched a
/// path and handed it back to the opener plugin, which meant the plugin's
/// scope had to permit any path on disk (instances can live anywhere the
/// user relocates them to). Resolving *and* opening entirely in Rust means
/// only these locations are reachable, so a compromised webview can't turn
/// "open folder" into "open anything".
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OpenTarget {
    AppLogs,
    InstanceLogs { id: String },
    InstanceSubfolder { id: String, folder: String },
}

#[tauri::command]
pub async fn open_managed_folder(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    target: OpenTarget,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    let path = match target {
        OpenTarget::AppLogs => get_app_logs_dir(state),
        OpenTarget::InstanceLogs { id } => get_instance_logs_dir(state, id).await?,
        OpenTarget::InstanceSubfolder { id, folder } => {
            get_instance_subfolder(state, id, folder).await?
        }
    };

    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| format!("Failed to open folder: {e}"))
}

/// Returns the app-level logs directory, for an "Open Logs Folder" button.
#[tauri::command]
pub fn get_app_logs_dir(state: State<'_, AppState>) -> String {
    state.paths.logs_dir.to_string_lossy().to_string()
}

/// Copies ModpackPilot's own log (including any captured panic - see the
/// `std::panic::set_hook` in `lib.rs`) to a location the user picked via a
/// native save dialog, so a crash report can actually leave the machine.
#[tauri::command]
pub async fn export_app_log(state: State<'_, AppState>, dest_path: String) -> Result<(), String> {
    let source = state
        .paths
        .logs_dir
        .join(format!("modpackpilot.log.{}", chrono::Utc::now().format("%Y-%m-%d")));

    tokio::fs::copy(&source, &dest_path)
        .await
        .map_err(|e| format!("Failed to export log: {e}"))?;

    Ok(())
}

/// Returns an instance's `logs/` directory, for an "Open Logs Folder" button.
#[tauri::command]
pub async fn get_instance_logs_dir(state: State<'_, AppState>, id: String) -> Result<String, String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    Ok(std::path::Path::new(&instance.server_directory)
        .join("logs")
        .to_string_lossy()
        .to_string())
}

/// Resolves one of an instance's well-known folders for the Files tab's
/// "Open X Folder" buttons. The path is always returned even if the
/// folder doesn't exist yet (e.g. `mods/` on a vanilla server) - opening it
/// is left to the frontend, which surfaces that failure to the user rather
/// than this command silently guessing whether it "should" exist.
#[tauri::command]
pub async fn get_instance_subfolder(
    state: State<'_, AppState>,
    id: String,
    folder: String,
) -> Result<String, String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let server_dir = std::path::Path::new(&instance.server_directory).join("server");

    let path = match folder.as_str() {
        "server" => server_dir,
        "mods" => server_dir.join("mods"),
        "config" => server_dir.join("config"),
        "world" => server_dir.join(detect_world_folder_name(&server_dir).await),
        other => return Err(format!("Unknown folder \"{other}\"")),
    };

    Ok(path.to_string_lossy().to_string())
}

/// Reads `level-name` out of `server.properties` if present, otherwise
/// falls back to Minecraft's own default world folder name.
///
/// The value is sanitized, not trusted. `server.properties` is not the
/// operator's word - it ships inside downloaded modpack ZIPs and imported
/// server folders, and the importer writes it to disk verbatim. Its value
/// is then used as a path segment by every caller here, several of which
/// rename, delete, or hand it to the OS opener, so a hostile
/// `level-name=../../../../Users/<name>/.../Startup` would turn a world
/// restore into writing attacker-chosen files into a startup folder.
///
/// This is the single choke point for all of those callers, so the check
/// lives here rather than being repeated (and eventually forgotten) at each
/// one. Anything that isn't a plain single folder name falls back to the
/// default, which is safe and matches what a server with an unusable
/// `level-name` would do anyway.
pub(crate) async fn detect_world_folder_name(server_dir: &std::path::Path) -> String {
    const DEFAULT: &str = "world";

    let Ok(contents) = tokio::fs::read_to_string(server_dir.join("server.properties")).await
    else {
        return DEFAULT.to_string();
    };

    contents
        .lines()
        .find_map(|line| line.strip_prefix("level-name="))
        .map(|name| name.trim().to_string())
        .filter(|name| is_safe_world_folder_name(name))
        .unwrap_or_else(|| DEFAULT.to_string())
}

/// Whether a `level-name` value is a plain folder name that can be safely
/// joined onto the instance directory.
///
/// Rejects, in order of how easy each is to overlook:
///
/// - Path separators and `..`, the obvious traversal.
/// - `:` - on Windows a drive-relative segment like `C:evil` carries a path
///   prefix, and `Path::join` *discards the base* when it sees one, so a
///   name with no separator at all can still escape.
/// - `.` alone, which resolves to the instance folder itself.
/// - A trailing dot or space, which Windows silently strips, letting
///   `world ` and `world` refer to one directory under two names.
fn is_safe_world_folder_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains(':')
        && !name.contains("..")
        && name != "."
        && !name.ends_with('.')
        && !name.ends_with(' ')
        && !name.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_world_names() {
        for name in ["world", "My World", "world_nether", "survival-2026"] {
            assert!(is_safe_world_folder_name(name), "should accept: {name}");
        }
    }

    /// A modpack ships its own `server.properties`, so these values are
    /// attacker-supplied rather than something the operator typed.
    #[test]
    fn rejects_anything_that_is_not_a_plain_folder_name() {
        for name in [
            "",
            ".",
            "..",
            "../../evil",
            "..\\..\\evil",
            "sub/world",
            "sub\\world",
            // Windows drive-relative: no separator, but `Path::join` still
            // throws the base away, so this escapes the instance folder.
            "C:evil",
            "C:\\Windows\\System32",
            // Windows strips these, so two names would collide.
            "world.",
            "world ",
        ] {
            assert!(!is_safe_world_folder_name(name), "should reject: {name:?}");
        }

        // Control characters, built here so the literal stays readable.
        let with_nul = format!("bad{}name", char::from(0));
        assert!(!is_safe_world_folder_name(&with_nul));
    }
}

/// Copies an instance's `logs/latest.log` to a user-chosen location -
/// the equivalent crash-log export for a server that crashed, rather than
/// ModpackPilot itself.
#[tauri::command]
pub async fn export_instance_log(
    state: State<'_, AppState>,
    id: String,
    dest_path: String,
) -> Result<(), String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let source = std::path::Path::new(&instance.server_directory)
        .join("logs")
        .join("latest.log");

    tokio::fs::copy(&source, &dest_path)
        .await
        .map_err(|e| format!("Failed to export log: {e}"))?;

    Ok(())
}
