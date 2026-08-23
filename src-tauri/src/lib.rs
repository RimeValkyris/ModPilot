mod commands;
mod database;
mod filesystem;
mod importer;
mod java;
mod logging;
mod models;
mod server;

use filesystem::AppPaths;
use sqlx::SqlitePool;
use tauri::{Emitter, Manager};

/// Shared state handed to every Tauri command via `tauri::State<AppState>`.
pub struct AppState {
    pub db: SqlitePool,
    pub paths: AppPaths,
    pub processes: server::ProcessManager,
    pub resource_monitor: server::ResourceMonitor,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();

            let paths = AppPaths::resolve(&handle)?;
            paths.ensure_dirs_exist()?;

            // Logging must start before anything else can fail loudly.
            // Leaked deliberately: it needs to live for the whole process,
            // and `setup` has no natural place to stash a guard that outlives it.
            let guard = logging::init(&paths.logs_dir);
            Box::leak(Box::new(guard));

            // The release build hides its console window, so a panic's
            // default stderr output is otherwise invisible - route it into
            // the same log file the crash-log export reads from.
            std::panic::set_hook(Box::new(|panic_info| {
                tracing::error!("ModForge panicked: {panic_info}");
            }));

            tracing::info!("ModForge starting up, app data dir: {:?}", paths.app_data_dir);

            // `setup` is synchronous; block briefly on the one-time pool/migration
            // step so every command that runs afterward can assume the DB is ready.
            let db = tauri::async_runtime::block_on(database::init_pool(&paths.db_path))?;

            app.manage(AppState {
                db,
                paths,
                processes: server::ProcessManager::new(),
                resource_monitor: server::ResourceMonitor::new(),
            });

            // Guard against closing ModForge while a Minecraft server is
            // still running: without this, the child process would be
            // orphaned (left running with no UI to manage it) rather than
            // shut down cleanly.
            if let Some(window) = app.get_webview_window("main") {
                let handle = app.handle().clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let handle = handle.clone();
                        tauri::async_runtime::spawn(async move {
                            let state = handle.state::<AppState>();
                            if state.processes.any_running().await {
                                let _ = handle.emit("close-requested-with-running-servers", ());
                            } else if let Some(window) = handle.get_webview_window("main") {
                                let _ = window.destroy();
                            }
                        });
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::instance::list_instances,
            commands::instance::create_instance,
            commands::instance::rename_instance,
            commands::instance::delete_instance,
            commands::instance::set_instance_java,
            commands::instance::list_server_jars,
            commands::instance::update_instance_settings,
            commands::import::analyze_import,
            commands::import::import_instance,
            commands::java::list_java_installations,
            commands::java::detect_java_installations,
            commands::java::set_default_java,
            commands::server::start_instance,
            commands::server::stop_instance,
            commands::server::force_stop_instance,
            commands::server::restart_instance,
            commands::server::send_console_command,
            commands::server::list_running_instance_ids,
            commands::logs::read_latest_log,
            commands::wallpaper::set_instance_wallpaper,
            commands::wallpaper::clear_instance_wallpaper,
            commands::wallpaper::read_instance_wallpaper,
            commands::monitor::get_resource_usage,
            commands::diagnostics::get_app_logs_dir,
            commands::diagnostics::export_app_log,
            commands::diagnostics::get_instance_logs_dir,
            commands::diagnostics::export_instance_log,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
