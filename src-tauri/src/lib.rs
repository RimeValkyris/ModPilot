mod commands;
mod database;
mod filesystem;
mod ftb;
mod importer;
mod java;
mod loader;
mod logging;
mod models;
mod modrinth;
mod packs;
mod server;

use filesystem::AppPaths;
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Manager};

/// Shared state handed to every Tauri command via `tauri::State<AppState>`.
pub struct AppState {
    pub db: SqlitePool,
    pub paths: AppPaths,
    pub processes: server::ProcessManager,
    pub resource_monitor: server::ResourceMonitor,
    pub crash_tracker: server::CrashTracker,
    pub schedules: server::ScheduleTracker,
    pub players: server::PlayerTracker,
    pub tps: server::TpsTracker,
    pub disks: server::DiskSampler,
    pub ports: server::PortCache,
    pub alerts: server::AlertTracker,
    pub update_checks: server::UpdateCheckTracker,
}

/// Checks for running servers before actually exiting: if any are running,
/// asks the frontend to confirm (via the close-guard dialog) rather than
/// killing them silently. Shared by the window's close button and the tray
/// menu's Quit item, so both go through the same safety check.
async fn request_app_exit(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.processes.any_running().await {
        let _ = app.emit("close-requested-with-running-servers", ());
    } else {
        app.exit(0);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let handle = app.handle().clone();

            let mut paths = AppPaths::resolve(&handle)?;

            // Logging must start before anything else can fail loudly.
            // Leaked deliberately: it needs to live for the whole process,
            // and `setup` has no natural place to stash a guard that outlives it.
            std::fs::create_dir_all(&paths.logs_dir)?;
            let guard = logging::init(&paths.logs_dir);
            Box::leak(Box::new(guard));

            // The release build hides its console window, so a panic's
            // default stderr output is otherwise invisible - route it into
            // the same log file the crash-log export reads from.
            std::panic::set_hook(Box::new(|panic_info| {
                tracing::error!("ModpackPilot panicked: {panic_info}");
            }));

            tracing::info!("ModpackPilot starting up, app data dir: {:?}", paths.app_data_dir);

            // `setup` is synchronous; block briefly on the one-time pool/migration
            // step so every command that runs afterward can assume the DB is ready.
            let db = tauri::async_runtime::block_on(database::init_pool(&paths.db_path))?;

            // A user-configured instances directory (Settings) overrides the
            // default location. Applied here, before anything else touches
            // `paths.instances_dir`, so the whole app session is consistent.
            if let Ok(Some(custom_dir)) = tauri::async_runtime::block_on(
                commands::settings::get_setting(&db, "instances_dir"),
            ) {
                if !custom_dir.trim().is_empty() {
                    paths.instances_dir = std::path::PathBuf::from(custom_dir);
                }
            }
            paths.ensure_dirs_exist()?;

            // Nothing is running in a process we just started, whatever the
            // database says. Repair rows left mid-lifecycle by a previous
            // session before the UI (or auto-start) can act on them - see
            // `server::reconcile_stale_state`.
            tauri::async_runtime::block_on(server::reconcile_stale_state(&db));

            app.manage(AppState {
                db,
                paths,
                processes: server::ProcessManager::new(),
                resource_monitor: server::ResourceMonitor::new(),
                crash_tracker: server::CrashTracker::new(),
                schedules: server::ScheduleTracker::new(),
                players: server::PlayerTracker::new(),
                tps: server::TpsTracker::new(),
                disks: server::DiskSampler::new(),
                ports: server::PortCache::new(),
                alerts: server::AlertTracker::new(),
                update_checks: server::UpdateCheckTracker::new(),
            });

            // Launch any instance marked auto-start, once the window/state
            // are ready. Fire-and-forget: a failure here (e.g. a missing
            // JAR) shouldn't block ModpackPilot from opening.
            let auto_start_handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                let db = &auto_start_handle.state::<AppState>().db;
                let ids: Vec<String> =
                    sqlx::query_scalar("SELECT id FROM instances WHERE auto_start = 1")
                        .fetch_all(db)
                        .await
                        .unwrap_or_default();

                for id in ids {
                    let handle = auto_start_handle.clone();
                    let state = handle.state::<AppState>();
                    if let Err(e) =
                        commands::server::start_instance(handle.clone(), state, id.clone()).await
                    {
                        tracing::warn!("Auto-start failed for instance {id}: {e}");
                    }
                }
            });

            // Guard against closing ModpackPilot while a Minecraft server is
            // still running: without this, the child process would be
            // orphaned (left running with no UI to manage it) rather than
            // shut down cleanly. If "minimize to tray" is on, a close
            // request just hides the window instead - nothing is exiting,
            // so there's nothing to guard.
            if let Some(window) = app.get_webview_window("main") {
                let handle = app.handle().clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let handle = handle.clone();
                        tauri::async_runtime::spawn(async move {
                            let state = handle.state::<AppState>();
                            let minimize_to_tray =
                                commands::settings::get_setting_bool(&state.db, "minimize_to_tray", false)
                                    .await;

                            if minimize_to_tray {
                                if let Some(window) = handle.get_webview_window("main") {
                                    let _ = window.hide();
                                }
                                return;
                            }

                            request_app_exit(&handle).await;
                        });
                    }
                });
            }

            // Automated restarts / backups (see `server::scheduler`).
            server::spawn_scheduler(handle.clone());
            server::spawn_alerts(handle.clone());
            server::spawn_tps_poller(handle.clone());

            setup_tray(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::instance::list_instances,
            commands::instance::create_instance,
            commands::instance::rename_instance,
            commands::instance::duplicate_instance,
            commands::instance::delete_instance,
            commands::instance::set_instance_java,
            commands::instance::list_server_jars,
            commands::instance::update_instance_settings,
            commands::instance::set_instance_schedules,
            commands::import::analyze_import,
            commands::import::import_instance,
            commands::import::update_instance_from_source,
            commands::java::list_java_installations,
            commands::java::detect_java_installations,
            commands::java::set_default_java,
            commands::java::reset_java_installations,
            commands::server::start_instance,
            commands::server::stop_instance,
            commands::server::force_stop_instance,
            commands::server::restart_instance,
            commands::server::send_console_command,
            commands::server::list_running_instance_ids,
            commands::server::install_forge_server,
            commands::logs::read_latest_log,
            commands::monitor::get_resource_usage,
            commands::monitor::get_all_resource_usage,
            commands::monitor::get_system_memory_mb,
            commands::monitor::get_disk_usage,
            commands::monitor::list_online_players,
            commands::diagnostics::get_app_logs_dir,
            commands::diagnostics::open_managed_folder,
            commands::diagnostics::export_app_log,
            commands::diagnostics::get_instance_logs_dir,
            commands::diagnostics::get_instance_subfolder,
            commands::diagnostics::export_instance_log,
            commands::backup::create_world_backup,
            commands::backup::list_world_backups,
            commands::backup::restore_world_backup,
            commands::backup::delete_world_backup,
            commands::playerlist::read_player_list,
            commands::playerlist::write_player_list,
            commands::mods::list_mods,
            commands::mods::toggle_mod,
            commands::mods::delete_mod,
            commands::settings::get_app_setting,
            commands::settings::set_app_setting,
            commands::settings::get_instances_dir,
            commands::settings::get_portable_instances_dir,
            commands::settings::set_instances_dir,
            commands::settings::quit_app,
            commands::server_icon::set_server_icon,
            commands::server_icon::clear_server_icon,
            commands::server_icon::read_server_icon,
            commands::server_properties::read_server_properties,
            commands::server_properties::write_server_properties,
            commands::instance_avatar::set_instance_avatar,
            commands::instance_avatar::clear_instance_avatar,
            commands::instance_avatar::read_instance_avatar,
            commands::instance_avatar::list_avatar_presets,
            commands::instance_avatar::set_instance_avatar_preset,
            commands::modrinth::search_modrinth_projects,
            commands::modrinth::browse_modrinth_packs,
            commands::modrinth::list_modrinth_project_versions,
            commands::modrinth::analyze_modrinth_version,
            commands::modrinth::import_modrinth_instance,
            commands::modrinth::link_modrinth_project,
            commands::modrinth::unlink_modrinth_project,
            commands::modrinth::check_modpack_update,
            commands::modrinth::list_modpack_versions,
            commands::modrinth::apply_modpack_update,
            commands::modrinth::set_update_policy,
            commands::ftb::search_ftb_packs,
            commands::ftb::browse_ftb_packs,
            commands::ftb::get_ftb_pack,
            commands::ftb::analyze_ftb_version,
            commands::ftb::import_ftb_instance,
            commands::ftb::link_ftb_pack,
            commands::ftb::unlink_ftb_pack,
            commands::ftb::check_ftb_update,
            commands::ftb::list_ftb_versions,
            commands::ftb::apply_ftb_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Builds the tray icon: left-click (or the "Show ModpackPilot" item) restores
/// the window, "Quit" goes through the same running-servers safety check as
/// the window's own close button.
fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let show_item = MenuItem::with_id(app, "show", "Show ModpackPilot", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .expect("app icon is bundled");

    TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .tooltip("ModpackPilot")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    request_app_exit(&app).await;
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(())
}
