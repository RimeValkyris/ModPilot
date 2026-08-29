use serde::{Deserialize, Serialize};

/// FTB's search endpoint returns bare pack IDs, not pack objects - the
/// caller has to fetch each one to get anything displayable. `curseforge`
/// holds IDs from the other provider and is deliberately ignored: those
/// aren't installable through this API.
#[derive(Debug, Clone, Deserialize)]
pub struct FtbSearchResponse {
    #[serde(default)]
    pub packs: Vec<i64>,
}

/// One piece of pack artwork. FTB ships several per pack (square logo,
/// splash, screenshots); the linking UI only wants the square one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtbArt {
    pub url: String,
    #[serde(rename = "type")]
    pub art_type: String,
}

/// A published version of a pack, as listed on the pack itself. The full
/// file list lives on [`FtbVersionManifest`], fetched separately.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FtbVersionSummary {
    pub id: i64,
    pub name: String,
    /// "release" | "beta" | "alpha" - lowercase in FTB's responses.
    #[serde(rename = "type")]
    pub version_type: String,
    /// Unix seconds. FTB uses `0` for "unknown" rather than omitting it.
    #[serde(default)]
    pub updated: i64,
    /// The version list carries each version's targets, so picking a
    /// compatible update costs no extra requests - unlike Modrinth, where
    /// the loader/game versions live on the version object itself.
    #[serde(default)]
    pub targets: Vec<FtbTarget>,
}

impl FtbVersionSummary {
    pub fn target(&self, name: &str) -> Option<&FtbTarget> {
        self.targets.iter().find(|t| t.name == name)
    }

    pub fn minecraft_version(&self) -> Option<&str> {
        self.target("minecraft").map(|t| t.version.as_str())
    }

    pub fn loader_target(&self) -> Option<&FtbTarget> {
        self.targets.iter().find(|t| t.target_type == "modloader")
    }
}

/// An FTB modpack. Only the fields the UI actually shows are modelled -
/// the real response also carries plays/installs/ratings/tags.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FtbPack {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub slug: String,
    /// The one-line blurb. FTB's `description` is a full markdown page and
    /// is far too long for a search result.
    #[serde(default)]
    pub synopsis: String,
    #[serde(default)]
    pub art: Vec<FtbArt>,
    #[serde(default)]
    pub versions: Vec<FtbVersionSummary>,
    /// The square logo, picked out of `art` once on fetch so the frontend
    /// doesn't have to know FTB's art-type vocabulary. Never present in
    /// FTB's own response - see `ftb::get_pack`.
    #[serde(default, skip_deserializing)]
    pub icon_url: Option<String>,
}

impl FtbPack {
    /// The square logo, which is what the pack picker renders. Falls back
    /// to whatever art exists rather than showing nothing.
    pub fn pick_icon(&self) -> Option<String> {
        self.art
            .iter()
            .find(|a| a.art_type == "square")
            .or_else(|| self.art.first())
            .map(|a| a.url.clone())
    }

    /// Versions newest-first. FTB returns them oldest-first, which is the
    /// opposite of what every "pick a version" list wants.
    pub fn versions_newest_first(&self) -> Vec<FtbVersionSummary> {
        let mut versions = self.versions.clone();
        versions.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| b.id.cmp(&a.id)));
        versions
    }
}

/// What a pack version targets: Minecraft version, mod loader, and the
/// Java runtime FTB itself ships for it.
///
/// This is the reason an FTB import can show a review screen without
/// downloading anything - `name` is one of "minecraft" / "forge" /
/// "neoforge" / "fabric" / "java".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FtbTarget {
    pub name: String,
    pub version: String,
    #[serde(rename = "type", default)]
    pub target_type: String,
}

/// One file in a pack version. `path` is a directory relative to the server
/// root (`"./config"`, `"./mods"`) and `name` is the filename - they are
/// separate fields, unlike the mrpack format's single joined path.
#[derive(Debug, Clone, Deserialize)]
pub struct FtbFile {
    pub path: String,
    pub name: String,
    #[serde(default)]
    pub url: String,
    /// Alternate download locations, used when `url` is empty (FTB does
    /// this for content it can't redistribute directly).
    #[serde(default)]
    pub mirrors: Vec<String>,
    #[serde(default)]
    pub sha1: String,
    /// "mod" | "config" | "resource" | "script" - used to report a mod
    /// count on the review screen.
    #[serde(rename = "type", default)]
    pub file_type: String,
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub clientonly: bool,
    #[serde(default)]
    pub serveronly: bool,
    #[serde(default)]
    pub optional: bool,
}

