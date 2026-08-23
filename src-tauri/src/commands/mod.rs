// `pub mod` (not a re-export) so `tauri::generate_handler!` can see the
// hidden items the `#[tauri::command]` macro generates alongside each
// command function.
pub mod diagnostics;
pub mod import;
pub mod instance;
pub mod java;
pub mod logs;
pub mod monitor;
pub mod server;
pub mod wallpaper;
