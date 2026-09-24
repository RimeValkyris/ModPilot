use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use super::instance::fetch_instance;
use crate::java;
use crate::models::{Instance, ServerLoader, ServerStatus};
use crate::server::{self, LoaderInstallProgressPayload, RunningProcess, LOADER_INSTALL_PROGRESS_EVENT};
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

    // Detection found only a loader installer for this instance, so there
    // is nothing to start yet - running the installer jar would just open
    // its GUI wizard and look like a broken server. The console page shows
    // the install banner for exactly this state.
    if instance.launch_mode == "installer" {
        return Err(
            "This server isn't installed yet - it only has a Forge/NeoForge installer. Run \
             \"Install Forge/NeoForge Server\" on this instance first."
                .to_string(),
        );
    }

    let server_jar = instance
        .server_jar
        .clone()
        .ok_or_else(|| "No server JAR configured for this instance".to_string())?;

    let working_dir = Path::new(&instance.server_directory).join("server");
    if !working_dir.join(&server_jar).is_file() {
        let kind = if instance.launch_mode == "script" { "Start script" } else { "Server JAR" };
        return Err(format!(
            "{kind} \"{server_jar}\" was not found in the instance's server folder"
        ));
    }

    ensure_eula_accepted(&working_dir).await?;

    // In "script" mode the pack's own script decides the JVM flags, so the
    // instance's `jvm_args` never reach Java. `user_jvm_args.txt` is the
    // one exception: it's the file those scripts read (and the file every
    // pack's own docs tell operators to edit), so the RAM configured in
    // ModpackPilot is applied there instead of being silently ignored.
    if instance.launch_mode == "script" {
        apply_ram_to_user_jvm_args(&working_dir, instance.min_ram_mb, instance.max_ram_mb).await;
    }

    if matches!(instance.loader, ServerLoader::Forge | ServerLoader::NeoForge) {
        disable_forge_update_checker(&working_dir).await;
    }

    let script_manages_java = instance.launch_mode == "script"
        && instance.java_installation_id.is_none()
        && script_downloads_java(&working_dir.join(&server_jar)).await;
    let java_path = if script_manages_java {
        // The pack's script owns the runtime bootstrap. Passing a selected
        // JAVA_HOME here would make its "download Java if missing" branch
        // believe Java is already installed and can also force the wrong
        // major before the script gets to choose its own runtime.
        tracing::info!(instance_id = %id, "Allowing server script to select or download Java");
        "java".to_string()
    } else if instance.java_installation_id.is_none() {
        if let Some(path) = find_bundled_compatible_java(
            &working_dir,
            instance.minecraft_version.as_deref(),
            instance.loader_version.as_deref(),
            instance.loader,
        )
        .await?
        {
            path
        } else {
            resolve_java_path(
                &state.db,
                None,
                instance.minecraft_version.as_deref(),
                instance.loader_version.as_deref(),
                instance.loader,
            )
            .await?
        }
    } else {
        resolve_java_path(
            &state.db,
            instance.java_installation_id.as_deref(),
            instance.minecraft_version.as_deref(),
            instance.loader_version.as_deref(),
            instance.loader,
        )
        .await?
    };

    // Minecraft's server console uses JLine, which tries to negotiate real
    // terminal capabilities on startup. That works fine attached to a real
    // console (e.g. running `start.bat` directly by hand) but can hang
    // indefinitely when stdin/stdout are redirected pipes instead - which
    // is unavoidable here, since ModpackPilot needs piped I/O to capture
    // the console into its own UI. This is the same fix every server-
    // wrapper tool (Multicraft, McMyAdmin, etc.) applies: skip JLine's
    // terminal detection entirely rather than let it hang trying to probe
    // a terminal that was never going to be there.
    //
    // `stdout.encoding`/`stderr.encoding` matter for the same reason: from
    // Java 19 on, `file.encoding` no longer governs `System.out` when it's
    // a pipe - the OS console codepage does (cp1252 here), so any mod
    // printing a non-ASCII character sends bytes ModpackPilot's UTF-8
    // reader can't decode. The reader survives that now (see
    // `spawn_log_reader`), but it shouldn't have to guess in the first
    // place.
    const WRAPPER_COMPAT_JVM_ARGS: &[&str] = &[
        "-Djline.terminal=jline.UnsupportedTerminal",
        "-Dfile.encoding=UTF8",
        "-Dstdout.encoding=UTF-8",
        "-Dstderr.encoding=UTF-8",
    ];

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

