//! The Tauri surface for the diagnostic system.
//!
//! This layer does the I/O and nothing else: it gathers the instance row,
//! its Java installation, the volume it sits on, its mods folder, the tail
//! of its log and its launch history, then hands the lot to
//! [`crate::diagnostics::build`], which is where every actual judgement is
//! made. Splitting it this way is what lets the checks be tested without a
//! real server on disk.

use std::io::SeekFrom;
use std::path::Path;

use sysinfo::System;
use tauri::State;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use super::instance::fetch_instance;
use crate::diagnostics::logs::{scan, MAX_SCAN_BYTES};
use crate::diagnostics::report::{DiagnosticInputs, LaunchRecord};
use crate::diagnostics::DiagnosticReport;
use crate::models::LaunchHistoryEntry;
use crate::mods::ModpackHealth;
use crate::AppState;

/// How many launch records to consider. The crash check only looks at the
/// last 24 hours, so a few dozen rows is always more than enough and keeps
/// this from reading the whole history of a long-lived instance.
const LAUNCH_HISTORY_LIMIT: i64 = 50;

/// Runs every diagnostic check against an instance and returns the report.
///
/// Read-only. Nothing here starts, stops, or modifies anything - it is safe
/// to run against a server that is currently up, and is most useful then.
#[tauri::command]
pub async fn run_diagnostics(
    state: State<'_, AppState>,
    id: String,
) -> Result<DiagnosticReport, String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let server_directory = instance.server_directory.clone();
    let instance_dir = Path::new(&server_directory);

    // --- Java ---------------------------------------------------------------
    // The version string of whatever installation this instance is set to
    // use. `None` covers both "nothing assigned" and "the assigned
    // installation has since been removed", which the check reports as the
    // blocking problem it is.
    let java_version: Option<String> = match instance.java_installation_id.as_deref() {
        Some(java_id) => sqlx::query_scalar("SELECT version FROM java_installations WHERE id = ?")
            .bind(java_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| format!("Failed to read Java installation: {e}"))?,
        None => None,
    };

    // Needed so the check can reproduce `commands::server::resolve_java_path`'s
    // auto-selection instead of treating "nothing pinned" as "cannot start".
    let detected_java_versions: Vec<String> =
        sqlx::query_scalar("SELECT version FROM java_installations")
            .fetch_all(&state.db)
            .await
            .map_err(|e| format!("Failed to read Java installations: {e}"))?;

    // --- system + disk ------------------------------------------------------
    let system_ram_mb = tauri::async_runtime::spawn_blocking(|| {
        let mut system = System::new();
        system.refresh_memory();
        let total = system.total_memory();
        (total > 0).then_some(total / (1024 * 1024))
    })
    .await
    .unwrap_or(None);

    let disk = state.disks.sample(instance_dir).await;

    // --- mods ---------------------------------------------------------------
    let modpack: Option<ModpackHealth> = {
        let mods_dir = instance_dir.join("server").join("mods");
        let loader = instance.loader.as_str().to_string();
        let minecraft_version = instance.minecraft_version.clone();

        tauri::async_runtime::spawn_blocking(move || {
            let entries = std::fs::read_dir(&mods_dir).ok()?;
            let mut metadata = Vec::new();
            for entry in entries.flatten() {
                let file_name = entry.file_name().to_string_lossy().to_string();
                if !file_name.ends_with(".jar") && !file_name.ends_with(".jar.disabled") {
                    continue;
                }
                metadata.push(crate::mods::read_jar(&entry.path()));
            }
            metadata.sort_by(|a, b| a.file_name.cmp(&b.file_name));
            Some(crate::mods::analyze(
                metadata,
                &loader,
                minecraft_version.as_deref(),
            ))
        })
        .await
        .unwrap_or(None)
    };

    // --- log ----------------------------------------------------------------
    // Mod ids come from the scan above, so an error is only ever attributed
    // to a mod that is genuinely installed here.
    let known_mod_ids: Vec<String> = modpack
        .as_ref()
        .map(|h| h.mods.iter().filter_map(|m| m.mod_id.clone()).collect())
        .unwrap_or_default();
    // `None` (no log file) stays distinct from `Some(vec![])` (a log that
    // was read and was clean) all the way into the report.
    let log_issues = read_log_tail(instance_dir)
        .await
        .map(|log| scan(&log, &known_mod_ids));

    // --- launch history -----------------------------------------------------
    let launch_history: Vec<LaunchRecord> = sqlx::query_as::<_, (
        chrono::DateTime<chrono::Utc>,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<i64>,
        String,
    )>(
        "SELECT started_at, stopped_at, exit_code, status FROM launch_history \
         WHERE instance_id = ? ORDER BY started_at DESC LIMIT ?",
    )
    .bind(&id)
    .bind(LAUNCH_HISTORY_LIMIT)
    .fetch_all(&state.db)
    .await
    .map_err(|e| format!("Failed to read launch history: {e}"))?
    .into_iter()
    .map(|(started_at, stopped_at, exit_code, status)| LaunchRecord {
        started_at,
        stopped_at,
        exit_code,
        status,
    })
    .collect();

    Ok(crate::diagnostics::build(DiagnosticInputs {
        minecraft_version: instance.minecraft_version.clone(),
        loader: instance.loader.as_str().to_string(),
        java_version,
        detected_java_versions,
        min_ram_mb: instance.min_ram_mb,
        max_ram_mb: instance.max_ram_mb,
        system_ram_mb,
        jvm_args: instance.jvm_args.clone(),
        disk,
        modpack,
        log_issues,
        launch_history,
    }))
}

/// Reads the tail of `logs/latest.log`, or `None` if there isn't one.
///
/// The tail rather than the whole file for the same reason the console does
/// it: a long-running server's log can reach hundreds of megabytes, and
/// what explains a failure is always at the end.
async fn read_log_tail(instance_dir: &Path) -> Option<String> {
    let path = instance_dir.join("logs").join("latest.log");
    let mut file = tokio::fs::File::open(&path).await.ok()?;
    let len = file.metadata().await.ok()?.len();

    if len > MAX_SCAN_BYTES {
        file.seek(SeekFrom::Start(len - MAX_SCAN_BYTES)).await.ok()?;
    }

    // Bytes, not `read_to_string`: seeking to a byte offset can land
    // mid-character in UTF-8.
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).await.ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// How many runs to show. Enough to cover a bad afternoon of crash-restart
/// cycles without turning the card into a log of its own.
const LAUNCH_HISTORY_PAGE: i64 = 25;

/// Reads an instance's recent runs, newest first.
///
/// Separate from `run_diagnostics` because it is cheap - one indexed query,
/// no JAR scanning or log reading - so the UI can show it immediately
/// rather than only after someone asks for a full diagnostic.
#[tauri::command]
pub async fn get_launch_history(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<LaunchHistoryEntry>, String> {
    sqlx::query_as::<_, LaunchHistoryEntry>(
        "SELECT id, started_at, stopped_at, exit_code, status FROM launch_history
         WHERE instance_id = ? ORDER BY started_at DESC LIMIT ?",
    )
    .bind(&id)
    .bind(LAUNCH_HISTORY_PAGE)
    .fetch_all(&state.db)
    .await
    .map_err(|e| format!("Failed to read launch history: {e}"))
}
