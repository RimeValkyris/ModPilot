use serde::{Deserialize, Serialize};

use super::ServerLoader;

/// Where an import's source files come from. The frontend gets this path
/// via a native file/folder picker or a drag-and-drop drop event - ModpackPilot
/// never lets the user type an arbitrary path into a text field.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ImportSource {
    Zip { path: String },
    Folder { path: String },
}

/// Best-effort read of what an imported server looks like, shown to the
/// user for review (wizard Step 4) before anything is copied into an
/// instance directory.
///
/// Every field here is a guess based on common server layouts - imported
/// files are untrusted and not every server follows the same structure, so
/// nothing here is ever treated as certain until the user confirms it.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedServerInfo {
    pub minecraft_version: Option<String>,
    pub loader: ServerLoader,
    pub loader_version: Option<String>,
    pub server_jar: Option<String>,
    /// `true` when `server_jar` is actually the path to a modern
    /// Forge/NeoForge `@`-argfile rather than a directly-runnable jar -
    /// see `Instance::launch_mode`.
    pub server_jar_is_argfile: bool,
    /// `true` when no jar or argfile could be identified at all and
    /// `server_jar` is the pack's own start script, to be run as-is - see
    /// `Instance::launch_mode`.
    pub server_jar_is_script: bool,
    /// The Forge/NeoForge installer jar this pack ships, if any - relative
    /// to the server root. Present whether or not the pack is already
    /// installed (an installed server often keeps the installer around).
    pub loader_installer: Option<String>,
    /// `true` when the installer above is all there is: the pack ships an
    /// installer and no runnable server, so it has to be installed once
    /// before it can start. `server_jar` then holds the installer's path
    /// and `launch_mode` is "installer".
    pub needs_loader_install: bool,
    pub has_mods_folder: bool,
    pub mod_count: usize,
    pub has_config_folder: bool,
    pub has_world_folder: bool,
    pub world_folder_name: Option<String>,
    pub has_server_properties: bool,
    pub start_scripts: Vec<String>,
    /// `-Xms`/`-Xmx` the pack sets for itself, in MB, read from
    /// `user_jvm_args.txt` or its start script. These seed the import
    /// form's RAM fields: a 250-mod pack that asks for 8 GB and gets
    /// ModpackPilot's 4 GB default instead doesn't crash, it just never
    /// finishes loading, which is a miserable thing to debug.
    pub suggested_min_ram_mb: Option<i64>,
    pub suggested_max_ram_mb: Option<i64>,
    /// Anything worth surfacing to the user that isn't fatal: multiple
    /// candidate server JARs, no JAR found at all, unreadable entries, etc.
    pub warnings: Vec<String>,
}

impl DetectedServerInfo {
    /// The `Instance::launch_mode` this detection result implies.
    pub fn launch_mode(&self) -> &'static str {
        if self.needs_loader_install {
            "installer"
        } else if self.server_jar_is_script {
            "script"
        } else if self.server_jar_is_argfile {
            "argfile"
        } else {
            "jar"
        }
    }
}

/// Input for `import_instance` (wizard Step 6). Any field left `None` falls
/// back to what detection found; fields the user edited during review
/// (Step 4/5) override it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportInstanceRequest {
    pub name: String,
    pub minecraft_version: Option<String>,
    pub loader: Option<ServerLoader>,
    pub loader_version: Option<String>,
    pub min_ram_mb: Option<i64>,
    pub max_ram_mb: Option<i64>,
    /// Set only after the user has been warned that an instance folder with
    /// this name already exists and chosen to proceed anyway. Importing
    /// never overwrites silently.
    #[serde(default)]
    pub overwrite: bool,
}
