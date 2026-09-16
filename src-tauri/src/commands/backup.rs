use std::path::Path;

use chrono::Utc;
use tauri::State;

use super::folders::detect_world_folder_name;
use super::instance::fetch_instance;
use crate::importer;
use crate::models::{BackupVerification, RestoreOutcome, WorldBackup};
use crate::AppState;

/// Headroom required beyond the archive's uncompressed size before a
/// restore is attempted. Covers filesystem overhead and leaves the server
/// somewhere to write when it next starts, rather than filling the volume
/// exactly.
const REQUIRED_FREE_MARGIN_BYTES: u64 = 1024 * 1024 * 1024;

/// Rejects a backup filename that isn't exactly what `list_world_backups`
/// would have produced - prevents a crafted name (e.g. containing `..` or a
/// path separator) from making `restore`/`delete` touch anything outside
/// the instance's own `backups/` folder.
fn validate_backup_name(name: &str) -> Result<(), String> {
    let safe = !name.is_empty()
        && name.ends_with(".zip")
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains("..");
    if safe {
        Ok(())
    } else {
        Err("Invalid backup name".to_string())
    }
}

async fn backups_dir_for(state: &State<'_, AppState>, id: &str) -> Result<std::path::PathBuf, String> {
    let instance = fetch_instance(state, id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;
    Ok(Path::new(&instance.server_directory).join("backups"))
}

async fn world_dir_for(state: &State<'_, AppState>, id: &str) -> Result<std::path::PathBuf, String> {
    let instance = fetch_instance(state, id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;
    let server_dir = Path::new(&instance.server_directory).join("server");
    Ok(server_dir.join(detect_world_folder_name(&server_dir).await))
}

/// Zips the instance's current world folder into `backups/<world>-<timestamp>.zip`.
#[tauri::command]
pub async fn create_world_backup(state: State<'_, AppState>, id: String) -> Result<String, String> {
    let world_dir = world_dir_for(&state, &id).await?;
    if !world_dir.is_dir() {
        return Err("This instance doesn't have a world folder yet".to_string());
    }

    let backups_dir = backups_dir_for(&state, &id).await?;
    tokio::fs::create_dir_all(&backups_dir)
        .await
        .map_err(|e| format!("Failed to create backups folder: {e}"))?;

    let world_name = world_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "world".to_string());
    let file_name = format!("{world_name}-{}.zip", Utc::now().format("%Y%m%d-%H%M%S"));
    let dest = backups_dir.join(&file_name);

    // Written and then read back before the backup is announced as
    // existing. A truncated or unreadable archive that still *looks* like a
    // backup in the listing is worse than no backup at all: it is only
    // discovered at the moment someone is relying on it, and it is deleted
    // here so it can never be that moment's answer.
    let verify_dest = dest.clone();
    tauri::async_runtime::spawn_blocking(move || {
        importer::create_zip_from_dir(&world_dir, &dest)?;
        importer::verify_zip(&dest)
    })
    .await
    .map_err(|e| format!("Backup task failed: {e}"))?
    .map_err(|e| {
        let _ = std::fs::remove_file(&verify_dest);
        format!("Backup failed verification and was discarded: {e}")
    })?;

    Ok(file_name)
}

/// Reads a saved backup all the way through to confirm it can actually be
/// restored, without changing anything on disk.
///
/// Separate from `list_world_backups` on purpose: listing is a directory
/// read that the UI does on every visit, while this decompresses the whole
/// archive. Tying the two together would make opening the Files tab on an
/// instance with a dozen multi-gigabyte worlds a multi-minute operation.
#[tauri::command]
pub async fn verify_world_backup(
    state: State<'_, AppState>,
    id: String,
    backup_name: String,
) -> Result<BackupVerification, String> {
    validate_backup_name(&backup_name)?;

    let backup_path = backups_dir_for(&state, &id).await?.join(&backup_name);
    if !backup_path.is_file() {
        return Err("Backup not found".to_string());
    }

    let contents = tauri::async_runtime::spawn_blocking(move || importer::verify_zip(&backup_path))
        .await
        .map_err(|e| format!("Verification task failed: {e}"))?
        .map_err(|e| format!("This backup is not readable: {e}"))?;

    Ok(BackupVerification {
        name: backup_name,
        file_count: contents.file_count,
        uncompressed_bytes: contents.uncompressed_bytes,
    })
}

/// Deletes all but the `keep_last` newest backups for an instance.
///
/// Used by scheduled backups so an automated schedule can't quietly fill a
/// disk - a large modded world zipped every few hours adds up fast. Manual
/// backups deliberately do NOT prune: someone clicking "Create Backup" is
/// making a deliberate checkpoint, and silently deleting an older one they
/// also made deliberately would be a nasty surprise.
pub(crate) async fn prune_backups(
    state: &State<'_, AppState>,
    id: &str,
    keep_last: i64,
) -> Result<(), String> {
    if keep_last <= 0 {
        return Ok(());
    }

    let backups = list_world_backups(state.clone(), id.to_string()).await?;
    let backups_dir = backups_dir_for(state, id).await?;

    for backup in backups.into_iter().skip(keep_last as usize) {
        // Re-validate even though these names came from our own listing:
        // it keeps the "nothing outside backups/ is ever deleted" guarantee
        // local to this function rather than resting on a caller's behavior.
        if validate_backup_name(&backup.name).is_err() {
            continue;
        }
        let path = backups_dir.join(&backup.name);
        if let Err(e) = tokio::fs::remove_file(&path).await {
            tracing::warn!("Failed to prune old backup {}: {e}", path.display());
        } else {
            tracing::info!("Pruned old backup {}", backup.name);
        }
    }
    Ok(())
}

/// Lists an instance's saved world backups, newest first.
#[tauri::command]
pub async fn list_world_backups(state: State<'_, AppState>, id: String) -> Result<Vec<WorldBackup>, String> {
    let backups_dir = backups_dir_for(&state, &id).await?;

    let mut entries = match tokio::fs::read_dir(&backups_dir).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("Failed to read backups folder: {e}")),
    };

    let mut backups = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| format!("Failed to read backups folder: {e}"))?
    {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".zip") {
            continue;
        }
        let Ok(metadata) = entry.metadata().await else {
            continue;
        };
        let created_at = metadata
            .modified()
            .ok()
            .map(chrono::DateTime::<Utc>::from)
            .unwrap_or_else(Utc::now);

        backups.push(WorldBackup {
            name,
            size_bytes: metadata.len(),
            created_at,
        });
    }

    backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(backups)
}