impl FtbFile {
    /// The file's location relative to the server directory, normalized to
    /// forward slashes with FTB's `./` and `.` path forms flattened away,
    /// so it can be fed straight to `packs::is_protected_path` /
    /// `packs::join_relative`.
    ///
    /// Getting this exactly right matters: a path that still carries a `.`
    /// segment doesn't compare equal to `"server.properties"`, so it would
    /// slip past the protected-path check and let an update overwrite live
    /// server state.
    pub fn relative_path(&self) -> String {
        let mut segments: Vec<&str> = self
            .path
            .split(['/', '\\'])
            .filter(|s| !s.is_empty() && *s != "." && *s != "..")
            .collect();
        segments.push(self.name.as_str());
        segments.join("/")
    }

    /// Every URL worth trying, primary first.
    pub fn download_urls(&self) -> Vec<&str> {
        let mut urls = Vec::new();
        if !self.url.is_empty() {
            urls.push(self.url.as_str());
        }
        urls.extend(self.mirrors.iter().map(String::as_str).filter(|u| !u.is_empty()));
        urls
    }
}

/// A pack version's full manifest: what to install, and what to install it
/// onto.
#[derive(Debug, Clone, Deserialize)]
pub struct FtbVersionManifest {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub targets: Vec<FtbTarget>,
    #[serde(default)]
    pub files: Vec<FtbFile>,
}

impl FtbVersionManifest {
    pub fn target(&self, name: &str) -> Option<&FtbTarget> {
        self.targets.iter().find(|t| t.name == name)
    }

    pub fn minecraft_version(&self) -> Option<String> {
        self.target("minecraft").map(|t| t.version.clone())
    }

    /// The modloader target, if this pack uses one. Vanilla packs have only
    /// a `minecraft` target.
    pub fn loader_target(&self) -> Option<&FtbTarget> {
        self.targets.iter().find(|t| t.target_type == "modloader")
    }

    /// Files that belong on a server.
    ///
    /// FTB marks client-only content explicitly, and `optional` files are
    /// opt-in extras a server shouldn't silently install - except when FTB
    /// also marks them `serveronly`, which is its way of saying the file
    /// exists *for* the server and has no client side to opt into.
    pub fn server_files(&self) -> impl Iterator<Item = &FtbFile> {
        self.files
            .iter()
            .filter(|f| f.serveronly || (!f.clientonly && !f.optional))
    }
}

/// Whether a linked instance's FTB pack has a newer version - the FTB
/// counterpart to `ModpackUpdateCheck`.
///
/// `latest_version` is `None` (not an error) when the pack publishes
/// versions but none match this instance's loader and Minecraft version;
/// the operator can still browse and pick one manually.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FtbUpdateCheck {
    pub has_update: bool,
    pub current_version_id: Option<i64>,
    pub latest_version: Option<FtbVersionSummary>,
}

/// Input for `import_ftb_instance`, the FTB counterpart to
/// `ImportInstanceRequest`. There is no detection to fall back on here -
/// the pack's own manifest supplies the Minecraft version and loader, so
/// only the operator's own choices travel in this struct.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FtbImportRequest {
    pub pack_id: i64,
    pub version_id: i64,
    pub name: String,
    pub min_ram_mb: Option<i64>,
    pub max_ram_mb: Option<i64>,
    /// Set only after the user has been warned that an instance folder with
    /// this name already exists and chosen to proceed anyway.
    #[serde(default)]
    pub overwrite: bool,
}

