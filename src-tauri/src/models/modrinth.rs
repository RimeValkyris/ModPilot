use serde::{Deserialize, Serialize};

/// One result from Modrinth's project search - just enough to let a user
/// pick the right project when linking an instance to one.
///
/// Deserializes from Modrinth's actual API (snake_case: `project_id`,
/// `icon_url`, ...) and serializes to the frontend as camelCase - a single
/// blanket `rename_all = "camelCase"` would apply to *both* directions and
/// silently fail to parse Modrinth's real response (this happened; every
/// multi-word field came back missing/wrong until this was split).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
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
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
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
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
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
    /// Required by the mrpack format (`sha1` and `sha512`). Checked against
    /// every download, since `downloads` points wherever the pack's author
    /// chose rather than only at Modrinth's own CDN.
    #[serde(default)]
    pub hashes: std::collections::HashMap<String, String>,
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
    /// Maps a component to the exact build the pack needs, e.g.
    /// `{"minecraft": "1.20.1", "neoforge": "47.1.99"}`. This is where a
    /// `.mrpack` says which loader to install - the equivalent of an FTB
    /// version's `targets`.
    #[serde(default)]
    pub dependencies: std::collections::HashMap<String, String>,
}

/// What installing a `.mrpack` version would do, shown for review before
/// anything is written. The Modrinth counterpart of
/// [`crate::models::FtbVersionPreview`].
///
/// Deliberately built from Modrinth's *version metadata* alone, with no
/// download: a `.mrpack` bundles the pack's whole `overrides/` tree, so it
/// runs to tens or hundreds of megabytes on a large pack, and fetching one
/// just to describe it - then fetching it again to install - would make
/// picking a version feel like installing one. The file count and download
/// size an FTB preview shows are unknowable without that archive, so they
/// are reported live during the install instead.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModrinthVersionPreview {
    pub version_id: String,
    pub version_name: String,
    pub minecraft_version: Option<String>,
    pub loader: crate::models::ServerLoader,
    pub warnings: Vec<String>,
}

/// The loader a `.mrpack` turned out to need, read from its manifest while
/// installing.
///
/// This is the authoritative source for the loader's exact build: Modrinth's
/// version metadata names the loader ("neoforge") but never which build, and
/// the loader's server can't be installed without one.
#[derive(Debug, Clone)]
pub struct AppliedPack {
    pub loader: crate::models::ServerLoader,
    pub loader_version: Option<String>,
    pub minecraft_version: Option<String>,
}

/// A request to install a Modrinth modpack version as a new instance.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModrinthImportRequest {
    pub project_id: String,
    pub version_id: String,
    pub name: String,
    pub min_ram_mb: Option<i64>,
    pub max_ram_mb: Option<i64>,
    #[serde(default)]
    pub overwrite: bool,
}