/// Restores a saved backup over the instance's world folder.
///
/// Ordered so that the existing world is never the only copy at risk:
///
/// 1. The archive is verified end to end. A backup that cannot be read is
///    rejected here, with the current world still completely untouched.
/// 2. Free space is checked against the archive's uncompressed size, so a
///    volume that cannot hold the restore fails immediately rather than
///    after decompressing for several minutes.
/// 3. It is extracted into a staging folder beside the world - same volume,
///    so the move in step 5 is a rename rather than a second full copy.
/// 4. The current world is *moved aside*, never deleted, to
///    `<world>.pre-restore-<timestamp>`.
/// 5. Staging is renamed into place. If that fails, the displaced world is
///    put straight back.
///
/// The previous implementation deleted the world first and extracted into
/// the gap, so any failure during extraction - a corrupt archive, a full
/// disk, a permissions error - destroyed the save permanently.
///
/// Refuses to run while the instance is started: restoring out from under a
/// live server would corrupt it.
#[tauri::command]
pub async fn restore_world_backup(
    state: State<'_, AppState>,
    id: String,
    backup_name: String,
) -> Result<RestoreOutcome, String> {
    validate_backup_name(&backup_name)?;

    if state.processes.is_running(&id).await {
        return Err("Stop the instance before restoring a backup".to_string());
    }

    let backup_path = backups_dir_for(&state, &id).await?.join(&backup_name);
    if !backup_path.is_file() {
        return Err("Backup not found".to_string());
    }
    let world_dir = world_dir_for(&state, &id).await?;
    let parent = world_dir
        .parent()
        .ok_or_else(|| "Instance world folder has no parent directory".to_string())?
        .to_path_buf();
    let world_name = world_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| "Instance world folder has no name".to_string())?;

    let stamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let staging = parent.join(format!(".restore-{stamp}"));
    let displaced_name = format!("{world_name}.pre-restore-{stamp}");
    let displaced = parent.join(&displaced_name);

    // 1. Prove the archive is good before anything on disk moves.
    let verify_path = backup_path.clone();
    let contents = tauri::async_runtime::spawn_blocking(move || importer::verify_zip(&verify_path))
        .await
        .map_err(|e| format!("Verification task failed: {e}"))?
        .map_err(|e| format!("This backup is not readable, so nothing was changed: {e}"))?;

    // 2. Refuse early if the volume cannot hold the restore.
    //
    // Staging the extraction beside the world is what makes the restore
    // recoverable, but it does mean the archive's contents and the existing
    // world coexist for the duration. Checking first turns "decompress 30 GB,
    // then fail" into an immediate, actionable message - and the headroom
    // matters because the displaced world is kept rather than deleted.
    if let Some(disk) = state.disks.sample(&parent).await {
        let free = disk.total_bytes.saturating_sub(disk.used_bytes);
        let needed = contents.uncompressed_bytes.saturating_add(REQUIRED_FREE_MARGIN_BYTES);
        if free < needed {
            return Err(format!(
                "Not enough free space on {} to restore safely: this backup expands to {:.1} GB                  and only {:.1} GB is free. The current world is kept alongside the restored one                  until you delete it, so a restore needs room for both.",
                disk.mount_point,
                contents.uncompressed_bytes as f64 / 1e9,
                free as f64 / 1e9,
            ));
        }
    }

    let file_count = contents.file_count;
    let outcome = tauri::async_runtime::spawn_blocking(move || -> std::io::Result<bool> {
        // 3. Extract into staging. Verbatim: a restore must reproduce the
        //    archive's own layout, not the import flow's reshaped one.
        if staging.exists() {
            std::fs::remove_dir_all(&staging)?;
        }
        std::fs::create_dir_all(&staging)?;
        if let Err(e) = importer::extract_zip_verbatim(&backup_path, &staging) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }

        // 4. Move the current world aside rather than deleting it.
        let had_world = world_dir.is_dir();
        if had_world {
            if let Err(e) = std::fs::rename(&world_dir, &displaced) {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(e);
            }
        }

        // 5. Swap staging in, and undo step 4 if that fails.
        if let Err(e) = std::fs::rename(&staging, &world_dir) {
            if had_world {
                let _ = std::fs::rename(&displaced, &world_dir);
            }
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }

        Ok(had_world)
    })
    .await
    .map_err(|e| format!("Restore task failed: {e}"))?
    .map_err(|e| format!("Failed to restore backup: {e}"))?;

    let had_world = outcome;
    if had_world {
        tracing::info!(
            "Restored {backup_name} for instance {id}; previous world kept as {displaced_name}"
        );
    } else {
        tracing::info!("Restored {backup_name} for instance {id} (no previous world)");
    }

    Ok(RestoreOutcome {
        restored_from: backup_name,
        displaced_world: had_world.then_some(displaced_name),
        file_count,
    })
}

