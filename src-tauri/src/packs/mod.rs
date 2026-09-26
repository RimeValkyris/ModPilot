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

/// Where a pack's files are assembled before any of them are allowed near
/// the live `server/` directory.
///
/// Kept inside the instance folder rather than a system temp dir so the
/// staged copy stays on the same volume - a multi-gigabyte pack crossing
/// drives is slow, and same-volume moves are what make the commit below
/// fast enough to be worth doing at all.
pub const STAGING_DIR: &str = ".pack-staging";

/// Holds the files an in-progress commit has displaced, so a failure
/// halfway through can put them back.
const ROLLBACK_DIR: &str = ".pack-rollback";

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
    // Compared on the same segments `join_relative` will actually write
    // to, and case- and trailing-dot-insensitively: Windows treats
    // `./OPS.json.` as `ops.json`, so a plain string comparison would let a
    // pack overwrite the operator list by spelling it differently.
    let normalized = relative_segments(relative_path)
        .map(|segment| segment.trim_end_matches(['.', ' ']).to_lowercase())
        .collect::<Vec<_>>()
        .join("/");
    let world = world_folder_name.to_lowercase();
    normalized == "server.properties"
        || normalized == "whitelist.json"
        || normalized == "ops.json"
        || normalized == "banned-players.json"
        || normalized == "banned-ips.json"
        || normalized == "eula.txt"
        || normalized.starts_with(&format!("{world}/"))
}

/// Joins a `/`-separated relative path onto a base directory one segment at
/// a time, so it behaves correctly on Windows regardless of the separator
/// baked into the source string, and can't escape `base`.
pub fn join_relative(base: &Path, relative: &str) -> PathBuf {
    let mut out = base.to_path_buf();
    out.extend(relative_segments(relative));
    out
}

/// The segments of a pack-supplied relative path that are safe to join.
///
/// Drops empty, `.` and `..` segments, and any containing `:`. The last is
/// the Windows-specific one: a drive-relative segment like `C:evil` carries
/// a path prefix, and `PathBuf::push` *replaces* the whole path when given
/// one, so without it a single segment escapes `base`.
fn relative_segments(relative: &str) -> impl Iterator<Item = &str> {
    relative
        .split(['/', '\\'])
        .filter(|s| !s.is_empty() && *s != "." && *s != ".." && !s.contains(':'))
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
        // A drive-relative segment would make `push` discard `base` on Windows.
        assert_eq!(join_relative(base, "C:evil/a.jar"), base.join("a.jar"));
        assert_eq!(join_relative(base, "C:/Windows/a.jar"), base.join("Windows").join("a.jar"));
    }

    /// Windows resolves every one of these to the protected file, so a pack
    /// must not be able to reach it by spelling the name differently.
    #[test]
    fn protection_survives_alternate_spellings() {
        for path in [
            "./ops.json",
            "OPS.JSON",
            "ops.json.",
            "ops.json ",
            ".\\ops.json",
            "World/level.dat",
            "./world/region/r.0.0.mca",
        ] {
            assert!(is_protected_path(path, "world"), "should protect: {path:?}");
        }
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
    let staged = staged_relative_paths(staging, world_folder_name);

    if staged.is_empty() {
        return Err(
            "No installable files were found in this pack - it doesn't look like a server pack."
                .to_string(),
        );
    }

    let rollback_dir = instance_dir.join(ROLLBACK_DIR);
    // A leftover from a previous run would otherwise be restored over the
    // top of this one's work.
    let _ = tokio::fs::remove_dir_all(&rollback_dir).await;

    let mut installed: HashSet<String> = HashSet::new();
    // What the rollback needs to undo: files that displaced an existing one
    // (recoverable from `rollback_dir`) and files that are new (which just
    // get deleted again).
    let mut replaced: Vec<String> = Vec::new();
    let mut added: Vec<String> = Vec::new();
    let mut failure: Option<String> = None;

    for relative in &staged {
        let source = join_relative(staging, relative);
        let dest = join_relative(server_dir, relative);

        if dest.is_file() {
            let saved = join_relative(&rollback_dir, relative);
            if let Err(e) = move_file(&dest, &saved).await {
                failure = Some(e);
                break;
            }
            replaced.push(relative.clone());
        } else {
            added.push(relative.clone());
        }

        if let Err(e) = move_file(&source, &dest).await {
            failure = Some(e);
            break;
        }
        installed.insert(relative.clone());
    }

    if let Some(e) = failure {
        roll_back(server_dir, &rollback_dir, &added, &replaced).await;
        let _ = tokio::fs::remove_dir_all(&rollback_dir).await;
        return Err(format!(
            "{e} - the update was rolled back, so this instance is unchanged."
        ));
    }

    let _ = tokio::fs::remove_dir_all(&rollback_dir).await;

    prune_stale(instance_dir, server_dir, &installed).await;
    write_pack_manifest(instance_dir, &installed).await?;
    Ok(installed)
}

/// Every installable file in a staging directory, as `/`-separated paths
/// relative to it. Protected paths are dropped here so they are never even
/// considered for the commit.
fn staged_relative_paths(staging: &Path, world_folder_name: &str) -> Vec<String> {
    walkdir::WalkDir::new(staging)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| {
            let rel = entry.path().strip_prefix(staging).ok()?;
            let relative = rel.to_string_lossy().replace('\\', "/");
            (!relative.is_empty() && !is_protected_path(&relative, world_folder_name))
                .then_some(relative)
        })
        .collect()
}

