use serde::Serialize;

/// One `.jar` (or disabled `.jar.disabled`) found in an instance's `mods/`
/// folder.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModInfo {
    /// The actual filename on disk - what `toggle_mod`/`delete_mod` expect back.
    pub file_name: String,
    /// `file_name` with any `.disabled` suffix stripped, for display.
    pub display_name: String,
    pub enabled: bool,
    pub size_bytes: u64,
}
