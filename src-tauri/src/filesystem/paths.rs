use std::path::PathBuf;

use tauri::{AppHandle, Manager};

/// Well-known ModForge directories, all rooted under the OS-specific app data
/// directory Tauri resolves for us (e.g. `%APPDATA%/com.modforge.app` on
/// Windows). Nothing here is hard-coded - `Manager::path()` is what makes
/// this portable to Linux later.
///
/// Users can move `instances_dir` elsewhere via Settings (Phase 7); the
/// stored override is applied by whoever constructs this struct at startup.
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub app_data_dir: PathBuf,
    pub instances_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub db_path: PathBuf,
}

impl AppPaths {
    pub fn resolve(app: &AppHandle) -> Result<Self, tauri::Error> {
        let app_data_dir = app.path().app_data_dir()?;
        Ok(Self {
            instances_dir: app_data_dir.join("instances"),
            logs_dir: app_data_dir.join("logs"),
            db_path: app_data_dir.join("modforge.sqlite"),
            app_data_dir,
        })
    }

    pub fn ensure_dirs_exist(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.app_data_dir)?;
        std::fs::create_dir_all(&self.instances_dir)?;
        std::fs::create_dir_all(&self.logs_dir)?;
        Ok(())
    }
}