/// Moves a file, falling back to copy-then-delete when a rename can't be
/// used. Staging lives on the same volume as the instance, so the rename
/// path is the one that normally runs.
async fn move_file(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
    }
    if tokio::fs::rename(from, to).await.is_ok() {
        return Ok(());
    }
    tokio::fs::copy(from, to)
        .await
        .map_err(|e| format!("Failed to write {}: {e}", to.display()))?;
    let _ = tokio::fs::remove_file(from).await;
    Ok(())
}

/// Puts the instance back the way it was after a failed commit.
///
/// Best-effort per file and deliberately so: having already hit one I/O
/// failure, the useful thing is to restore as much as possible and report
/// it, not to abandon the rest at the first stubborn file.
async fn roll_back(
    server_dir: &Path,
    rollback_dir: &Path,
    added: &[String],
    replaced: &[String],
) {
    for relative in added {
        let _ = tokio::fs::remove_file(join_relative(server_dir, relative)).await;
    }
    for relative in replaced {
        let saved = join_relative(rollback_dir, relative);
        let dest = join_relative(server_dir, relative);
        if let Err(e) = move_file(&saved, &dest).await {
            tracing::error!("Failed to restore {relative} while rolling back an update: {e}");
        }
    }
}

#[cfg(test)]
mod commit_tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn scratch(label: &str) -> Scratch {
        let dir = std::env::temp_dir()
            .join(format!("modpackpilot-commit-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("server")).expect("scratch dir");
        Scratch(dir)
    }

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent");
        }
        std::fs::write(path, contents).expect("write");
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).expect("read")
    }

    #[tokio::test]
    async fn commits_staged_files_and_leaves_live_state_alone() {
        let scratch = scratch("success");
        let instance_dir = &scratch.0;
        let server_dir = instance_dir.join("server");
        let staging = instance_dir.join(STAGING_DIR);

        write(&server_dir.join("mods/old.jar"), "old");
        write(&server_dir.join("server.properties"), "motd=mine");
        write(&server_dir.join("world/level.dat"), "save");
        // A file the previous update installed, so cleanup is allowed to
        // remove it when the new version stops shipping it.
        write_pack_manifest(instance_dir, &HashSet::from(["mods/old.jar".to_string()]))
            .await
            .expect("seed manifest");

        write(&staging.join("mods/new.jar"), "new");
        write(&staging.join("server.properties"), "motd=THE PACK");
        write(&staging.join("world/level.dat"), "PACK SAVE");

        let installed = apply_staged_pack(instance_dir, &server_dir, &staging, "world")
            .await
            .expect("commit");

        assert_eq!(installed, HashSet::from(["mods/new.jar".to_string()]));
        assert_eq!(read(&server_dir.join("mods/new.jar")), "new");
        // Live server state survives, even though the pack shipped its own.
        assert_eq!(read(&server_dir.join("server.properties")), "motd=mine");
        assert_eq!(read(&server_dir.join("world/level.dat")), "save");
        // The previous version's file is gone, since this version dropped it.
        assert!(!server_dir.join("mods/old.jar").exists());
    }

    /// The guarantee that makes a failed update survivable: if the commit
    /// dies partway, the instance is left exactly as it was rather than
    /// half-updated.
    #[tokio::test]
    async fn a_failed_commit_restores_everything_it_touched() {
        let scratch = scratch("rollback");
        let instance_dir = &scratch.0;
        let server_dir = instance_dir.join("server");
        let staging = instance_dir.join(STAGING_DIR);

        write(&server_dir.join("mods/a.jar"), "old-a");
        write(&server_dir.join("mods/b.jar"), "old-b");

        write(&staging.join("mods/a.jar"), "new-a");
        write(&staging.join("mods/b.jar"), "new-b");
        write(&staging.join("mods/c.jar"), "new-c");

        // Force the commit to fail on the last file: a directory already
        // occupies the path it needs to write to, so neither a rename nor a
        // copy can succeed.
        std::fs::create_dir_all(server_dir.join("mods/c.jar")).expect("blocker");

        let err = apply_staged_pack(instance_dir, &server_dir, &staging, "world")
            .await
            .expect_err("commit should fail");
        assert!(err.contains("rolled back"), "unhelpful error: {err}");

        // Every replaced file is back to its original contents...
        assert_eq!(read(&server_dir.join("mods/a.jar")), "old-a");
        assert_eq!(read(&server_dir.join("mods/b.jar")), "old-b");
        // ...the blocker is untouched, and no manifest was written, so the
        // next attempt still knows what the *previous* version installed.
        assert!(server_dir.join("mods/c.jar").is_dir());
        assert!(!instance_dir.join(PACK_MANIFEST_FILE).exists());
        // And the rollback scratch space cleans up after itself.
        assert!(!instance_dir.join(ROLLBACK_DIR).exists());
    }

    #[tokio::test]
    async fn an_empty_pack_is_rejected_before_anything_is_touched() {
        let scratch = scratch("empty");
        let instance_dir = &scratch.0;
        let server_dir = instance_dir.join("server");
        let staging = instance_dir.join(STAGING_DIR);
        std::fs::create_dir_all(&staging).expect("staging");
        write(&server_dir.join("mods/a.jar"), "old-a");

        let err = apply_staged_pack(instance_dir, &server_dir, &staging, "world")
            .await
            .expect_err("should reject");
        assert!(err.contains("No installable files"), "unexpected error: {err}");
        assert_eq!(read(&server_dir.join("mods/a.jar")), "old-a");
    }
}