/// Finds and prefers a runtime shipped inside the instance. A bundled JVM is
/// the strongest compatibility signal available: pack authors tested their
/// server against it, and it avoids selecting an unrelated system JDK.
async fn find_bundled_compatible_java(
    working_dir: &Path,
    minecraft_version: Option<&str>,
    loader_version: Option<&str>,
    loader: ServerLoader,
) -> Result<Option<String>, String> {
    let scan_root = working_dir.to_path_buf();
    let found = tauri::async_runtime::spawn_blocking(move || {
        crate::java::detect_java_installations_under(&scan_root)
    })
    .await
    .map_err(|error| format!("Bundled Java detection task failed: {error}"))?;

    if found.is_empty() {
        return Ok(None);
    }

    let required = minecraft_version
        .map(str::to_string)
        .or_else(|| {
            (loader == ServerLoader::NeoForge)
                .then(|| loader_version.and_then(crate::importer::minecraft_version_from_neoforge))
                .flatten()
        })
        .and_then(|version| java::required_java_major(Some(&version)));

    let Some(required) = required else {
        return Ok(None);
    };

    let mut compatible: Vec<_> = found
        .into_iter()
        .filter(|java| java::parse_java_major(&java.version) == Some(required))
        .collect();
    compatible.sort_by(|left, right| right.version.cmp(&left.version));

    if let Some(selected) = compatible.first() {
        tracing::info!(
            java_path = %selected.path,
            required_java = required,
            "Selected bundled Java runtime"
        );
        return verify_java_executable(
            &selected.path,
            Some(required),
            minecraft_version,
            loader,
            true,
        )
        .await
        .map(Some);
    }

    Ok(None)
}

/// Returns true for pack scripts that appear to bootstrap their own JVM.
/// Those scripts commonly download a runtime only when their expected local
/// runtime is absent; injecting ModPilot's JVM environment would short-circuit
/// that branch and can select an incompatible system JDK.
async fn script_downloads_java(path: &Path) -> bool {
    let Ok(contents) = tokio::fs::read_to_string(path).await else {
        return false;
    };
    let contents = contents.to_ascii_lowercase();
    let refers_to_runtime = ["runtime", "\\jre", "/jre", "java_home", "jdk"].iter().any(|term| contents.contains(term));
    let downloads = [
        "download",
        "invoke-webrequest",
        "curl ",
        "wget ",
        "bitsadmin",
        "certutil",
        "powershell",
        "http://",
        "https://",
    ]
    .iter()
    .any(|term| contents.contains(term));
    refers_to_runtime && downloads
}

/// Requests a graceful shutdown by sending Minecraft's own `stop` command.
/// Returns as soon as the request is sent - the instance transitions to
/// STOPPED asynchronously once the server actually exits (watch
/// `instance-status-changed`).
#[tauri::command]
pub async fn stop_instance(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    if !state.processes.is_running(&id).await {
        server::clear_stale_instance(&app, &state.db, &id).await;
        return Ok(());
    }

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
    if !state.processes.is_running(&id).await {
        server::clear_stale_instance(&app, &state.db, &id).await;
        return Ok(());
    }

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
    } else {
        // No live process, but the stored status may still claim otherwise
        // (e.g. the app was killed while the server was up), which
        // `start_instance` would reject. Correct it first so a restart is
        // still the one-click way out.
        server::clear_stale_instance(&app, &state.db, &id).await;
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
pub async fn install_forge_server(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Instance, String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let (installed, learned_version) = install_loader_files(&app, &state.db, &instance).await?;

    let server_jar = installed.server_jar;
    let launch_mode = installed.launch_mode.as_str();

    // COALESCE so a version the operator typed by hand is never replaced
    // by one read out of the installer.
    sqlx::query(
        "UPDATE instances
         SET server_jar = ?, launch_mode = ?, minecraft_version = COALESCE(minecraft_version, ?)
         WHERE id = ?",
    )
        .bind(&server_jar)
        .bind(launch_mode)
        .bind(&learned_version)
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| format!("Installed, but failed to save the result: {e}"))?;

    fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())
}

