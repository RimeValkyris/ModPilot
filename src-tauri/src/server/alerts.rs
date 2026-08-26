use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Mutex;

use super::events::{ResourceAlertPayload, RESOURCE_ALERT_EVENT};
use crate::AppState;

/// How often to sample. Slower than the UI's 2s resource poll on purpose -
/// an alert is about a sustained condition, not an instantaneous spike.
const TICK: std::time::Duration = std::time::Duration::from_secs(30);

/// Fraction of an instance's configured max RAM that counts as "near the
/// limit". Below 100% because the JVM rarely reports exactly its ceiling,
/// and a server pinned this high is already GC-thrashing.
const MEMORY_HIGH_FRACTION: f64 = 0.90;

/// How long the condition must hold before alerting. A modded server
/// briefly touching its ceiling during chunk generation is normal; staying
/// there for minutes is the actual problem worth interrupting someone for.
const SUSTAINED_FOR: Duration = Duration::minutes(5);

/// Don't repeat the same alert for the same instance more often than this,
/// so a genuinely under-provisioned server doesn't spam notifications
/// every half minute for hours.
const REALERT_AFTER: Duration = Duration::minutes(60);

#[derive(Default)]
struct InstanceAlertState {
    /// When the high-memory condition was first observed continuously.
    high_since: Option<DateTime<Utc>>,
    /// When we last told the operator about it.
    last_alerted: Option<DateTime<Utc>>,
}

#[derive(Default)]
pub struct AlertTracker {
    memory: Mutex<HashMap<String, InstanceAlertState>>,
}

impl AlertTracker {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Starts the background resource-alert watcher: one task for the whole
/// app, mirroring `server::scheduler`.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(TICK).await;
            if let Err(e) = tick(&app).await {
                tracing::warn!("Resource alert tick failed: {e}");
            }
        }
    });
}

async fn tick(app: &AppHandle) -> Result<(), String> {
    let now = Utc::now();

    let snapshot = {
        let state = app.state::<AppState>();
        state.processes.running_snapshot().await
    };

    for (id, pid, started_at) in snapshot {
        let (usage, max_ram_mb, name) = {
            let state = app.state::<AppState>();
            let usage = state.resource_monitor.sample(pid, started_at).await;
            let row: Option<(i64, String)> =
                sqlx::query_as("SELECT max_ram_mb, name FROM instances WHERE id = ?")
                    .bind(&id)
                    .fetch_optional(&state.db)
                    .await
                    .map_err(|e| format!("Failed to read instance: {e}"))?;
            let Some((max_ram_mb, name)) = row else {
                continue;
            };
            (usage, max_ram_mb, name)
        };

        if max_ram_mb <= 0 {
            continue;
        }
        let is_high = (usage.memory_mb as f64) >= (max_ram_mb as f64) * MEMORY_HIGH_FRACTION;

        let should_alert = {
            let state = app.state::<AppState>();
            let mut memory = state.alerts.memory.lock().await;
            let entry = memory.entry(id.clone()).or_default();

            if !is_high {
                // Recovered - reset the clock so a later spike has to earn
                // its own sustained window rather than inheriting this one.
                entry.high_since = None;
                false
            } else {
                let since = *entry.high_since.get_or_insert(now);
                let sustained = now - since >= SUSTAINED_FOR;
                let cooled_down = entry
                    .last_alerted
                    .is_none_or(|last| now - last >= REALERT_AFTER);
                if sustained && cooled_down {
                    entry.last_alerted = Some(now);
                    true
                } else {
                    false
                }
            }
        };

        if should_alert {
            let message = format!(
                "{name} has been using {} MB of its {} MB limit for over {} minutes. Consider raising Max RAM.",
                usage.memory_mb,
                max_ram_mb,
                SUSTAINED_FOR.num_minutes(),
            );
            tracing::warn!("{message}");

            let _ = app.emit(
                RESOURCE_ALERT_EVENT,
                ResourceAlertPayload {
                    instance_id: id.clone(),
                    kind: "memory-high",
                    message: message.clone(),
                },
            );

            let notifications_enabled = {
                let state = app.state::<AppState>();
                let setting: Option<String> = sqlx::query_scalar(
                    "SELECT value FROM application_settings WHERE key = 'notifications_enabled'",
                )
                .fetch_optional(&state.db)
                .await
                .unwrap_or_default();
                setting.as_deref() != Some("false")
            };
            if notifications_enabled {
                let _ = app
                    .notification()
                    .builder()
                    .title("ModpackPilot")
                    .body(message)
                    .show();
            }
        }
    }

    Ok(())
}