/// What an FTB version would install, shown for review before anything is
/// downloaded - the FTB counterpart to `DetectedServerInfo`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FtbVersionPreview {
    pub version_id: i64,
    pub version_name: String,
    pub minecraft_version: Option<String>,
    pub loader: crate::models::ServerLoader,
    pub loader_version: Option<String>,
    /// The Java major FTB targets for this pack, parsed from its `java`
    /// target (e.g. `"21.0.4+7-LTS"` -> `21`).
    pub java_major: Option<u32>,
    pub mod_count: usize,
    pub total_files: usize,
    /// Sum of every server file's size, for a "this is a 3.2 GB download"
    /// warning before committing to it.
    pub download_size_bytes: i64,
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, name: &str, clientonly: bool, serveronly: bool, optional: bool) -> FtbFile {
        FtbFile {
            path: path.to_string(),
            name: name.to_string(),
            url: "https://example.invalid/x".to_string(),
            mirrors: Vec::new(),
            sha1: String::new(),
            file_type: "mod".to_string(),
            size: 0,
            clientonly,
            serveronly,
            optional,
        }
    }

    #[test]
    fn relative_path_normalizes_ftbs_leading_dot_slash() {
        assert_eq!(file("./mods", "jei.jar", false, false, false).relative_path(), "mods/jei.jar");
        assert_eq!(file("./", "server-icon.png", false, false, false).relative_path(), "server-icon.png");
        assert_eq!(file(".", "eula.txt", false, false, false).relative_path(), "eula.txt");
        // Nested paths and Windows separators both land on one form.
        assert_eq!(
            file("./config/foo", "bar.toml", false, false, false).relative_path(),
            "config/foo/bar.toml"
        );
        assert_eq!(
            file(".\\config", "bar.toml", false, false, false).relative_path(),
            "config/bar.toml"
        );
    }

    #[test]
    fn server_files_skips_client_only_and_optional_content() {
        let manifest = FtbVersionManifest {
            id: 1,
            name: "1.0".to_string(),
            targets: Vec::new(),
            files: vec![
                file("./mods", "keep.jar", false, false, false),
                file("./mods", "client-only.jar", true, false, false),
                file("./mods", "opt-in.jar", false, false, true),
                // Optional *and* server-only: FTB's way of saying this
                // exists for the server, so it is installed.
                file("./mods", "server-extra.jar", false, true, true),
            ],
        };

        let kept: Vec<_> = manifest.server_files().map(|f| f.name.clone()).collect();
        assert_eq!(kept, vec!["keep.jar", "server-extra.jar"]);
    }

    #[test]
    fn targets_describe_the_server_to_build() {
        let manifest = FtbVersionManifest {
            id: 1,
            name: "1.0".to_string(),
            targets: vec![
                FtbTarget { name: "minecraft".into(), version: "1.21.1".into(), target_type: "game".into() },
                FtbTarget { name: "neoforge".into(), version: "21.1.248".into(), target_type: "modloader".into() },
                FtbTarget { name: "java".into(), version: "21.0.4+7-LTS".into(), target_type: "runtime".into() },
            ],
            files: Vec::new(),
        };

        assert_eq!(manifest.minecraft_version().as_deref(), Some("1.21.1"));
        let loader = manifest.loader_target().expect("modloader target");
        assert_eq!(loader.name, "neoforge");
        assert_eq!(loader.version, "21.1.248");
        // A vanilla pack has no modloader target at all.
        let vanilla = FtbVersionManifest { targets: vec![], files: vec![], ..manifest };
        assert!(vanilla.loader_target().is_none());
    }

    #[test]
    fn newest_versions_come_first() {
        let pack = FtbPack {
            id: 1,
            name: "Pack".into(),
            slug: "pack".into(),
            synopsis: String::new(),
            art: Vec::new(),
            icon_url: None,
            versions: vec![
                FtbVersionSummary {
                    id: 10,
                    name: "1.0".into(),
                    version_type: "release".into(),
                    updated: 100,
                    targets: Vec::new(),
                },
                FtbVersionSummary {
                    id: 30,
                    name: "3.0".into(),
                    version_type: "release".into(),
                    updated: 300,
                    targets: Vec::new(),
                },
                FtbVersionSummary {
                    id: 20,
                    name: "2.0".into(),
                    version_type: "beta".into(),
                    updated: 200,
                    targets: Vec::new(),
                },
            ],
        };
        let ordered: Vec<_> = pack.versions_newest_first().into_iter().map(|v| v.id).collect();
        assert_eq!(ordered, vec![30, 20, 10]);
    }
}