/// Runs an instance's Forge/NeoForge installer and reports what it produced,
/// without touching the database - the callers decide what to record.
///
/// Shared by the manual "Install Forge/NeoForge Server" button and the
/// importer, which runs this as the last step of an import so a pack that
/// ships only an installer arrives ready to start instead of parked on a
/// banner asking for one more click.
/// Returns what the install produced, plus a Minecraft version read out of
/// the installer when the instance didn't already have one - see
/// `loader::minecraft_version_from_installer`.
pub(crate) async fn install_loader_files(
    app: &AppHandle,
    db: &sqlx::SqlitePool,
    instance: &Instance,
) -> Result<(crate::loader::InstalledLoader, Option<String>), String> {
    let server_dir = Path::new(&instance.server_directory).join("server");

    // The instance's own `server_jar` is the installer whenever detection
    // flagged it as needing this step, so trust that first and only fall
    // back to scanning when it points at something else (an instance
    // imported before that detection existed, say).
    let installer_path = match instance.server_jar.as_deref() {
        Some(jar)
            if crate::importer::is_loader_installer_name(jar)
                && server_dir.join(jar).is_file() =>
        {
            server_dir.join(jar)
        }
        _ => find_installer_jar(&server_dir).await?.ok_or_else(|| {
            "No Forge/NeoForge installer jar was found in this instance's server folder"
                .to_string()
        })?,
    };

    // An installer whose file name kept its versions (the official
    // naming) has already given detection a Minecraft version. A renamed
    // one hasn't, and without a version there is nothing to choose a Java
    // from - so ask the installer itself before giving up.
    let learned_version = match instance.minecraft_version {
        Some(_) => None,
        None => {
            let path = installer_path.clone();
            tauri::async_runtime::spawn_blocking(move || {
                crate::loader::minecraft_version_from_installer(&path)
            })
            .await
            .unwrap_or_default()
        }
    };
    if let Some(version) = &learned_version {
        tracing::info!(
            "Read Minecraft {version} out of {}",
            installer_path.display()
        );
    }

    let java_path = resolve_java_path(
        db,
        instance.java_installation_id.as_deref(),
        instance
            .minecraft_version
            .as_deref()
            .or(learned_version.as_deref()),
        instance.loader_version.as_deref(),
        instance.loader,
    )
    .await?;

    // The installer prints what it's doing but never how far along it is,
    // so its lines are forwarded as-is: real steps beat a made-up
    // percentage during a wait this long.
    let report = |step: &str| {
        let _ = app.emit(
            LOADER_INSTALL_PROGRESS_EVENT,
            LoaderInstallProgressPayload {
                instance_id: instance.id.clone(),
                step: step.to_string(),
            },
        );
    };

    crate::loader::run_installer_jar_reporting(
        &java_path,
        &installer_path,
        &server_dir,
        Some(&report),
    )
    .await?;

    Ok((crate::loader::detect_installed(&server_dir).await?, learned_version))
}

/// Scans an instance's server folder for a loader installer jar, at the
/// root or one directory down - the same two places import detection looks
/// (see `importer::detect`'s `find_loader_installer`), so a pack whose
/// installer sits in a subfolder can still be installed from here.
async fn find_installer_jar(server_dir: &Path) -> Result<Option<PathBuf>, String> {
    let mut root = tokio::fs::read_dir(server_dir)
        .await
        .map_err(|e| format!("Failed to read server directory: {e}"))?;

    let mut subdirs = Vec::new();
    while let Some(entry) = root
        .next_entry()
        .await
        .map_err(|e| format!("Failed to read server directory: {e}"))?
    {
        let name = entry.file_name().to_string_lossy().to_string();
        if crate::importer::is_loader_installer_name(&name) {
            return Ok(Some(entry.path()));
        }
        // `mods/` is skipped for the same reason detection skips it: a mod
        // jar with "installer" in its name is a mod.
        if entry.path().is_dir() && !name.eq_ignore_ascii_case("mods") {
            subdirs.push(entry.path());
        }
    }

    for dir in subdirs {
        let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if crate::importer::is_loader_installer_name(&entry.file_name().to_string_lossy()) {
                return Ok(Some(entry.path()));
            }
        }
    }

    Ok(None)
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

/// Marks the lines ModpackPilot owns in a pack's `user_jvm_args.txt`, so
/// rewriting them can't accumulate duplicates across launches.
const MANAGED_RAM_MARKER: &str = "# Memory settings managed by ModpackPilot";

