//! Records performance history to the database.
//!
//! Deliberately records the *same* numbers the dashboard shows, by going
//! through the same collection path the frontend polls rather than
//! reimplementing it. Two sampling paths would eventually disagree, and a
//! history that contradicts the live view is worse than no history - it
//! makes the operator doubt both.
//!
//! The one deliberate exception is CPU, which is sampled through this
//! task's own [`crate::server::ResourceMonitor`]: `sysinfo` computes CPU
//! percentage from the delta between refreshes of one `System`, so sharing
//! a monitor would have the two tasks corrupting each other's intervals.
//!
//! The cadence is the whole design decision. The dashboard polls every two
//! seconds because a person is watching it; history is written once a
//! minute because a trend does not need that resolution and a year of
//! 2-second samples would be 15 million rows per server.

use chrono::{Duration, Utc};
use tauri::{AppHandle, Manager};

use crate::commands::monitor::collect_all_resource_usage;
use crate::commands::settings::get_setting;
use crate::AppState;

/// How often a sample is written, per running instance.
const TICK: std::time::Duration = std::time::Duration::from_secs(60);

/// How often old samples are deleted. Hourly rather than every tick: the
/// retention boundary moves continuously, but nothing depends on it being
/// enforced to the minute, and this is a write to a table that is otherwise
/// append-only.
const PRUNE_EVERY_TICKS: u32 = 60;

/// How long history is kept when the operator hasn't chosen.
///
/// Two weeks answers "did this get worse after the update I did last
/// Tuesday?", which is the question this data exists for, while keeping a
/// busy multi-server install in the low hundreds of thousands of rows.
pub const DEFAULT_RETENTION_DAYS: i64 = 14;

/// The settings key holding the retention window in days. `0` means keep
/// everything, matching how `backup_keep_last` treats zero.
pub const RETENTION_SETTING_KEY: &str = "performance_retention_days";

/// Starts the background history recorder: one task for the whole app,
/// mirroring `server::scheduler` and `server::alerts`.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut ticks: u32 = 0;
        loop {
            tokio::time::sleep(TICK).await;
            ticks = ticks.wrapping_add(1);

            if let Err(e) = record(&app).await {
                tracing::warn!("Performance history tick failed: {e}");
            }
            if ticks.is_multiple_of(PRUNE_EVERY_TICKS) {
                if let Err(e) = prune(&app).await {
                    tracing::warn!("Performance history prune failed: {e}");
                }
            }
        }
    });
}

async fn record(app: &AppHandle) -> Result<(), String> {
    let usage_by_id = {
        let state = app.state::<AppState>();
        // Its own sampler, not the dashboard's - see
        // `collect_all_resource_usage`. Everything but CPU still comes from
        // the same shared sources, so the two views agree.
        collect_all_resource_usage(&state, &state.history_monitor).await?
    };
    if usage_by_id.is_empty() {
        return Ok(());
    }

    let recorded_at = Utc::now();
    let state = app.state::<AppState>();

    for (instance_id, usage) in usage_by_id {
        if !usage.is_running {
            continue;
        }

        // The ping's count is authoritative once the server answers; the
        // console-derived roster covers the window before it accepts
        // connections. Same precedence the dashboard uses.
        let players = usage
            .ping
            .as_ref()
            .and_then(|p| p.players_online)
            .unwrap_or(usage.players_tracked);

        let result = sqlx::query(
            "INSERT INTO performance_samples
             (instance_id, recorded_at, cpu_percent, memory_mb, tps, mspt, players, ping_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&instance_id)
        .bind(recorded_at)
        .bind(usage.cpu_percent as f64)
        .bind(usage.memory_mb)
        // Null, not zero, for anything this server cannot report - see the
        // migration's note on why that distinction is load-bearing.
        .bind(usage.tps.map(|v| v as f64))
        .bind(usage.mspt.map(|v| v as f64))
        .bind(players as i64)
        .bind(usage.ping.as_ref().map(|p| p.latency_ms as i64))
        .execute(&state.db)
        .await;

        if let Err(e) = result {
            tracing::warn!("Failed to record performance sample for {instance_id}: {e}");
        }
    }

    Ok(())
}

/// Deletes samples past the retention window.
async fn prune(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();

    let days = get_setting(&state.db, RETENTION_SETTING_KEY)
        .await
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_RETENTION_DAYS);

    // Zero means keep everything - an explicit choice, so it is honored.
    if days <= 0 {
        return Ok(());
    }

    let cutoff = Utc::now() - Duration::days(days);
    let deleted = sqlx::query("DELETE FROM performance_samples WHERE recorded_at < ?")
        .bind(cutoff)
        .execute(&state.db)
        .await
        .map_err(|e| format!("Failed to prune performance history: {e}"))?
        .rows_affected();

    if deleted > 0 {
        tracing::info!("Pruned {deleted} performance sample(s) older than {days} days");
    }
    Ok(())
}
