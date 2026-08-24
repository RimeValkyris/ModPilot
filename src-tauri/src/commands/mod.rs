// `pub mod` (not a re-export) so `tauri::generate_handler!` can see the
// hidden items the `#[tauri::command]` macro generates alongside each
// command function.
pub mod backup;
pub mod diagnostics;
pub mod import;
pub mod instance;
pub mod instance_avatar;
pub mod java;
pub mod logs;
pub mod playerlist;
pub mod mods;
pub mod monitor;
pub mod server;
pub mod server_icon;
pub mod server_properties;
pub mod settings;