/// Permanently deletes a saved backup.
#[tauri::command]
pub async fn delete_world_backup(
    state: State<'_, AppState>,
    id: String,
    backup_name: String,
) -> Result<(), String> {
    validate_backup_name(&backup_name)?;

    let backup_path = backups_dir_for(&state, &id).await?.join(&backup_name);
    tokio::fs::remove_file(&backup_path)
        .await
        .map_err(|e| format!("Failed to delete backup: {e}"))?;

    Ok(())
}

/// The prefix `restore_world_backup` gives a world it displaces.
const PRE_RESTORE_MARKER: &str = ".pre-restore-";

/// Rejects anything that isn't exactly a name `list_pre_restore_worlds`
/// would have produced. Same role as `validate_backup_name`: this is what
/// actually enforces that a delete can only ever reach a displaced world,
/// not an arbitrary folder - including the live world itself.
fn validate_pre_restore_name(name: &str) -> Result<(), String> {
    let safe = !name.is_empty()
        && name.contains(PRE_RESTORE_MARKER)
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains("..");
    if safe {
        Ok(())
    } else {
        Err("Invalid pre-restore folder name".to_string())
    }
}

/// Lists the worlds that previous restores moved aside, newest first.
///
/// These exist because a restore never deletes the world it replaces. That
/// is the right default, but without somewhere to see them an operator who
/// restores a few times accumulates full copies of a modded world with no
/// indication they are there - so they are surfaced and deleting them is
/// made an explicit, available action.
#[tauri::command]
pub async fn list_pre_restore_worlds(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<WorldBackup>, String> {
    let world_dir = world_dir_for(&state, &id).await?;
    let Some(parent) = world_dir.parent().map(Path::to_path_buf) else {
        return Ok(Vec::new());
    };

    let mut entries = match tokio::fs::read_dir(&parent).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("Failed to read instance folder: {e}")),
    };

    let mut found = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| format!("Failed to read instance folder: {e}"))?
    {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.contains(PRE_RESTORE_MARKER) {
            continue;
        }
        let Ok(metadata) = entry.metadata().await else {
            continue;
        };
        if !metadata.is_dir() {
            continue;
        }

        // Size is deliberately not walked: these are whole modded worlds,
        // and stat-ing every region file to render a list would stall the
        // Files tab. The creation time is what identifies which restore
        // produced it, which is what the operator is choosing between.
        found.push(WorldBackup {
            name,
            size_bytes: 0,
            created_at: metadata
                .modified()
                .ok()
                .map(chrono::DateTime::<Utc>::from)
                .unwrap_or_else(Utc::now),
        });
    }

    found.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(found)
}

