use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::ChildStdin;
use tokio::sync::{mpsc, Mutex};

use super::events::{
    LogLinePayload, PlayersChangedPayload, StatusChangedPayload, StuckStartingPayload, LOG_EVENT,
    PLAYERS_EVENT, STATUS_EVENT, STUCK_STARTING_EVENT,
};
use crate::models::ServerStatus;
use crate::AppState;

/// How long a "starting" instance can produce zero stdout/stderr lines
/// before it's flagged as possibly stuck (see `watch_for_stuck_startup`).
/// Real mod-heavy packs can legitimately go quiet for a while during
/// CPU-bound init work, but a genuine hang (e.g. a mod's update-checker
/// stuck on a dead network connection with no timeout) produces total
/// silence indefinitely - this is long enough to avoid flagging a merely-
/// slow pack, short enough to catch a real hang long before an operator
/// would otherwise notice on their own.
const STUCK_STARTING_THRESHOLD: Duration = Duration::from_secs(3 * 60);

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

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

/// Builds the command for `"script"` launch mode, where the pack ships a
/// `run.bat`/`run.sh` and nothing this could parse a jar out of (see
/// `importer::script`). The script is run through its shell, in the server
/// directory, with stdio piped exactly as a direct Java launch would be -
/// a batch file forwards its own stdin/stdout to the Java process it
/// starts, so the console and the graceful `stop` command still work.
///
/// The script decides its own RAM and JVM flags, so the instance's
/// `jvm_args` are deliberately not applied here; what *is* applied is the
/// instance's Java, put ahead of everything else on `PATH` (plus
/// `JAVA_HOME`) so a script calling a bare `java` gets the same runtime
/// every other launch mode would have used.
fn script_command(script: &str, java_path: &str) -> tokio::process::Command {
    #[cfg(windows)]
    let mut command = {
        let mut command = tokio::process::Command::new("cmd");
        // `/C` rather than a bare invocation: `.bat` files are not
        // executable images, only cmd.exe can run one.
        command.arg("/C").arg(script);
        command
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut command = tokio::process::Command::new("sh");
        command.arg(format!("./{script}"));
        // Without its own process group, the shell's children survive a
        // force-stop - see `kill_process_tree`.
        command.process_group(0);
        command
    };

    let java_bin = std::path::Path::new(java_path).parent();
    if let Some(bin) = java_bin {
        if let Some(home) = bin.parent() {
            command.env("JAVA_HOME", home);
        }
        let existing = std::env::var_os("PATH").unwrap_or_default();
        let mut entries = vec![bin.to_path_buf()];
        entries.extend(std::env::split_paths(&existing));
        if let Ok(joined) = std::env::join_paths(entries) {
            command.env("PATH", joined);
        }
    }

    command
}

/// Force-kills a process and everything it spawned. Only needed for
/// `"script"` mode, where the tracked child is a shell standing between
/// ModpackPilot and the actual server.
async fn kill_process_tree(pid: u32) {
    #[cfg(windows)]
    let mut killer = {
        let mut killer = tokio::process::Command::new("taskkill");
        killer.args(["/T", "/F", "/PID", &pid.to_string()]);
        killer.creation_flags(0x0800_0000);
        killer
    };
    // The shell was started in its own process group, so a negative PID
    // signals the group - the server included.
    #[cfg(not(windows))]
    let mut killer = {
        let mut killer = tokio::process::Command::new("kill");
        killer.args(["-9", &format!("-{pid}")]);
        killer
    };

    if let Err(e) = killer.stdout(Stdio::null()).stderr(Stdio::null()).status().await {
        tracing::warn!("Couldn't kill process tree for pid {pid}: {e}");
    }
}

