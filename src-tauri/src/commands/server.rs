use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tauri::{AppHandle, State};
use uuid::Uuid;

use super::instance::fetch_instance;
use crate::importer;
use crate::java;
use crate::models::{Instance, ServerLoader, ServerStatus};
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

    ensure_eula_accepted(&working_dir).await?;

    if matches!(instance.loader, ServerLoader::Forge | ServerLoader::NeoForge) {
        disable_forge_update_checker(&working_dir).await;
    }

    let java_path = resolve_java_path(
        &state,
        instance.java_installation_id.as_deref(),
        instance.minecraft_version.as_deref(),
        instance.loader,
    )
    .await?;

    // Minecraft's server console uses JLine, which tries to negotiate real
    // terminal capabilities on startup. That works fine attached to a real
    // console (e.g. running `start.bat` directly by hand) but can hang
    // indefinitely when stdin/stdout are redirected pipes instead - which
    // is unavoidable here, since ModpackPilot needs piped I/O to capture
    // the console into its own UI. This is the same fix every server-
    // wrapper tool (Multicraft, McMyAdmin, etc.) applies: skip JLine's
    // terminal detection entirely rather than let it hang trying to probe
    // a terminal that was never going to be there.
    const WRAPPER_COMPAT_JVM_ARGS: &[&str] =
        &["-Djline.terminal=jline.UnsupportedTerminal", "-Dfile.encoding=UTF8"];

    let mut jvm_args: Vec<String> = WRAPPER_COMPAT_JVM_ARGS.iter().map(|s| s.to_string()).collect();
    jvm_args.extend(if instance.jvm_args.is_empty() {
        vec![
            format!("-Xms{}M", instance.min_ram_mb),
            format!("-Xmx{}M", instance.max_ram_mb),
        ]
    } else {
        instance.jvm_args.clone()
    });
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
        instance.launch_mode.clone(),
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

/// Returns the ids of every instance ModpackPilot currently has a running
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

/// Runs a Forge/NeoForge installer jar's headless `--installServer` mode
/// to turn it into an actual runnable server (`libraries/`, `run.bat`/
/// `run.sh`, the `@`-argfiles `spawn_server_process` needs), then
/// re-detects the instance's `server_jar`/`launch_mode` from the result.
///
/// Without `--installServer`, running a Forge/NeoForge installer jar just
/// opens its interactive GUI wizard - which is exactly what silently
/// happened every time an instance whose `server_jar` pointed at an
/// installer was started (see `importer::detect`'s installer-jar
/// exclusion, added alongside this command).
#[tauri::command]
pub async fn install_forge_server(state: State<'_, AppState>, id: String) -> Result<Instance, String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let server_dir = Path::new(&instance.server_directory).join("server");

    let installer_path = {
        let mut entries = tokio::fs::read_dir(&server_dir)
            .await
            .map_err(|e| format!("Failed to read server directory: {e}"))?;
        let mut found = None;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| format!("Failed to read server directory: {e}"))?
        {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.ends_with("installer.jar") {
                found = Some(entry.path());
                break;
            }
        }
        found.ok_or_else(|| {
            "No Forge/NeoForge installer jar (*-installer.jar) was found in this instance's server folder".to_string()
        })?
    };

    let java_path = resolve_java_path(
        &state,
        instance.java_installation_id.as_deref(),
        instance.minecraft_version.as_deref(),
        instance.loader,
    )
    .await?;

    let mut command = tokio::process::Command::new(&java_path);
    command
        .arg("-jar")
        .arg(&installer_path)
        .arg("--installServer")
        .current_dir(&server_dir);
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW - this runs headless; the installer's own GUI
        // is exactly what --installServer exists to skip.
        command.creation_flags(0x0800_0000);
    }

    let output = command
        .output()
        .await
        .map_err(|e| format!("Failed to run the Forge/NeoForge installer: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let tail = if stderr.trim().is_empty() { stdout } else { stderr };
        return Err(format!(
            "Forge/NeoForge installer failed (exit code {:?}): {}",
            output.status.code(),
            tail.lines().rev().take(5).collect::<Vec<_>>().join(" / "),
        ));
    }

    let detected = {
        let server_dir = server_dir.clone();
        tauri::async_runtime::spawn_blocking(move || importer::detect_from_dir(&server_dir))
            .await
            .map_err(|e| format!("Detection task failed: {e}"))?
    };

    let Some(server_jar) = detected.server_jar else {
        return Err(
            "The installer finished, but ModpackPilot couldn't find the resulting server files. \
             Check this instance's server folder manually."
                .to_string(),
        );
    };
    let launch_mode = if detected.server_jar_is_argfile { "argfile" } else { "jar" };

    sqlx::query("UPDATE instances SET server_jar = ?, launch_mode = ? WHERE id = ?")
        .bind(&server_jar)
        .bind(launch_mode)
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| format!("Installed, but failed to save the result: {e}"))?;

    fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())
}

/// Proactively disables Forge/NeoForge's built-in update checker before
/// the server ever boots. `enableUpdateChecker` is what lets a mod's
/// update-check HTTP call run at startup at all - if that connection gets
/// silently dropped instead of refused (no connect-timeout is set by these
/// checkers, so this isn't rare on a network with an overzealous firewall
/// or filtered DNS), the whole server hangs forever on it. This is applied
/// on every start, not just the first - an operator who wants update
/// checks back on can still flip it in the config themselves, but a config
/// this hasn't been generated yet gets the safe default instead of hitting
/// the same hang the very first time.
///
/// Best-effort: patches both `forge-common.toml` and `neoforge-common.toml`
/// regardless of which loader this is (the one that doesn't apply is just
/// never read by the actual loader) and never fails the start over it - a
/// config it can't touch just means the loader's own default applies.
async fn disable_forge_update_checker(working_dir: &Path) {
    for filename in ["forge-common.toml", "neoforge-common.toml"] {
        let path = working_dir.join("config").join(filename);
        if let Err(e) = patch_update_checker_toggle(&path).await {
            tracing::warn!("Failed to disable update checker in {}: {e}", path.display());
        }
    }
}

