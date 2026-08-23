use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tauri::{AppHandle, State};
use uuid::Uuid;

use super::instance::fetch_instance;
use crate::models::ServerStatus;
use crate::server::{self, RunningProcess};
use crate::AppState;

/// How long a graceful `stop` is given to finish before `restart_instance`
/// gives up waiting and force-kills the process instead.
const GRACEFUL_STOP_TIMEOUT: Duration = Duration::from_secs(30);

/// Starts an instance's Minecraft server process.
///
/// Only valid from STOPPED or CRASHED - starting an already-running or
/// mid-transition instance is rejected rather than spawning a second
/// process on top of it.
#[tauri::command]
pub async fn start_instance(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    if state.processes.is_running(&id).await {
        return Err("Instance is already running".to_string());
    }

    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    if !matches!(instance.status, ServerStatus::Stopped | ServerStatus::Crashed) {
        return Err(format!(
            "Cannot start an instance that is currently {}",
            instance.status.as_str()
        ));
    }

    let server_jar = instance
        .server_jar
        .clone()
        .ok_or_else(|| "No server JAR configured for this instance".to_string())?;

    let working_dir = Path::new(&instance.server_directory).join("server");
    if !working_dir.join(&server_jar).is_file() {
        return Err(format!("Server JAR \"{server_jar}\" was not found in the instance's server folder"));
    }

    let java_path = resolve_java_path(&state, instance.java_installation_id.as_deref()).await?;

    let jvm_args = if instance.jvm_args.is_empty() {
        vec![
            format!("-Xms{}M", instance.min_ram_mb),
            format!("-Xmx{}M", instance.max_ram_mb),
        ]
    } else {
        instance.jvm_args.clone()
    };
    let server_args = if instance.server_args.is_empty() {
        vec!["nogui".to_string()]
    } else {
        instance.server_args.clone()
    };

    let logs_dir = Path::new(&instance.server_directory).join("logs");
    let launch_history_id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO launch_history (id, instance_id, started_at, status) VALUES (?, ?, ?, 'running')",
    )
    .bind(&launch_history_id)
    .bind(&id)
    .bind(Utc::now())
    .execute(&state.db)
    .await
    .map_err(|e| format!("Failed to record launch: {e}"))?;

    server::set_status(&app, &state.db, &id, ServerStatus::Starting).await;
    sqlx::query("UPDATE instances SET last_launched_at = ? WHERE id = ?")
        .bind(Utc::now())
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| format!("Failed to update last launched time: {e}"))?;

    let stop_requested = Arc::new(AtomicBool::new(false));

    let spawned = server::spawn_server_process(
        app.clone(),
        state.db.clone(),
        id.clone(),
        launch_history_id.clone(),
        java_path,
        jvm_args,
        server_jar,
        server_args,
        working_dir,
        logs_dir,
        stop_requested.clone(),
    )
    .await;

    let spawned = match spawned {
        Ok(spawned) => spawned,
        Err(e) => {
            server::set_status(&app, &state.db, &id, ServerStatus::Crashed).await;
            return Err(format!("Failed to launch server process: {e}"));
        }
    };

    state
        .processes
        .insert(
            id,
            RunningProcess {
                pid: spawned.pid,
                stdin: spawned.stdin,
                kill_tx: spawned.kill_tx,
                stop_requested,
                launch_history_id,
                started_at: Utc::now(),
            },
        )
        .await;

    Ok(())
}

/// Requests a graceful shutdown by sending Minecraft's own `stop` command.
/// Returns as soon as the request is sent - the instance transitions to
/// STOPPED asynchronously once the server actually exits (watch
/// `instance-status-changed`).
#[tauri::command]
pub async fn stop_instance(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.processes.mark_stopping(&id).await?;
    state.processes.write_line(&id, "stop").await?;
    server::set_status(&app, &state.db, &id, ServerStatus::Stopping).await;
    Ok(())
}

/// Immediately kills the process without giving it a chance to save.
/// Use only when a graceful stop isn't working - this can corrupt an
/// in-progress world save.
#[tauri::command]
pub async fn force_stop_instance(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.processes.mark_stopping(&id).await?;
    let kill_tx = state.processes.kill_sender(&id).await?;
    server::set_status(&app, &state.db, &id, ServerStatus::Stopping).await;
    let _ = kill_tx.send(()).await;
    Ok(())
}

/// Stops the instance (gracefully, falling back to force after a timeout)
/// and starts it again.
#[tauri::command]
pub async fn restart_instance(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    if state.processes.is_running(&id).await {
        state.processes.mark_stopping(&id).await?;
        state.processes.write_line(&id, "stop").await?;
        server::set_status(&app, &state.db, &id, ServerStatus::Stopping).await;

        let deadline = tokio::time::Instant::now() + GRACEFUL_STOP_TIMEOUT;
        while state.processes.is_running(&id).await {
            if tokio::time::Instant::now() >= deadline {
                if let Ok(kill_tx) = state.processes.kill_sender(&id).await {
                    let _ = kill_tx.send(()).await;
                }
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }

        // Give the exit watcher a moment to finish removing the map entry
        // and persisting STOPPED after the kill signal above.
        while state.processes.is_running(&id).await {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    start_instance(app, state, id).await
}

/// Returns the ids of every instance ModForge currently has a running
/// server process for. Used by the "servers are still running" close-guard.
#[tauri::command]
pub async fn list_running_instance_ids(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    Ok(state.processes.running_ids().await)
}

/// Sends an arbitrary command to a running instance's console, e.g.
/// `say Hello` or `whitelist add PlayerName`.
#[tauri::command]
pub async fn send_console_command(state: State<'_, AppState>, id: String, command: String) -> Result<(), String> {
    let command = command.trim();
    if command.is_empty() {
        return Err("Command cannot be empty".to_string());
    }
    state.processes.write_line(&id, command).await
}

async fn resolve_java_path(state: &State<'_, AppState>, java_installation_id: Option<&str>) -> Result<String, String> {
    let Some(java_id) = java_installation_id else {
        // No Java explicitly assigned - fall back to whatever "java" resolves
        // to on PATH, same as running the server from a terminal directly.
        return Ok("java".to_string());
    };

    sqlx::query_scalar::<_, String>("SELECT path FROM java_installations WHERE id = ?")
        .bind(java_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| format!("Failed to look up Java installation: {e}"))?
        .ok_or_else(|| "The Java installation assigned to this instance no longer exists".to_string())
}