/// Launches the Minecraft server as a managed child process in
/// `working_dir`, wires up stdout/stderr capture to both log files and
/// Tauri events, and spawns the background task that detects process exit.
///
/// `launch_mode` controls how the command line is built:
/// - `"jar"` (everything except modern Forge/NeoForge): plain
///   `java <jvm_args> -jar <server_jar> <server_args>`.
/// - `"argfile"`: `server_jar` is actually the path to a Forge/NeoForge
///   `@`-argfile (see `importer::detect::find_loader_argfile`), which
///   already encodes the real main class - our own `jvm_args` (RAM, etc.)
///   have to be placed *before* it to still be read as JVM options rather
///   than program arguments, and *after* `@user_jvm_args.txt` so they take
///   priority over that file's own defaults.
/// - `"script"`: `server_jar` is the pack's own `run.bat`/`run.sh`, run
///   through its shell (see `script_command`) because nothing launchable
///   could be identified any other way.
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
    launch_mode: String,
    server_args: Vec<String>,
    working_dir: PathBuf,
    logs_dir: PathBuf,
    stop_requested: Arc<AtomicBool>,
) -> std::io::Result<SpawnedServer> {
    let is_script = launch_mode == "script";
    let mut command = if is_script {
        script_command(&server_jar, &java_path)
    } else {
        let mut command = tokio::process::Command::new(&java_path);
        if launch_mode == "argfile" {
            command
                .arg("@user_jvm_args.txt")
                .args(&jvm_args)
                .arg(format!("@{server_jar}"))
                .args(&server_args);
        } else {
            command.args(&jvm_args).arg("-jar").arg(&server_jar).args(&server_args);
        }
        command
    };
    command
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

    let last_activity_millis = Arc::new(AtomicU64::new(now_millis()));
    let reached_running = Arc::new(AtomicBool::new(false));

    spawn_log_reader(
        app.clone(),
        db.clone(),
        instance_id.clone(),
        stdout,
        "stdout",
        latest_log.clone(),
        dated_log.clone(),
        last_activity_millis.clone(),
        reached_running.clone(),
    );
    spawn_log_reader(
        app.clone(),
        db.clone(),
        instance_id.clone(),
        stderr,
        "stderr",
        latest_log,
        dated_log,
        last_activity_millis.clone(),
        reached_running.clone(),
    );
    watch_for_stuck_startup(app.clone(), db.clone(), instance_id.clone(), last_activity_millis, reached_running);

    let (kill_tx, mut kill_rx) = mpsc::channel::<()>(1);
    tokio::spawn(async move {
        let exit_status = tokio::select! {
            status = child.wait() => status,
            _ = kill_rx.recv() => {
                // In script mode the child is the shell, not Java: killing
                // it alone would leave the real server running headless
                // with nothing attached to it.
                if is_script {
                    if let Some(pid) = pid {
                        kill_process_tree(pid).await;
                    }
                }
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
        // A stopped server has no players, no tick rate, and may come back
        // on a different port if the operator edited server.properties in
        // the meantime - drop all three so the UI can't show a stale one.
        app.state::<AppState>().players.clear(&instance_id).await;
        app.state::<AppState>().tps.clear(&instance_id).await;
        app.state::<AppState>().ports.invalidate(&instance_id).await;

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

/// Reports that auto-restart has given up on an instance.
///
/// Two channels, deliberately: an in-app event so the UI can show a
/// persistent state the operator will still see when they come back to the
/// window, and an OS notification so they find out while they are elsewhere.
/// Only the second is gated on the notification setting - turning off
/// desktop notifications should not make ModpackPilot's own UI go quiet
/// about a server that has stopped trying to come back.
async fn notify_crash_loop_gave_up(app: &AppHandle, db: &SqlitePool, instance_id: &str) {
    let name: Option<String> = sqlx::query_scalar("SELECT name FROM instances WHERE id = ?")
        .bind(instance_id)
        .fetch_optional(db)
        .await
        .unwrap_or_default();
    let name = name.unwrap_or_else(|| "Instance".to_string());

    let crash_count = app.state::<AppState>().crash_tracker.count(instance_id).await;

    let message = format!(
        "{name} has crashed repeatedly and auto-restart has stopped trying. Check its logs and start it manually once fixed."
    );

    let _ = app.emit(
        super::events::CRASH_LOOP_EVENT,
        super::events::CrashLoopPayload {
            instance_id: instance_id.to_string(),
            instance_name: name.clone(),
            crash_count,
            message: message.clone(),
        },
    );

    let setting: Option<String> = sqlx::query_scalar(
        "SELECT value FROM application_settings WHERE key = 'notifications_enabled'",
    )
    .fetch_optional(db)
    .await
    .unwrap_or_default();
    if setting.as_deref() == Some("false") {
        return;
    }

    let _ = app
        .notification()
        .builder()
        .title("ModpackPilot")
        .body(message)
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
    last_activity_millis: Arc<AtomicU64>,
    reached_running: Arc<AtomicBool>,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        // Read bytes and decode lossily rather than using `lines()`, which
        // yields an error the moment a line isn't valid UTF-8 - and mods do
        // print bytes in the OS console codepage (Java only guarantees
        // UTF-8 on stdout when `stdout.encoding` is set, which is why it is
        // now, but that's a JVM-version-dependent fix and this is not the
        // only way a stray byte can arrive). Aborting the reader on such a
        // line is worse than mangling one character: nothing drains the
        // pipe any more, so the server blocks on its next `println` once
        // the pipe buffer fills and hangs mid-startup forever, while
        // running the pack's own `run.bat` in a real console works fine.
        let mut reader = BufReader::new(stream);
        let mut buf: Vec<u8> = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf).await {
                Ok(0) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buf)
                        .trim_end_matches('\n')
                        .trim_end_matches('\r')
                        .to_string();
                    last_activity_millis.store(now_millis(), Ordering::Relaxed);

                    let entry = format!("[{}] {line}\n", Utc::now().format("%H:%M:%S"));
                    {
                        let mut f = latest_log.lock().await;
                        let _ = f.write_all(entry.as_bytes()).await;
                    }
                    {
                        let mut f = dated_log.lock().await;
                        let _ = f.write_all(entry.as_bytes()).await;
                    }

                    // A TPS report is an answer to a question the poller
                    // asked, not to one the operator did, so it's fed to the
                    // tracker and swallowed rather than emitted - otherwise
                    // the console fills with replies nobody requested. It
                    // still reaches the log files above, which should record
                    // everything the server actually printed.
                    let is_tps_report = if stream_name == "stdout" {
                        let state = app.state::<AppState>();
                        state.tps.observe(&instance_id, &line).await
                    } else {
                        false
                    };

                    if !is_tps_report {
                        let _ = app.emit(
                            LOG_EVENT,
                            LogLinePayload {
                                instance_id: instance_id.clone(),
                                stream: stream_name,
                                line: line.clone(),
                            },
                        );
                    }

                    if stream_name == "stdout" && line.contains(STARTUP_COMPLETE_MARKER) {
                        reached_running.store(true, Ordering::Relaxed);
                        set_status(&app, &db, &instance_id, ServerStatus::Running).await;
                    }

                    // Player joins/leaves are announced on stdout; feeding
                    // the tracker here is free (this loop already has every
                    // line) versus polling the server with `list`.
                    if stream_name == "stdout" {
                        let changed = {
                            let state = app.state::<AppState>();
                            state.players.observe(&instance_id, &line).await
                        };
                        if changed {
                            let roster = {
                                let state = app.state::<AppState>();
                                state.players.list(&instance_id).await
                            };
                            let _ = app.emit(
                                PLAYERS_EVENT,
                                PlayersChangedPayload {
                                    instance_id: instance_id.clone(),
                                    players: roster,
                                },
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Error reading {stream_name} for instance {instance_id}: {e}");
                    break;
                }
            }
        }
    });
}

/// Watches a starting instance's own real activity (not local UI state) and
/// fires a one-time notice - a Tauri event plus a desktop notification - if
/// it goes quiet for longer than `STUCK_STARTING_THRESHOLD` before ever
/// reaching "running". Global by construction: it's driven by the actual
/// child process, so it works regardless of which page (or whether any
/// page) is open when the hang happens - the gap an earlier, UI-only
/// version of this check had.
fn watch_for_stuck_startup(
    app: AppHandle,
    db: SqlitePool,
    instance_id: String,
    last_activity_millis: Arc<AtomicU64>,
    reached_running: Arc<AtomicBool>,
) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(15)).await;

            if reached_running.load(Ordering::Relaxed) {
                return;
            }
            if !app.state::<AppState>().processes.is_running(&instance_id).await {
                return; // stopped/crashed/force-stopped before ever going stuck
            }

            let elapsed_ms = now_millis().saturating_sub(last_activity_millis.load(Ordering::Relaxed));
            if elapsed_ms < STUCK_STARTING_THRESHOLD.as_millis() as u64 {
                continue;
            }

            let _ = app.emit(STUCK_STARTING_EVENT, StuckStartingPayload { instance_id: instance_id.clone() });

            let notifications_enabled: Option<String> = sqlx::query_scalar(
                "SELECT value FROM application_settings WHERE key = 'notifications_enabled'",
            )
            .fetch_optional(&db)
            .await
            .unwrap_or_default();
            if notifications_enabled.as_deref() != Some("false") {
                let name: Option<String> = sqlx::query_scalar("SELECT name FROM instances WHERE id = ?")
                    .bind(&instance_id)
                    .fetch_optional(&db)
                    .await
                    .unwrap_or_default();
                let name = name.unwrap_or_else(|| "An instance".to_string());

                let _ = app
                    .notification()
                    .builder()
                    .title("ModpackPilot")
                    .body(format!(
                        "{name} hasn't produced any output in {}+ minutes and may be stuck starting.",
                        STUCK_STARTING_THRESHOLD.as_secs() / 60
                    ))
                    .show();
            }

            return; // one notice per start attempt, not a repeat every 15s
        }
    });
}
