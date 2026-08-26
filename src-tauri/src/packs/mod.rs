//! Applying a modpack's files onto an existing instance.
//!
//! Shared by both update paths - a Modrinth `.mrpack` and a plain local
//! ZIP/folder - because the dangerous parts are identical either way:
//! deciding what must never be overwritten, and knowing which files the
//! previous version installed so stale ones can be cleaned up.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Records exactly which relative paths the last-applied pack wrote.
///
/// This is what makes safe cleanup possible: on the next update, anything
/// this manifest claims but the new version no longer contains was
/// installed by ModpackPilot and can be removed, while anything never
/// recorded here (a mod the operator dropped in by hand) is left alone,
/// because it was never ours to manage.
const PACK_MANIFEST_FILE: &str = ".modpackpilot-pack-files.json";

/// The pre-rename filename, still read so an instance last updated by an
/// older build keeps its cleanup history instead of silently losing track
/// of every file it had installed.
const LEGACY_PACK_MANIFEST_FILE: &str = ".modrinth-pack-files.json";

pub async fn read_pack_manifest(instance_dir: &Path) -> HashSet<String> {
    for name in [PACK_MANIFEST_FILE, LEGACY_PACK_MANIFEST_FILE] {
        if let Ok(contents) = tokio::fs::read_to_string(instance_dir.join(name)).await {
            if let Ok(files) = serde_json::from_str(&contents) {
                return files;
            }
        }
    }
    HashSet::new()
}

pub async fn write_pack_manifest(
    instance_dir: &Path,
    files: &HashSet<String>,
) -> Result<(), String> {
    let json = serde_json::to_string(files).unwrap_or_else(|_| "[]".to_string());
    tokio::fs::write(instance_dir.join(PACK_MANIFEST_FILE), json)
        .await
        .map_err(|e| format!("Update installed, but failed to save its file manifest: {e}"))?;
    // Drop the old-named file so the two can't drift apart later.
    let _ = tokio::fs::remove_file(instance_dir.join(LEGACY_PACK_MANIFEST_FILE)).await;
    Ok(())
}

/// Paths that belong to the running server, not the modpack, and must
/// survive an update untouched: the world save, player-list state, and the
/// operator's own `server.properties`.
///
/// A pack ships its author's *recommended* defaults for these, but blindly
/// overwriting a live server's actual state on every update would silently
/// undo whitelist/op/ban changes and settings the operator chose
/// deliberately - and replacing the world folder would destroy the save.
pub fn is_protected_path(relative_path: &str, world_folder_name: &str) -> bool {
    let normalized = relative_path.replace('\\', "/");
    normalized == "server.properties"
        || normalized == "whitelist.json"
        || normalized == "ops.json"
        || normalized == "banned-players.json"
        || normalized == "banned-ips.json"
        || normalized == "eula.txt"
        || normalized.starts_with(&format!("{world_folder_name}/"))
}

/// Joins a `/`-separated relative path onto a base directory one segment at
/// a time, so it behaves correctly on Windows regardless of the separator
/// baked into the source string, and can't escape `base`.
pub fn join_relative(base: &Path, relative: &str) -> PathBuf {
    let mut out = base.to_path_buf();
    out.extend(
        relative
            .split('/')
            .filter(|s| !s.is_empty() && *s != "." && *s != ".."),
    );
    out
}

/// Removes files the previous pack version installed that this one didn't.
///
/// Best-effort per file: a removal that fails (the server still holds the
/// jar open, say) is logged rather than failing the whole update, since the
/// new files are already in place by this point.
pub async fn prune_stale(instance_dir: &Path, server_dir: &Path, installed: &HashSet<String>) {
    let previous = read_pack_manifest(instance_dir).await;
    for stale in previous.difference(installed) {
        let path = join_relative(server_dir, stale);
        if path.is_file() {
            if let Err(e) = tokio::fs::remove_file(&path).await {
                tracing::warn!("Failed to remove stale pack file {}: {e}", path.display());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protects_live_server_state() {
        assert!(is_protected_path("server.properties", "world"));
        assert!(is_protected_path("whitelist.json", "world"));
        assert!(is_protected_path("eula.txt", "world"));
        assert!(is_protected_path("world/level.dat", "world"));
        // A non-default world folder name is honored.
        assert!(is_protected_path("RAD BBOP/level.dat", "RAD BBOP"));
        assert!(!is_protected_path("world/level.dat", "RAD BBOP"));
        // Pack content is not protected - it is what gets replaced.
        assert!(!is_protected_path("mods/jei.jar", "world"));
        assert!(!is_protected_path("config/foo.toml", "world"));
    }

    #[test]
    fn join_relative_cannot_escape_base() {
        let base = Path::new("/srv/instance");
        assert_eq!(join_relative(base, "mods/a.jar"), base.join("mods").join("a.jar"));
        // Traversal segments are dropped rather than honored.
        assert_eq!(join_relative(base, "../../etc/passwd"), base.join("etc").join("passwd"));
        assert_eq!(join_relative(base, "./mods/./a.jar"), base.join("mods").join("a.jar"));
    }
}

/// Copies an already-extracted pack from `staging` onto an instance's
/// `server/` directory, then removes files the previous pack version had
/// installed that this one no longer contains.
///
/// This is the local-file counterpart to `modrinth::apply_update`, and it
/// deliberately shares that path's guarantees: `is_protected_path` entries
/// (the world, `server.properties`, whitelist/ops/bans, `eula.txt`) are
/// skipped entirely, and cleanup only ever touches files this app recorded
/// installing - a mod dropped in by hand is never removed.
///
/// Returns the set of relative paths installed, already persisted to the
/// manifest.
pub async fn apply_staged_pack(
    instance_dir: &Path,
    server_dir: &Path,
    staging: &Path,
    world_folder_name: &str,
) -> Result<HashSet<String>, String> {
    let mut installed: HashSet<String> = HashSet::new();

    for entry in walkdir::WalkDir::new(staging).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(staging) else {
            continue;
        };
        let relative = rel.to_string_lossy().replace('\\', "/");
        if relative.is_empty() || is_protected_path(&relative, world_folder_name) {
            continue;
        }

        let dest = join_relative(server_dir, &relative);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
        }
        tokio::fs::copy(entry.path(), &dest)
            .await
            .map_err(|e| format!("Failed to write {}: {e}", dest.display()))?;
        installed.insert(relative);
    }

    if installed.is_empty() {
        return Err(
            "That file doesn't look like a server pack - no installable files were found in it."
                .to_string(),
        );
    }

    prune_stale(instance_dir, server_dir, &installed).await;
    write_pack_manifest(instance_dir, &installed).await?;
    Ok(installed)
}
