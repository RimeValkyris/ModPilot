// `pub mod` (not a re-export) so `tauri::generate_handler!` can see the
// hidden items the `#[tauri::command]` macro generates alongside each
// command function.
pub mod instance;