/// Permanently deletes one world that a previous restore moved aside.
///
/// This really does destroy a world, so it refuses to run while the
/// instance is started and the frontend confirms it first - the same
/// treatment `restore_world_backup` gets, for the same reason.
#[tauri::command]
pub async fn delete_pre_restore_world(
    state: State<'_, AppState>,
    id: String,
    folder_name: String,
) -> Result<(), String> {
    validate_pre_restore_name(&folder_name)?;

    if state.processes.is_running(&id).await {
        return Err("Stop the instance before deleting a saved world".to_string());
    }

    let world_dir = world_dir_for(&state, &id).await?;
    let parent = world_dir
        .parent()
        .ok_or_else(|| "Instance world folder has no parent directory".to_string())?;
    let target = parent.join(&folder_name);

    // Belt and braces over the name validation: whatever the name said, the
    // resolved path must still be a direct child of the instance folder and
    // must not be the live world.
    if target.parent() != Some(parent) || target == world_dir {
        return Err("Invalid pre-restore folder name".to_string());
    }
    if !target.is_dir() {
        return Err("That saved world no longer exists".to_string());
    }

    tokio::fs::remove_dir_all(&target)
        .await
        .map_err(|e| format!("Failed to delete saved world: {e}"))?;

    tracing::info!("Deleted pre-restore world {folder_name} for instance {id}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_names_cannot_escape_the_backups_folder() {
        assert!(validate_backup_name("world-20260101-120000.zip").is_ok());
        assert!(validate_backup_name("../../etc/passwd.zip").is_err());
        assert!(validate_backup_name("sub/dir.zip").is_err());
        assert!(validate_backup_name("nope.txt").is_err());
        assert!(validate_backup_name("").is_err());
    }

    #[test]
    fn pre_restore_names_must_look_like_ones_we_created() {
        assert!(validate_pre_restore_name("world.pre-restore-20260101-120000").is_ok());
        // The live world is not a displaced copy, and must not be deletable
        // through this path.
        assert!(validate_pre_restore_name("world").is_err());
        assert!(validate_pre_restore_name("../world.pre-restore-1").is_err());
        assert!(validate_pre_restore_name("a/world.pre-restore-1").is_err());
        assert!(validate_pre_restore_name("").is_err());
    }
}
