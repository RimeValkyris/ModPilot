use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Mutex;

use super::events::{ModpackUpdateAvailablePayload, MODPACK_UPDATE_EVENT};
use crate::AppState;

/// How often a linked instance is checked against Modrinth. Deliberately
/// infrequent - a modpack publishes a new version every few weeks at most,
/// and this is a network call per linked instance.
const CHECK_EVERY: Duration = Duration::hours(6);

/// How long to wait for a graceful stop before giving up on an automatic
/// update. Deliberately generous: a big modded world can take a while to
/// save, and killing it partway through is exactly what this feature must
/// never do.
const STOP_TIMEOUT_SECS: i64 = 120;

#[derive(Default)]
pub struct UpdateCheckTracker {
    last_checked: Mutex<HashMap<String, DateTime<Utc>>>,
}

impl UpdateCheckTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether this instance is due a check, recording the attempt if so.
    async fn take_due(&self, id: &str, now: DateTime<Utc>) -> bool {
        let mut last = self.last_checked.lock().await;
        let due = last.get(id).is_none_or(|prev| now - *prev >= CHECK_EVERY);
        if due {
            last.insert(id.to_string(), now);
        }
        due
    }
}

/// Checks linked instances for new Modrinth versions and, depending on each
/// instance's `update_policy`, either announces or installs them.
pub(super) async fn tick(app: &AppHandle, now: DateTime<Utc>) {
    let rows: Vec<(String, String, String, Option<String>)> = {
        let state = app.state::<AppState>();
        match sqlx::query_as(
            "SELECT id, name, update_policy, modrinth_project_id FROM instances
             WHERE update_policy != 'off' AND modrinth_project_id IS NOT NULL",
        )
        .fetch_all(&state.db)
        .await
        {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("Auto-update query failed: {e}");
                return;
            }
        }
    };

    for (id, name, policy, _project_id) in rows {
        let due = {
            let state = app.state::<AppState>();
            state.update_checks.take_due(&id, now).await
        };
        if !due {
            continue;
        }

        let check = {
            let state = app.state::<AppState>();
            crate::commands::modrinth::check_modpack_update(state, id.clone()).await
        };
        let check = match check {
            Ok(check) => check,
            Err(e) => {
                // A failed check is routine (offline, Modrinth down) - log it
                // and try again next cycle rather than bothering the operator.
                tracing::info!("Update check for {name} failed: {e}");
                continue;
            }
        };

        let Some(latest) = check.latest_version.filter(|_| check.has_update) else {
            continue;
        };

        tracing::info!("Update available for {name}: {}", latest.version_number);
        let _ = app.emit(
            MODPACK_UPDATE_EVENT,
            ModpackUpdateAvailablePayload {
                instance_id: id.clone(),
                version_name: latest.name.clone(),
                version_number: latest.version_number.clone(),
                installing: policy == "auto",
            },
        );

        if policy != "auto" {
            notify(
                app,
                format!(
                    "{name} has a new modpack version available ({}). Open its Configuration tab to install it.",
                    latest.version_number
                ),
            )
            .await;
            continue;
        }

        if let Err(e) = apply_unattended(app, &id, &name, &latest.id, &latest.version_number).await {
            tracing::warn!("Automatic update failed for {name}: {e}");
            notify(app, format!("Automatic update for {name} failed: {e}")).await;
        }
    }
}

/// Stops the server (if running), backs the world up, installs the update,
/// and starts it again.
///
/// The order matters and is the whole point of doing this in one place:
/// swapping mod jars under a live server corrupts it, and a pack update can
/// render an existing world unloadable - so a backup is taken *every* time,
/// regardless of the instance's own backup schedule, and it is never pruned
/// by retention because it is the only way back from a bad update.
async fn apply_unattended(
    app: &AppHandle,
    id: &str,
    name: &str,
    version_id: &str,
    version_number: &str,
) -> Result<(), String> {
    let was_running = {
        let state = app.state::<AppState>();
        state.processes.is_running(id).await
    };

    if was_running {
        // Never interrupt an active session. Waiting for an empty server is
        // better than kicking people mid-game for a background chore; the
        // next cycle will find the update again.
        let players = {
            let state = app.state::<AppState>();
            state.players.list(id).await
        };
        if !players.is_empty() {
            tracing::info!(
                "Skipping automatic update for {name}: {} player(s) online",
                players.len()
            );
            return Ok(());
        }

        tracing::info!("Stopping {name} for automatic update");
        {
            let state = app.state::<AppState>();
            crate::commands::server::stop_instance(app.clone(), state, id.to_string()).await?;
        }

        let deadline = Utc::now() + Duration::seconds(STOP_TIMEOUT_SECS);
        loop {
            let running = {
                let state = app.state::<AppState>();
                state.processes.is_running(id).await
            };
            if !running {
                break;
            }
            if Utc::now() >= deadline {
                return Err("server did not shut down in time; update aborted".to_string());
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    }

    tracing::info!("Backing up {name} before automatic update");
    {
        let state = app.state::<AppState>();
        crate::commands::backup::create_world_backup(state, id.to_string()).await?;
    }

    tracing::info!("Installing {version_number} for {name}");
    {
        let state = app.state::<AppState>();
        crate::commands::modrinth::apply_modpack_update(state, id.to_string(), version_id.to_string())
            .await?;
    }

    if was_running {
        let state = app.state::<AppState>();
        crate::commands::server::start_instance(app.clone(), state, id.to_string()).await?;
    }

    notify(
        app,
        format!("{name} was updated to {version_number}. A world backup was taken first."),
    )
    .await;
    Ok(())
}

async fn notify(app: &AppHandle, message: String) {
    let enabled = {
        let state = app.state::<AppState>();
        let setting: Option<String> = sqlx::query_scalar(
            "SELECT value FROM application_settings WHERE key = 'notifications_enabled'",
        )
        .fetch_optional(&state.db)
        .await
        .unwrap_or_default();
        setting.as_deref() != Some("false")
    };
    if enabled {
        let _ = app
            .notification()
            .builder()
            .title("ModpackPilot")
            .body(message)
            .show();
    }
}
