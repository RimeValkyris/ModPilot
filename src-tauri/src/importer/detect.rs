use std::path::Path;

use regex::Regex;

use crate::models::{DetectedServerInfo, ServerLoader};

const START_SCRIPT_NAMES: &[&str] = &[
    "start.sh",
    "start.bat",
    "run.sh",
    "run.bat",
    "startserver.sh",
    "startserver.bat",
    "launch.sh",
    "launch.bat",
];

/// If every file in the list lives under one shared top-level folder - and
/// none sit directly at the root - returns that folder's name. Many
/// modpack export tools zip (or fold into an extracted folder) a server
/// pack wrapped in a directory matching the pack's name, e.g.
/// `MyServerPack-1.0/mods/...`, `MyServerPack-1.0/server.properties`. Without
/// unwrapping that, every fixed path the rest of the app relies on
/// (`server/mods`, `server/server.properties`, the server JAR itself) ends
/// up one directory too deep for anything to find - the server still gets
/// imported, but shows up looking empty (no mods, no detected JAR).
pub(crate) fn detect_wrapper_folder(file_paths: &[String]) -> Option<String> {
    if file_paths.is_empty() {
        return None;
    }
    let mut wrapper: Option<&str> = None;
    for path in file_paths {
        let (first, _rest) = path.split_once('/')?;
        if first.is_empty() {
            return None;
        }
        match wrapper {
            None => wrapper = Some(first),
            Some(existing) if existing == first => {}
            _ => return None,
        }
    }
    wrapper.map(str::to_string)
}

fn strip_wrapper_folder(file_paths: Vec<String>) -> (Vec<String>, Option<String>) {
    match detect_wrapper_folder(&file_paths) {
        Some(wrapper) => {
            let prefix = format!("{wrapper}/");
            let stripped = file_paths
                .into_iter()
                .map(|p| p.strip_prefix(&prefix).unwrap_or(&p).to_string())
                .collect();
            (stripped, Some(wrapper))
        }
        None => (file_paths, None),
    }
}

/// Runs detection against a real, already-extracted directory (either a
/// freshly-imported instance's `server/` folder, or a folder the user
/// picked directly for "import from folder").
pub fn detect_from_dir(root: &Path) -> DetectedServerInfo {
    let mut file_paths = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.file_type().is_file() {
            if let Ok(rel) = entry.path().strip_prefix(root) {
                file_paths.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let (file_paths, wrapper) = strip_wrapper_folder(file_paths);
    let mut info = analyze(&file_paths);
    if let Some(wrapper) = wrapper {
        info.warnings.insert(
            0,
            format!("Removed wrapping folder \"{wrapper}\" - its contents were treated as the server root."),
        );
    }
    info
}

/// Runs detection against a ZIP archive's file listing, without extracting
/// anything - used for the importer wizard's "review before importing"
/// step, so a multi-gigabyte modpack doesn't have to be unpacked twice.
pub fn detect_from_zip(zip_path: &Path) -> std::io::Result<DetectedServerInfo> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let mut file_paths = Vec::new();
    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            if !entry.is_dir() {
                file_paths.push(entry.name().replace('\\', "/"));
            }
        }
    }

    let (file_paths, wrapper) = strip_wrapper_folder(file_paths);
    let mut info = analyze(&file_paths);
    if let Some(wrapper) = wrapper {
        info.warnings.insert(
            0,
            format!("Removed wrapping folder \"{wrapper}\" - its contents were treated as the server root."),
        );
    }
    Ok(info)
}

