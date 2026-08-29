use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Mutex;

use super::events::{ModpackUpdateAvailablePayload, MODPACK_UPDATE_EVENT};
use crate::AppState;

/// How often a linked instance is checked against its modpack source
/// (Modrinth or FTB). Deliberately infrequent - a modpack publishes a new
/// version every few weeks at most, and this is a network call per linked
/// instance.
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

/// Which service an instance's update was found on, and what to install.
///
/// The surrounding routine - wait for an empty server, stop it, back the
/// world up, restart afterwards - is identical for both, so only the
/// install step itself branches.
enum PendingUpdate {
    Modrinth { version_id: String },
    Ftb { version_id: i64 },
}

/// Checks linked instances for new modpack versions and, depending on each
/// instance's `update_policy`, either announces or installs them.
pub(super) async fn tick(app: &AppHandle, now: DateTime<Utc>) {
    let rows: Vec<(String, String, String, Option<String>)> = {
        let state = app.state::<AppState>();
        match sqlx::query_as(
            "SELECT id, name, update_policy, modrinth_project_id FROM instances
             WHERE update_policy != 'off'
               AND (modrinth_project_id IS NOT NULL OR ftb_pack_id IS NOT NULL)",
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

    for (id, name, policy, project_id) in rows {
        let due = {
            let state = app.state::<AppState>();
            state.update_checks.take_due(&id, now).await
        };
        if !due {
            continue;
        }

        // A Modrinth link wins when an instance somehow has both: it is the
        // one the operator linked by hand, rather than one recorded by an
        // install.
        let found = if project_id.is_some() {
            check_modrinth(app, &id, &name).await
        } else {
            check_ftb(app, &id, &name).await
        };
        let Some((display_name, version_number, pending)) = found else {
            continue;
        };

        tracing::info!("Update available for {name}: {version_number}");
        let _ = app.emit(
            MODPACK_UPDATE_EVENT,
            ModpackUpdateAvailablePayload {
                instance_id: id.clone(),
                version_name: display_name,
                version_number: version_number.clone(),
                installing: policy == "auto",
            },
        );

        if policy != "auto" {
            notify(
                app,
                format!(
                    "{name} has a new modpack version available ({version_number}). Open its Configuration tab to install it.",
                ),
            )
            .await;
            continue;
        }

        if let Err(e) = apply_unattended(app, &id, &name, &pending, &version_number).await {
            tracing::warn!("Automatic update failed for {name}: {e}");
            notify(app, format!("Automatic update for {name} failed: {e}")).await;
        }
    }
}

/// A failed check is routine (offline, the service down) - logged and
/// retried next cycle rather than bothering the operator about it.
async fn check_modrinth(
    app: &AppHandle,
    id: &str,
    name: &str,
) -> Option<(String, String, PendingUpdate)> {
    let check = {
        let state = app.state::<AppState>();
        crate::commands::modrinth::check_modpack_update(state, id.to_string()).await
    };
    let check = match check {
        Ok(check) => check,
        Err(e) => {
            tracing::info!("Update check for {name} failed: {e}");
            return None;
        }
    };

    let latest = check.latest_version.filter(|_| check.has_update)?;
    Some((
        latest.name.clone(),
        latest.version_number.clone(),
        PendingUpdate::Modrinth { version_id: latest.id },
    ))
}

async fn check_ftb(
    app: &AppHandle,
    id: &str,
    name: &str,
) -> Option<(String, String, PendingUpdate)> {
    let check = {
        let state = app.state::<AppState>();
        crate::commands::ftb::check_ftb_update(state, id.to_string()).await
    };
    let check = match check {
        Ok(check) => check,
        Err(e) => {
            tracing::info!("Update check for {name} failed: {e}");
            return None;
        }
    };

    let latest = check.latest_version.filter(|_| check.has_update)?;
    // FTB has no separate "version number" - the version's name is what
    // operators see everywhere else, so it is used for both.
    Some((
        latest.name.clone(),
        latest.name.clone(),
        PendingUpdate::Ftb { version_id: latest.id },
    ))
}

/// Stops the server (if running), installs the update, and starts it again.
///
/// The order matters and is the whole point of doing this in one place:
/// swapping mod jars under a live server corrupts it. The world backup
/// itself is taken by the apply commands (which every update path goes
/// through, manual or not), so it happens exactly once and is never pruned
/// by retention - it is the only way back from a bad update.
async fn apply_unattended(
    app: &AppHandle,
    id: &str,
    name: &str,
    pending: &PendingUpdate,
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

    tracing::info!("Installing {version_number} for {name}");
    match pending {
        PendingUpdate::Modrinth { version_id } => {
            let state = app.state::<AppState>();
            crate::commands::modrinth::apply_modpack_update(
                state,
                id.to_string(),
                version_id.clone(),
            )
            .await?;
        }
        PendingUpdate::Ftb { version_id } => {
            let state = app.state::<AppState>();
            crate::commands::ftb::apply_ftb_update(
                app.clone(),
                state,
                id.to_string(),
                *version_id,
            )
            .await?;
        }
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
