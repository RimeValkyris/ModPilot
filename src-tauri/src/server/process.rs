use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use chrono::Utc;
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::ChildStdin;
use tokio::sync::{mpsc, Mutex};

use super::events::{LogLinePayload, StatusChangedPayload, LOG_EVENT, STATUS_EVENT};
use crate::models::ServerStatus;
use crate::AppState;

/// Substring vanilla, Forge, NeoForge, Fabric, and Quilt servers all print
/// once world loading finishes and the server is ready for players/commands.
const STARTUP_COMPLETE_MARKER: &str = "Done (";

pub struct SpawnedServer {
    pub stdin: ChildStdin,
    pub kill_tx: mpsc::Sender<()>,
    pub pid: Option<u32>,
}

/// Persists a status change and tells the frontend about it. Kept small and
/// standalone (rather than reusing `commands::instance`'s helpers) so the
/// `server` module has no dependency on `commands`.
pub async fn set_status(
    app: &AppHandle,
    db: &SqlitePool,
    instance_id: &str,
    status: ServerStatus,
) {
    if let Err(e) = sqlx::query("UPDATE instances SET status = ? WHERE id = ?")
        .bind(status.as_str())
        .bind(instance_id)
        .execute(db)
        .await
    {
        tracing::error!("Failed to persist status for instance {instance_id}: {e}");
    }

    let _ = app.emit(
        STATUS_EVENT,
        StatusChangedPayload {
            instance_id: instance_id.to_string(),
            status,
        },
    );

    // Reaching RUNNING means the crash (if there was one) is behind it -
    // an instance that crashes occasionally but recovers fine shouldn't
    // stay penalized by crashes from long before.
    if status == ServerStatus::Running {
        app.state::<AppState>().crash_tracker.clear(instance_id).await;
    }

    // Only the two states worth interrupting the user for - not every
    // Starting/Stopping blip, which happens on every routine restart.
    if matches!(status, ServerStatus::Running | ServerStatus::Crashed) {
        notify_status(app, db, instance_id, status).await;
    }
}

async fn notify_status(app: &AppHandle, db: &SqlitePool, instance_id: &str, status: ServerStatus) {
    // Inlined rather than reusing `commands::settings`'s helper - this
    // module deliberately has no dependency on `commands` (see doc comment
    // on `set_status` above).
    let setting: Option<String> = sqlx::query_scalar(
        "SELECT value FROM application_settings WHERE key = 'notifications_enabled'",
    )
    .fetch_optional(db)
    .await
    .unwrap_or_default();
    if setting.as_deref() == Some("false") {
        return;
    }

    let name: Option<String> = sqlx::query_scalar("SELECT name FROM instances WHERE id = ?")
        .bind(instance_id)
        .fetch_optional(db)
        .await
        .unwrap_or_default();
    let name = name.unwrap_or_else(|| "Instance".to_string());

    let body = match status {
        ServerStatus::Running => format!("{name} finished starting and is ready."),
        ServerStatus::Crashed => format!("{name} crashed. Check its console or logs."),
        _ => return,
    };

    let _ = app
        .notification()
        .builder()
        .title("ModpackPilot")
        .body(body)
        .show();
}