/// Shared heuristics: given every file's path (forward-slash, relative to
/// the server root), guess loader/version/JAR/mods/world/etc.
///
/// This is deliberately best-effort. Server layouts vary a lot in the wild,
/// so anything not clearly identifiable is left as `None`/`false` plus a
/// warning, rather than guessed with false confidence.
fn analyze(file_paths: &[String]) -> DetectedServerInfo {
    let mut info = DetectedServerInfo::default();
    let lower_paths: Vec<String> = file_paths.iter().map(|p| p.to_lowercase()).collect();

    let root_jars: Vec<&str> = file_paths
        .iter()
        .zip(&lower_paths)
        .filter(|(_, lower)| !lower.contains('/') && lower.ends_with(".jar"))
        .map(|(original, _)| original.as_str())
        .collect();

    info.has_mods_folder = lower_paths.iter().any(|p| p.starts_with("mods/"));
    info.mod_count = lower_paths
        .iter()
        .filter(|p| p.starts_with("mods/") && p.ends_with(".jar"))
        .count();

    info.has_config_folder = lower_paths.iter().any(|p| p.starts_with("config/"));

    info.has_server_properties = lower_paths.iter().any(|p| p == "server.properties");

    info.start_scripts = file_paths
        .iter()
        .zip(&lower_paths)
        .filter(|(_, lower)| !lower.contains('/') && START_SCRIPT_NAMES.contains(&lower.as_str()))
        .map(|(original, _)| original.clone())
        .collect();

    // A world folder is recognized by a `level.dat` sitting exactly one
    // directory deep - i.e. `<worldname>/level.dat`.
    for (original, lower) in file_paths.iter().zip(&lower_paths) {
        if lower.ends_with("/level.dat") {
            if let Some((folder, _)) = original.split_once('/') {
                info.has_world_folder = true;
                info.world_folder_name = Some(folder.to_string());
                break;
            }
        }
    }

    detect_loader_and_version(&lower_paths, &root_jars, &mut info);

    if info.server_jar.is_none() && info.start_scripts.is_empty() {
        info.warnings.push(
            "No server JAR or start script detected. You'll need to configure this instance manually.".to_string(),
        );
    } else if info.server_jar.is_none() && !info.start_scripts.is_empty() {
        info.warnings.push(
            "No single server JAR found; this loader likely launches via its start script instead.".to_string(),
        );
    }

    if !info.has_server_properties {
        info.warnings
            .push("No server.properties found.".to_string());
    }

    info
}

fn detect_loader_and_version(
    lower_paths: &[String],
    root_jars: &[&str],
    info: &mut DetectedServerInfo,
) {
    let any_path_contains = |needle: &str| lower_paths.iter().any(|p| p.contains(needle));

    if any_path_contains("neoforge") {
        info.loader = ServerLoader::NeoForge;
        let re = Regex::new(r"neoforge-([\d.]+)").unwrap();
        info.loader_version = find_capture(lower_paths, &re, 1);
    } else if any_path_contains("minecraftforge") || any_path_contains("forge-") {
        info.loader = ServerLoader::Forge;
        let re = Regex::new(r"forge-(\d+\.\d+(?:\.\d+)?)-([\d.]+)").unwrap();
        if let Some(path) = lower_paths.iter().find(|p| re.is_match(p)) {
            if let Some(caps) = re.captures(path) {
                info.minecraft_version = Some(caps[1].to_string());
                info.loader_version = Some(caps[2].to_string());
            }
        }
    } else if any_path_contains("fabric") {
        info.loader = ServerLoader::Fabric;
        let re = Regex::new(r"fabric-server-mc\.([\d.]+)-loader\.([\d.]+)").unwrap();
        if let Some(path) = lower_paths.iter().find(|p| re.is_match(p)) {
            if let Some(caps) = re.captures(path) {
                info.minecraft_version = Some(caps[1].to_string());
                info.loader_version = Some(caps[2].to_string());
            }
        }
    } else if any_path_contains("quilt") {
        info.loader = ServerLoader::Quilt;
    } else if root_jars.iter().any(|j| j.to_lowercase() == "server.jar")
        || any_path_contains("minecraft_server")
    {
        info.loader = ServerLoader::Vanilla;
        let re = Regex::new(r"minecraft_server\.([\d.]+)\.jar").unwrap();
        info.minecraft_version = find_capture(lower_paths, &re, 1);
    }

    info.server_jar = pick_server_jar(root_jars, info.loader);
}

fn find_capture(paths: &[String], re: &Regex, group: usize) -> Option<String> {
    paths
        .iter()
        .find_map(|p| re.captures(p).and_then(|c| c.get(group)).map(|m| m.as_str().to_string()))
}

/// Picks the most likely launchable JAR from the server root, preferring
/// well-known names for the detected loader over an arbitrary guess.
fn pick_server_jar(root_jars: &[&str], loader: ServerLoader) -> Option<String> {
    if root_jars.is_empty() {
        return None;
    }

    let preferred = match loader {
        ServerLoader::Vanilla => root_jars.iter().find(|j| j.to_lowercase() == "server.jar"),
        ServerLoader::Forge => root_jars.iter().find(|j| j.to_lowercase().contains("forge")),
        ServerLoader::NeoForge => root_jars
            .iter()
            .find(|j| j.to_lowercase().contains("neoforge")),
        ServerLoader::Fabric => root_jars.iter().find(|j| j.to_lowercase().contains("fabric")),
        ServerLoader::Quilt => root_jars.iter().find(|j| j.to_lowercase().contains("quilt")),
        ServerLoader::Unknown => None,
    };

    preferred.or(root_jars.first()).map(|s| s.to_string())
}
