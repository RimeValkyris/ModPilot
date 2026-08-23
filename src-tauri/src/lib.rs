mod commands;
mod database;
mod filesystem;
mod logging;
mod models;

use filesystem::AppPaths;
use sqlx::SqlitePool;
use tauri::Manager;

/// Shared state handed to every Tauri command via `tauri::State<AppState>`.
pub struct AppState {
    pub db: SqlitePool,
    pub paths: AppPaths,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();

            let paths = AppPaths::resolve(&handle)?;
            paths.ensure_dirs_exist()?;

            // Logging must start before anything else can fail loudly.
            // Leaked deliberately: it needs to live for the whole process,
            // and `setup` has no natural place to stash a guard that outlives it.
            let guard = logging::init(&paths.logs_dir);
            Box::leak(Box::new(guard));

            tracing::info!("ModForge starting up, app data dir: {:?}", paths.app_data_dir);

            // `setup` is synchronous; block briefly on the one-time pool/migration
            // step so every command that runs afterward can assume the DB is ready.
            let db = tauri::async_runtime::block_on(database::init_pool(&paths.db_path))?;

            app.manage(AppState { db, paths });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::instance::list_instances,
            commands::instance::create_instance,
            commands::instance::rename_instance,
            commands::instance::delete_instance,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
