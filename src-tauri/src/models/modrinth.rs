use serde::{Deserialize, Serialize};

/// One result from Modrinth's project search - just enough to let a user
/// pick the right project when linking an instance to one.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModrinthSearchHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub icon_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ModrinthSearchResponse {
    pub hits: Vec<ModrinthSearchHit>,
}

/// A Modrinth project (modpack), fetched to validate/confirm a link.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModrinthProject {
    pub id: String,
    pub slug: String,
    pub title: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ModrinthVersionFile {
    pub url: String,
    pub filename: String,
    #[serde(default)]
    pub primary: bool,
}

/// One published version of a Modrinth project.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModrinthVersion {
    pub id: String,
    pub name: String,
    pub version_number: String,
    #[serde(default)]
    pub changelog: Option<String>,
    pub date_published: String,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    // Not sent to the frontend - it only needs to know a version exists
    // and show its changelog; `apply_update` reads this directly off a
    // freshly-fetched `ModrinthVersion` on the Rust side instead.
    #[serde(skip)]
    pub(crate) files: Vec<ModrinthVersionFile>,
}

/// Result of comparing an instance's installed version against the latest
/// one Modrinth has for its linked project. `latest_version` is `None` when
/// no published version matches the instance's loader/Minecraft version -
/// not an error, just nothing to auto-suggest (the operator can still
/// browse every version manually via `list_modpack_versions`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModpackUpdateCheck {
    pub has_update: bool,
    pub current_version_id: Option<String>,
    pub latest_version: Option<ModrinthVersion>,
}

/// One file entry from a `.mrpack`'s `modrinth.index.json` - the manifest
/// describing which mods to download and where they go, rather than
/// bundling the jars directly in the archive.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MrpackFile {
    pub path: String,
    pub downloads: Vec<String>,
    #[serde(default)]
    pub env: Option<MrpackEnv>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MrpackEnv {
    #[serde(default)]
    pub server: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MrpackIndex {
    pub files: Vec<MrpackFile>,
}