/// Rewrites the `-Xms`/`-Xmx` lines of `user_jvm_args.txt` to match the
/// instance's configured RAM, leaving every other line (GC flags, custom
/// properties, comments) untouched.
///
/// Deliberately does nothing when the pack doesn't ship the file: its
/// presence is what tells us the start script actually reads it. Creating
/// one for a script that ignores it would only look like it worked.
async fn apply_ram_to_user_jvm_args(working_dir: &Path, min_ram_mb: i64, max_ram_mb: i64) {
    let path = working_dir.join("user_jvm_args.txt");
    let Ok(existing) = tokio::fs::read_to_string(&path).await else {
        return;
    };

    let mut lines: Vec<String> = existing
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            // `-Xmn` (young generation) is a different flag and stays.
            !trimmed.starts_with("-Xms")
                && !trimmed.starts_with("-Xmx")
                && trimmed != MANAGED_RAM_MARKER
        })
        .map(str::to_string)
        .collect();

    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    lines.push(MANAGED_RAM_MARKER.to_string());
    lines.push(format!("-Xms{min_ram_mb}M"));
    lines.push(format!("-Xmx{max_ram_mb}M"));

    let contents = format!("{}\n", lines.join("\n"));
    if let Err(e) = tokio::fs::write(&path, contents).await {
        tracing::warn!("Couldn't apply RAM settings to user_jvm_args.txt: {e}");
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
/// EULA requires an operator to accept it before a server will run at
/// all: vanilla and every loader just print a message and exit
/// immediately otherwise. Pressing "Start" in ModpackPilot *is* that
/// acceptance: the
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

/// Verifies the executable that will actually be launched rather than
/// trusting the version cached during Java discovery. A replaced JDK or a
/// stale database row must not make a server appear to be pinned to Java 17.
async fn verify_java_executable(
    java_path: &str,
    required: Option<u32>,
    minecraft_version: Option<&str>,
    loader: ServerLoader,
    enforce_required: bool,
) -> Result<String, String> {
    let output = tokio::process::Command::new(java_path)
        .arg("-version")
        .output()
        .await
        .map_err(|error| format!("Unable to run Java executable \"{java_path}\": {error}"))?;

    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let raw_version = text
        .split("version \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .ok_or_else(|| format!("Java executable \"{java_path}\" did not report a version"))?;
    let normalized_version = raw_version.strip_prefix("1.").unwrap_or(raw_version);
    let actual = normalized_version
        .split('.')
        .next()
        .and_then(|major| major.parse::<u32>().ok())
        .ok_or_else(|| format!("Could not determine the Java major version from \"{raw_version}\""))?;

    tracing::info!(java_path, java_version = raw_version, "Verified Java executable");

    if enforce_required {
        if let Some(required) = required {
            if actual != required {
                return Err(format!(
                    "Java mismatch: ModPilot is about to launch \"{java_path}\" as Java {actual}, but Minecraft {} on {} requires Java {required}. Select a Java {required} installation for this instance.",
                    minecraft_version.unwrap_or("?"),
                    loader.as_str(),
                ));
            }
        }
    }

    Ok(java_path.to_string())
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
pub(crate) async fn resolve_java_path(
    db: &sqlx::SqlitePool,
    java_installation_id: Option<&str>,
    minecraft_version: Option<&str>,
    loader_version: Option<&str>,
    loader: ServerLoader,
) -> Result<String, String> {
    // The Java page is not required to be opened before starting a server.
    // Refresh automatic candidates here so a newly installed JDK or changed
    // JAVA_HOME is visible without relying on stale database rows.
    if java_installation_id.is_none() {
        if let Err(error) = super::java::refresh_java_installations(db).await {
            tracing::warn!(?error, "Java scan before automatic resolution failed; using cached installations");
        }
    }

    // A pack imported as a plain server folder often has no Minecraft
    // version of its own, and without one the checks below have nothing to
    // check against - the PATH fallback takes over and launches whatever
    // JDK happens to be first, which is the exact silent mismatch this
    // function exists to prevent. NeoForge's version encodes the Minecraft
    // version it targets, so use that rather than give up: `21.1.244` is
    // 1.21.1, which needs Java 21, no matter what PATH points at.
    let effective_mc_version = minecraft_version.map(str::to_string).or_else(|| {
        (loader == ServerLoader::NeoForge)
            .then(|| loader_version.and_then(crate::importer::minecraft_version_from_neoforge))
            .flatten()
    });
    let required = java::required_java_major(effective_mc_version.as_deref());
    let minecraft_version = effective_mc_version.as_deref().or(minecraft_version);

    if let Some(java_id) = java_installation_id {
        let row = sqlx::query_as::<_, (String, String)>(
            "SELECT path, version FROM java_installations WHERE id = ?",
        )
        .bind(java_id)
        .fetch_optional(db)
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
        return verify_java_executable(&path, required, minecraft_version, loader, false).await;
    }

    // Nothing assigned - try to auto-pick a detected install that matches.
    let Some(required) = required else {
        return verify_java_executable("java", required, minecraft_version, loader, false).await;
    };

    let installations = sqlx::query_as::<_, (String, String)>(
        "SELECT path, version FROM java_installations
         ORDER BY is_default DESC, detected_at DESC, version DESC",
    )
    .fetch_all(db)
    .await
    .map_err(|e| format!("Failed to look up Java installations: {e}"))?;

    if let Some((path, _)) = installations
        .iter()
        .find(|(_, version)| java::parse_java_major(version) == Some(required))
    {
        tracing::info!("Auto-selected Java {required} for Minecraft {}", minecraft_version.unwrap_or("?"));
        return verify_java_executable(path, Some(required), minecraft_version, loader, true).await;
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

    verify_java_executable("java", Some(required), minecraft_version, loader, true).await
}

#[cfg(test)]
mod user_jvm_args_tests {
    use super::*;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("modpackpilot-jvmargs-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// Forge/NeoForge ship this file with the memory flags commented out
    /// and other flags live; only the memory ones may be touched.
    #[tokio::test]
    async fn rewrites_memory_flags_and_leaves_everything_else_alone() {
        let dir = temp_dir("rewrite");
        std::fs::write(
            dir.join("user_jvm_args.txt"),
            "# JVM arguments
-Xmx2G
-XX:+UseG1GC
-Xmn128M
",
        )
        .expect("seed file");

        apply_ram_to_user_jvm_args(&dir, 3072, 8192).await;

        let contents = std::fs::read_to_string(dir.join("user_jvm_args.txt")).expect("read back");
        assert!(contents.contains("# JVM arguments"), "{contents}");
        assert!(contents.contains("-XX:+UseG1GC"), "{contents}");
        assert!(contents.contains("-Xmn128M"), "the young-gen flag is not a memory setting: {contents}");
        assert!(contents.contains("-Xms3072M"), "{contents}");
        assert!(contents.contains("-Xmx8192M"), "{contents}");
        assert!(!contents.contains("-Xmx2G"), "the old maximum should be gone: {contents}");
    }

    /// Every launch rewrites the file, so it must converge rather than
    /// grow a new pair of flags each time.
    #[tokio::test]
    async fn repeated_applications_are_idempotent() {
        let dir = temp_dir("idempotent");
        std::fs::write(dir.join("user_jvm_args.txt"), "-Xmx2G
").expect("seed file");

        apply_ram_to_user_jvm_args(&dir, 1024, 4096).await;
        let once = std::fs::read_to_string(dir.join("user_jvm_args.txt")).expect("read back");
        apply_ram_to_user_jvm_args(&dir, 1024, 4096).await;
        let twice = std::fs::read_to_string(dir.join("user_jvm_args.txt")).expect("read back");

        assert_eq!(once, twice);
        assert_eq!(twice.matches("-Xmx").count(), 1, "{twice}");
    }

    /// A pack with no `user_jvm_args.txt` has a script that doesn't read
    /// one - writing the file would only pretend the setting took effect.
    #[tokio::test]
    async fn does_not_create_the_file_when_the_pack_has_none() {
        let dir = temp_dir("absent");

        apply_ram_to_user_jvm_args(&dir, 1024, 4096).await;

        assert!(!dir.join("user_jvm_args.txt").exists());
    }

    #[tokio::test]
    async fn recognizes_a_script_that_downloads_its_own_java() {
        let dir = temp_dir("managed-java");
        let script = dir.join("run.bat");
        std::fs::write(
            &script,
            "if not exist runtime\\bin\\java.exe powershell Invoke-WebRequest https://example.invalid/jre.zip\n",
        )
        .expect("seed script");

        assert!(script_downloads_java(&script).await);
    }

    #[tokio::test]
    async fn keeps_a_plain_java_script_on_modpilots_selected_runtime() {
        let dir = temp_dir("plain-java");
        let script = dir.join("run.bat");
        std::fs::write(&script, "java -Xmx4G -jar server.jar nogui\n").expect("seed script");

        assert!(!script_downloads_java(&script).await);
    }
}