/// Launches `java <jvm_args> -jar <server_jar> <server_args>` as a managed
/// child process in `working_dir`, wires up stdout/stderr capture to both
/// log files and Tauri events, and spawns the background task that detects
/// process exit.
///
/// The Minecraft process is a plain child process of ModpackPilot - never a
/// shell command string, so nothing here is vulnerable to shell injection
/// via, say, a crafted instance name or JVM argument.
#[allow(clippy::too_many_arguments)]
pub async fn spawn_server_process(
    app: AppHandle,
    db: SqlitePool,
    instance_id: String,
    launch_history_id: String,
    java_path: String,
    jvm_args: Vec<String>,
    server_jar: String,
    server_args: Vec<String>,
    working_dir: PathBuf,
    logs_dir: PathBuf,
    stop_requested: Arc<AtomicBool>,
) -> std::io::Result<SpawnedServer> {
    let mut command = tokio::process::Command::new(&java_path);
    command
        .args(&jvm_args)
        .arg("-jar")
        .arg(&server_jar)
        .args(&server_args)
        .current_dir(&working_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW - the server has its own console (ModpackPilot's),
        // it doesn't need a second native console window popping up.
        command.creation_flags(0x0800_0000);
    }

    let mut child = command.spawn()?;
    let pid = child.id();
    let stdin = child.stdin.take().expect("stdin was piped");
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");

    tokio::fs::create_dir_all(&logs_dir).await?;
    let latest_log = Arc::new(Mutex::new(
        tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(logs_dir.join("latest.log"))
            .await?,
    ));
    let dated_log = Arc::new(Mutex::new(
        tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(logs_dir.join(format!("{}.log", Utc::now().format("%Y-%m-%d"))))
            .await?,
    ));

    spawn_log_reader(
        app.clone(),
        db.clone(),
        instance_id.clone(),
        stdout,
        "stdout",
        latest_log.clone(),
        dated_log.clone(),
    );
    spawn_log_reader(
        app.clone(),
        db.clone(),
        instance_id.clone(),
        stderr,
        "stderr",
        latest_log,
        dated_log,
    );

    let (kill_tx, mut kill_rx) = mpsc::channel::<()>(1);
    tokio::spawn(async move {
        let exit_status = tokio::select! {
            status = child.wait() => status,
            _ = kill_rx.recv() => {
                let _ = child.start_kill();
                child.wait().await
            }
        };

        let was_requested = stop_requested.load(Ordering::SeqCst);
        let final_status = if was_requested {
            ServerStatus::Stopped
        } else {
            ServerStatus::Crashed
        };

        let exit_code = exit_status.ok().and_then(|s| s.code());
        if let Err(e) = sqlx::query(
            "UPDATE launch_history SET stopped_at = ?, exit_code = ?, status = ? WHERE id = ?",
        )
        .bind(Utc::now())
        .bind(exit_code)
        .bind(final_status.as_str())
        .bind(&launch_history_id)
        .execute(&db)
        .await
        {
            tracing::error!("Failed to close launch_history row: {e}");
        }

        // Must happen before `set_status`/auto-restart: `start_instance`
        // refuses to run while an entry for this id still exists, so a
        // crash-triggered auto-restart would otherwise always fail.
        app.state::<AppState>().processes.remove(&instance_id).await;

        set_status(&app, &db, &instance_id, final_status).await;

        if final_status == ServerStatus::Crashed {
            maybe_auto_restart(app, db, instance_id).await;
        }
    });

    Ok(SpawnedServer { stdin, kill_tx, pid })
}

/// If the instance that just crashed has `auto_restart` set, relaunches it
/// after a short delay - long enough to avoid hammering a server that
/// fails to start at all. Also enforces a crash-loop limit (see
/// `CrashTracker`): a server crashing repeatedly in a short window has a
/// problem a restart won't fix, so auto-restart gives up and says so
/// instead of looping forever.
///
/// Returns a boxed future (rather than being `async fn`) deliberately: this
/// calls into `commands::server::start_instance`, which calls back into
/// `spawn_server_process` above, and `async fn` return types are opaque -
/// without boxing, the compiler can't resolve the resulting auto-trait
/// cycle (`error[E0391]`) between the two functions' generated futures.
fn maybe_auto_restart(
    app: AppHandle,
    db: SqlitePool,
    instance_id: String,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(async move {
        let auto_restart: Option<i64> =
            sqlx::query_scalar("SELECT auto_restart FROM instances WHERE id = ?")
                .bind(&instance_id)
                .fetch_optional(&db)
                .await
                .unwrap_or_default();

        if auto_restart != Some(1) {
            return;
        }

        let should_restart = app
            .state::<AppState>()
            .crash_tracker
            .record_crash_and_check(&instance_id)
            .await;

        if !should_restart {
            tracing::warn!(
                "Instance {instance_id} crashed too many times in a row; auto-restart giving up"
            );
            notify_crash_loop_gave_up(&app, &db, &instance_id).await;
            return;
        }

        tracing::info!("Auto-restarting crashed instance {instance_id}");
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        let state = app.state::<AppState>();
        if let Err(e) =
            crate::commands::server::start_instance(app.clone(), state, instance_id.clone()).await
        {
            tracing::error!("Auto-restart failed for instance {instance_id}: {e}");
        }
    })
}

async fn notify_crash_loop_gave_up(app: &AppHandle, db: &SqlitePool, instance_id: &str) {
    let setting: Option<String> = sqlx::query_scalar(
        "SELECT value FROM application_settings WHERE key = 'notifications_enabled'",
    )
    .fetch_optional(db)
    .await
    .unwrap_or_default();
    if setting.as_deref() == Some("false") {
        return;
    }

    let name: Option<String> = sqlx::query_scalar("SELECT name FROM instances WHERE id = ?")
        .bind(instance_id)
        .fetch_optional(db)
        .await
        .unwrap_or_default();
    let name = name.unwrap_or_else(|| "Instance".to_string());

    let _ = app
        .notification()
        .builder()
        .title("ModpackPilot")
        .body(format!(
            "{name} has crashed repeatedly and auto-restart has stopped trying. Check its logs and start it manually once fixed."
        ))
        .show();
}

#[allow(clippy::too_many_arguments)]
fn spawn_log_reader<R>(
    app: AppHandle,
    db: SqlitePool,
    instance_id: String,
    stream: R,
    stream_name: &'static str,
    latest_log: Arc<Mutex<File>>,
    dated_log: Arc<Mutex<File>>,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(stream).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    let entry = format!("[{}] {line}\n", Utc::now().format("%H:%M:%S"));
                    {
                        let mut f = latest_log.lock().await;
                        let _ = f.write_all(entry.as_bytes()).await;
                    }
                    {
                        let mut f = dated_log.lock().await;
                        let _ = f.write_all(entry.as_bytes()).await;
                    }

                    let _ = app.emit(
                        LOG_EVENT,
                        LogLinePayload {
                            instance_id: instance_id.clone(),
                            stream: stream_name,
                            line: line.clone(),
                        },
                    );

                    if stream_name == "stdout" && line.contains(STARTUP_COMPLETE_MARKER) {
                        set_status(&app, &db, &instance_id, ServerStatus::Running).await;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    tracing::warn!("Error reading {stream_name} for instance {instance_id}: {e}");
                    break;
                }
            }
        }
    });
}
