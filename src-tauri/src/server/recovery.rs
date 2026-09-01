use chrono::Utc;
use sqlx::SqlitePool;
use tauri::{AppHandle, Manager};

use super::process::set_status;
use crate::models::ServerStatus;
use crate::AppState;

/// Clears server state left behind by a previous app session.
///
/// [`super::ProcessManager`] lives only in memory, so on a fresh start
/// nothing can be running no matter what the database says. When
/// ModpackPilot goes away without its exit watcher getting to run - a power
/// cut, an OS forced shutdown, a hard kill - instance rows are left claiming
/// `starting`/`running`/`stopping`, and the app comes back permanently
/// wedged: the card renders as RUNNING (with resource usage stuck on
/// "Waiting for resource data…", since there is no process to measure), but
/// every action routes through `ProcessManager`, which has no entry for the
/// instance - so Stop, Force Stop and Restart all fail with "Instance is not
/// running", while Start refuses because the status isn't STOPPED or
/// CRASHED. There is no in-app way out of that state.
///
/// Runs once during setup, before the frontend loads and before auto-start
/// (which is subject to the same STOPPED/CRASHED check), so the first
/// `list_instances` already reflects reality.
pub async fn reconcile_stale_state(db: &SqlitePool) {
    // The launch that owned these rows ended without an exit code, so
    // `crashed` is the honest record - but the instance itself goes to
    // STOPPED, since nothing is running now and the user should just be
    // able to press Start.
    match sqlx::query(
        "UPDATE launch_history SET stopped_at = ?, status = 'crashed' WHERE stopped_at IS NULL",
    )
    .bind(Utc::now())
    .execute(db)
    .await
    {
        Ok(result) if result.rows_affected() > 0 => {
            tracing::info!(
                "Closed {} launch history row(s) left open by a previous session",
                result.rows_affected()
            );
        }
        Ok(_) => {}
        Err(e) => tracing::error!("Failed to close stale launch history rows: {e}"),
    }

    match sqlx::query("UPDATE instances SET status = 'stopped' WHERE status <> 'stopped'")
        .execute(db)
        .await
    {
        Ok(result) if result.rows_affected() > 0 => {
            tracing::warn!(
                "Reset {} instance(s) left mid-lifecycle by a previous session to STOPPED",
                result.rows_affected()
            );
        }
        Ok(_) => {}
        Err(e) => tracing::error!("Failed to reset stale instance statuses: {e}"),
    }
}

/// Same repair, for one instance, at runtime.
///
/// Backs the stop/force-stop/restart paths: if the database says an
/// instance is live but `ProcessManager` has no process for it, the
/// honest answer to "stop this" is to correct the record rather than
/// return an error the user can do nothing about.
pub async fn clear_stale_instance(app: &AppHandle, db: &SqlitePool, instance_id: &str) {
    if let Err(e) = sqlx::query(
        "UPDATE launch_history SET stopped_at = ?, status = 'crashed'
         WHERE instance_id = ? AND stopped_at IS NULL",
    )
    .bind(Utc::now())
    .bind(instance_id)
    .execute(db)
    .await
    {
        tracing::error!("Failed to close stale launch history for {instance_id}: {e}");
    }

    // Same per-instance caches the exit watcher drops when a process
    // really does exit - none of them describe anything live any more.
    let state = app.state::<AppState>();
    state.players.clear(instance_id).await;
    state.tps.clear(instance_id).await;
    state.ports.invalidate(instance_id).await;

    set_status(app, db, instance_id, ServerStatus::Stopped).await;
}