async fn patch_update_checker_toggle(path: &Path) -> std::io::Result<()> {
    match tokio::fs::read_to_string(path).await {
        Ok(contents) => {
            let re = regex::Regex::new(r"(?im)^\s*enableUpdateChecker\s*=.*$").unwrap();
            let patched = if re.is_match(&contents) {
                re.replace(&contents, "enableUpdateChecker = false").into_owned()
            } else {
                // Key not present (the file exists but was only partially
                // generated) - TOML allows re-opening a table, so it's
                // safe to append a fresh `[general]` block even if one
                // already exists earlier in the file.
                format!("{contents}\n[general]\nenableUpdateChecker = false\n")
            };
            if patched != contents {
                tokio::fs::write(path, patched).await?;
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(path, "[general]\nenableUpdateChecker = false\n").await
        }
        Err(e) => Err(e),
    }
}

/// Writes `eula.txt` with `eula=true` if it isn't already there. Mojang's
/// EULA requires an operator to accept it before a server will run at all
/// - vanilla and every loader just print a message and exit immediately
/// otherwise. Pressing "Start" in ModpackPilot *is* that acceptance: the
/// operator explicitly chose to launch this server, the same way typing
/// `eula=true` into the file by hand would be. Never overwrites an
/// existing file, so an operator who deliberately set `eula=false` stays
/// in control.
async fn ensure_eula_accepted(working_dir: &Path) -> Result<(), String> {
    let path = working_dir.join("eula.txt");
    if path.is_file() {
        return Ok(());
    }
    tokio::fs::write(
        &path,
        "# Accepted automatically by ModpackPilot when this instance was first started.\n\
         # https://aka.ms/MinecraftEULA\n\
         eula=true\n",
    )
    .await
    .map_err(|e| format!("Failed to write eula.txt: {e}"))
}

/// Picks the JVM to launch an instance with.
///
/// An explicitly-assigned installation always wins - that's the operator's
/// deliberate choice, and it's respected even if it looks wrong (only
/// logged). Otherwise this auto-selects a *detected* installation matching
/// the Java version the instance's Minecraft version actually targets,
/// rather than falling back to whatever `java` happens to be first on
/// PATH.
///
/// That naive PATH fallback was a real, hard-to-diagnose bug: a machine
/// with several JDKs installed would silently launch e.g. a Minecraft
/// 1.20.1 + Forge pack (which targets Java 17) under Java 21, where Forge
/// 47.x hangs partway through mod loading with no error - while the same
/// pack's own `start.bat` worked fine because it resolved a different
/// JVM. Silently picking the wrong JVM and hanging is far worse than
/// saying plainly which Java is needed, so an unresolvable mismatch is now
/// a clear error instead of a mystery freeze.
async fn resolve_java_path(
    state: &State<'_, AppState>,
    java_installation_id: Option<&str>,
    minecraft_version: Option<&str>,
    loader: ServerLoader,
) -> Result<String, String> {
    let required = java::required_java_major(minecraft_version);

    if let Some(java_id) = java_installation_id {
        let row = sqlx::query_as::<_, (String, String)>(
            "SELECT path, version FROM java_installations WHERE id = ?",
        )
        .bind(java_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| format!("Failed to look up Java installation: {e}"))?
        .ok_or_else(|| "The Java installation assigned to this instance no longer exists".to_string())?;

        let (path, version) = row;
        if let (Some(required), Some(actual)) = (required, java::parse_java_major(&version)) {
            if required != actual {
                tracing::warn!(
                    "Instance is explicitly set to Java {actual} but Minecraft {} targets Java {required}; honoring the explicit choice",
                    minecraft_version.unwrap_or("?"),
                );
            }
        }
        return Ok(path);
    }

    // Nothing assigned - try to auto-pick a detected install that matches.
    let Some(required) = required else {
        return Ok("java".to_string()); // unknown MC version: nothing better to go on
    };

    let installations = sqlx::query_as::<_, (String, String)>(
        "SELECT path, version FROM java_installations",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| format!("Failed to look up Java installations: {e}"))?;

    if let Some((path, _)) = installations
        .iter()
        .find(|(_, version)| java::parse_java_major(version) == Some(required))
    {
        tracing::info!("Auto-selected Java {required} for Minecraft {}", minecraft_version.unwrap_or("?"));
        return Ok(path.clone());
    }

    // No match installed. For Forge/NeoForge specifically, running on the
    // wrong major is a known hang rather than a graceful failure, so refuse
    // instead of starting something that will freeze partway through mod
    // loading.
    if matches!(loader, ServerLoader::Forge | ServerLoader::NeoForge) {
        let available = if installations.is_empty() {
            "none detected".to_string()
        } else {
            installations
                .iter()
                .filter_map(|(_, v)| java::parse_java_major(v))
                .map(|m| format!("Java {m}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        return Err(format!(
            "This pack is Minecraft {} on {}, which needs Java {required}, but no Java {required} \
             installation was found (available: {available}). Running it on a different Java \
             version typically hangs partway through mod loading. Install Java {required}, then \
             use Java -> Rescan and pick it for this instance.",
            minecraft_version.unwrap_or("?"),
            loader.as_str(),
        ));
    }

    Ok("java".to_string())
}
