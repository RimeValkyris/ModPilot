use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

use crate::models::{
    AppliedPack, ModrinthProject, ModrinthSearchHit, ModrinthSearchResponse, ModrinthVersion,
    ModrinthVersionFile, ModrinthVersionPreview, MrpackIndex, ServerLoader,
};
use crate::packs::{apply_staged_pack, is_protected_path, join_relative, STAGING_DIR};

const BASE_URL: &str = "https://api.modrinth.com/v2";

/// A descriptive User-Agent identifying the app, per Modrinth's API
/// etiquette (https://docs.modrinth.com/api/#authentication) - built once
/// and reused rather than recreating a TLS-configured client per call.
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("ModpackPilot/0.1.0 (+https://github.com/RimeValkyris/ModPilot)")
            .build()
            .expect("static reqwest client config is always valid")
    })
}

pub async fn search_projects(query: &str) -> Result<Vec<ModrinthSearchHit>, String> {
    search_modpacks(query, "relevance").await
}

/// The list shown before anyone has typed a search term: the most
/// downloaded modpacks. Modrinth's search endpoint doubles as a browse
/// endpoint when the query is empty, so this is the same call with a
/// different sort.
pub async fn browse_projects() -> Result<Vec<ModrinthSearchHit>, String> {
    search_modpacks("", "downloads").await
}

async fn search_modpacks(query: &str, index: &str) -> Result<Vec<ModrinthSearchHit>, String> {
    let response = client()
        .get(format!("{BASE_URL}/search"))
        .query(&[
            ("query", query),
            ("facets", r#"[["project_type:modpack"]]"#),
            ("index", index),
            ("limit", "20"),
        ])
        .send()
        .await
        .map_err(|e| format!("Failed to reach Modrinth: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Modrinth search failed: {e}"))?;

    let parsed: ModrinthSearchResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Modrinth's response: {e}"))?;

    Ok(parsed.hits)
}

/// Turns a Modrinth 404 into a message that actually explains what to do
/// about it, instead of reqwest's generic "404 Not Found" - this is the
/// case a project being deleted/renamed/unpublished on Modrinth after an
/// instance was linked to it shows up as.
fn not_found_message(what: &str) -> String {
    format!(
        "{what} could not be found on Modrinth - it may have been removed, renamed, or made \
         private. Try unlinking and searching for it again."
    )
}

pub async fn get_project(id_or_slug: &str) -> Result<ModrinthProject, String> {
    let response = client()
        .get(format!("{BASE_URL}/project/{id_or_slug}"))
        .send()
        .await
        .map_err(|e| format!("Failed to reach Modrinth: {e}"))?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(not_found_message("This project"));
    }
    response
        .error_for_status()
        .map_err(|e| format!("Failed to load project from Modrinth: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse Modrinth's response: {e}"))
}

pub async fn get_project_versions(project_id: &str) -> Result<Vec<ModrinthVersion>, String> {
    let response = client()
        .get(format!("{BASE_URL}/project/{project_id}/version"))
        .send()
        .await
        .map_err(|e| format!("Failed to reach Modrinth: {e}"))?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(not_found_message(
            "The Modrinth project this instance is linked to",
        ));
    }
    response
        .error_for_status()
        .map_err(|e| format!("Failed to load versions from Modrinth: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse Modrinth's response: {e}"))
}

pub async fn get_version(version_id: &str) -> Result<ModrinthVersion, String> {
    client()
        .get(format!("{BASE_URL}/version/{version_id}"))
        .send()
        .await
        .map_err(|e| format!("Failed to reach Modrinth: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Failed to load version from Modrinth: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse Modrinth's response: {e}"))
}

/// Picks the version to treat as "the latest" for an instance: the newest
/// one matching its loader and Minecraft version, if either is known.
/// Modrinth returns versions newest-first already, so the first match wins.
///
/// Returns `None` if nothing matches - unlike an earlier version of this
/// function, it deliberately does *not* fall back to "newest overall",
/// since silently suggesting e.g. a Fabric build to a Forge server is
/// worse than saying plainly that nothing compatible was found. The
/// caller can still offer every version through `get_project_versions`
/// for the operator to pick from manually.
pub fn pick_latest_for_instance<'a>(
    versions: &'a [ModrinthVersion],
    loader: Option<&str>,
    minecraft_version: Option<&str>,
) -> Option<&'a ModrinthVersion> {
    versions.iter().find(|v| {
        let loader_ok = loader.is_none_or(|l| v.loaders.iter().any(|vl| vl.eq_ignore_ascii_case(l)));
        let mc_ok = minecraft_version.is_none_or(|mc| v.game_versions.iter().any(|vmc| vmc == mc));
        loader_ok && mc_ok
    })
}

/// Downloads a version's `.mrpack` and reads its `modrinth.index.json`.
///
/// The archive is only a manifest of download URLs plus the pack's
/// `overrides/` (configs, scripts) - not the mod jars - so it is small
/// enough to fetch just to *describe* a version on the review screen.
async fn fetch_mrpack(
    version: &ModrinthVersion,
) -> Result<(MrpackIndex, zip::ZipArchive<std::io::Cursor<Vec<u8>>>), String> {
    let mrpack_file = find_mrpack_file(&version.files)
        .ok_or_else(|| "This version doesn't have a server-installable pack file".to_string())?;

    let bytes = download_bytes(&mrpack_file.url).await?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("Failed to read downloaded pack: {e}"))?;

    let index: MrpackIndex = {
        let mut entry = archive
            .by_name("modrinth.index.json")
            .map_err(|_| "Downloaded pack is missing modrinth.index.json".to_string())?;
        let mut contents = String::new();
        entry
            .read_to_string(&mut contents)
            .map_err(|e| format!("Failed to read pack manifest: {e}"))?;
        serde_json::from_str(&contents).map_err(|e| format!("Failed to parse pack manifest: {e}"))?
    };

    Ok((index, archive))
}

/// Reads the loader and its exact build out of a pack's `dependencies`.
///
/// A `.mrpack` names loaders differently from how ModpackPilot does
/// (`fabric-loader`, `quilt-loader`), and a pack with no loader dependency
/// at all is plain vanilla.
fn loader_from_dependencies(
    dependencies: &std::collections::HashMap<String, String>,
) -> (ServerLoader, Option<String>) {
    for (key, loader) in [
        ("neoforge", ServerLoader::NeoForge),
        ("forge", ServerLoader::Forge),
        ("fabric-loader", ServerLoader::Fabric),
        ("quilt-loader", ServerLoader::Quilt),
    ] {
        if let Some(version) = dependencies.get(key) {
            return (loader, Some(version.clone()));
        }
    }
    (ServerLoader::Vanilla, None)
}

/// Maps one of Modrinth's loader names onto a [`ServerLoader`].
fn loader_from_name(name: &str) -> ServerLoader {
    match name.to_ascii_lowercase().as_str() {
        "neoforge" => ServerLoader::NeoForge,
        "forge" => ServerLoader::Forge,
        "fabric" => ServerLoader::Fabric,
        "quilt" => ServerLoader::Quilt,
        "minecraft" | "vanilla" => ServerLoader::Vanilla,
        _ => ServerLoader::Unknown,
    }
}

/// Describes what installing a version would do, without downloading
/// anything. The Modrinth counterpart of `ftb::preview`.
///
/// See [`ModrinthVersionPreview`] for why this reads metadata rather than
/// the `.mrpack` itself.
pub fn preview(version: &ModrinthVersion) -> ModrinthVersionPreview {
    let loader = version
        .loaders
        .iter()
        .map(|l| loader_from_name(l))
        .find(|l| *l != ServerLoader::Unknown)
        .unwrap_or(ServerLoader::Unknown);

    let mut warnings = Vec::new();
    if !crate::loader::is_supported(loader) {
        warnings.push(format!(
            "This pack targets \"{}\", which ModpackPilot doesn't know how to install \
             automatically. Its files will still be downloaded, but you'll have to set up the \
             server jar yourself.",
            version.loaders.first().map(String::as_str).unwrap_or("an unknown loader"),
        ));
    }
    if find_mrpack_file(&version.files).is_none() {
        warnings.push(
            "This version doesn't publish a .mrpack file, so it can't be installed \
             automatically."
                .to_string(),
        );
    }

    ModrinthVersionPreview {
        version_id: version.id.clone(),
        version_name: version.name.clone(),
        // Modrinth lists every Minecraft version a pack is marked
        // compatible with; the newest is the one it is actually built on.
        minecraft_version: version.game_versions.last().cloned(),
        loader,
        warnings,
    }
}

fn find_mrpack_file(files: &[ModrinthVersionFile]) -> Option<&ModrinthVersionFile> {
    files
        .iter()
        .find(|f| f.primary && f.filename.ends_with(".mrpack"))
        .or_else(|| files.iter().find(|f| f.filename.ends_with(".mrpack")))
}

async fn download_bytes(url: &str) -> Result<Vec<u8>, String> {
    let response = client()
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Failed to download {url}: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Failed to download {url}: {e}"))?;
    response
        .bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| format!("Failed to read downloaded data: {e}"))
}


/// Downloads and applies a Modrinth version's `.mrpack` to an instance:
/// every mod listed in `modrinth.index.json` gets downloaded, and the
/// pack's `overrides`/`server-overrides` are extracted on top - except
/// anything in `is_protected_path`. Afterwards, any file the *previous*
/// update installed that this one no longer lists gets removed (see
/// `PACK_MANIFEST_FILE`) - a mod the operator added by hand was never
/// recorded there, so it's never touched.
///
/// The whole pack is assembled in a staging directory and only moved into
/// `server/` once it is complete, so a download that dies halfway through
/// leaves the running instance exactly as it was rather than half-updated
/// (see `packs::apply_staged_pack`).
pub async fn apply_update<F>(
    instance_dir: &Path,
    version: &ModrinthVersion,
    world_folder_name: &str,
    on_progress: F,
) -> Result<AppliedPack, String>
where
    F: Fn(usize, usize),
{
    let server_dir = instance_dir.join("server");
    let staging = instance_dir.join(STAGING_DIR);
    let _ = tokio::fs::remove_dir_all(&staging).await;

    let (index, mut archive) = fetch_mrpack(version).await?;
    let (loader, loader_version) = loader_from_dependencies(&index.dependencies);
    let applied = AppliedPack {
        loader,
        loader_version,
        minecraft_version: index.dependencies.get("minecraft").cloned(),
    };

    let total = index.files.len();
    let mut done = 0usize;
    on_progress(0, total);
    // Download every file the manifest lists - `.mrpack` only bundles a
    // manifest of download URLs, not the mod jars themselves.
    for file in &index.files {
        done += 1;
        on_progress(done, total);
        let server_env = file.env.as_ref().and_then(|e| e.server.as_deref());
        if server_env == Some("unsupported") {
            continue; // client-only content (e.g. a resource pack mod)
        }
        let relative = file.path.replace('\\', "/");
        if is_protected_path(&relative, world_folder_name) {
            continue;
        }
        let Some(url) = file.downloads.first() else {
            continue;
        };
        let bytes = download_bytes(url).await?;
        let dest = join_relative(&staging, &relative);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
        }
        tokio::fs::write(&dest, bytes)
            .await
            .map_err(|e| format!("Failed to write {}: {e}", dest.display()))?;
    }

    // Extract `overrides/`, then `server-overrides/` on top of it (the
    // mrpack spec has server-overrides take precedence for server
    // installs) - skipping anything protected.
    for prefix in ["overrides/", "server-overrides/"] {
        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| format!("Failed to read pack entry: {e}"))?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().replace('\\', "/");
            let Some(relative) = name.strip_prefix(prefix) else {
                continue;
            };
            if relative.is_empty() || is_protected_path(relative, world_folder_name) {
                continue;
            }
            let relative = relative.to_string();
            let dest = join_relative(&staging, &relative);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
            }
            let mut out = std::fs::File::create(&dest)
                .map_err(|e| format!("Failed to write {}: {e}", dest.display()))?;
            std::io::copy(&mut entry, &mut out)
                .map_err(|e| format!("Failed to write {}: {e}", dest.display()))?;
        }
    }

    // Commit the staged pack. Anything the previous update installed that
    // this one didn't gets cleaned up in there too.
    let result = apply_staged_pack(instance_dir, &server_dir, &staging, world_folder_name).await;
    let _ = tokio::fs::remove_dir_all(&staging).await;
    result.map(|_installed| applied)
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Ignored by default - see `ftb::live_tests` for why.
    #[tokio::test]
    #[ignore]
    async fn reaches_modrinth() {
        let hits = search_projects("create").await.expect("modrinth search");
        assert!(!hits.is_empty());
    }

    /// The browse list is what the import wizard shows before anyone types,
    /// so an empty one would leave the picker looking broken.
    #[tokio::test]
    #[ignore]
    async fn browses_modpacks_without_a_search_term() {
        let hits = browse_projects().await.expect("modrinth browse");
        assert!(!hits.is_empty());
    }

    /// A preview must be derivable from version metadata alone - the whole
    /// point of not downloading the `.mrpack` to build one.
    #[tokio::test]
    #[ignore]
    async fn previews_a_version_without_downloading_it() {
        let hits = browse_projects().await.expect("modrinth browse");
        let versions = get_project_versions(&hits[0].project_id)
            .await
            .expect("project versions");
        let preview = preview(&versions[0]);
        assert!(
            preview.minecraft_version.is_some(),
            "preview should name a Minecraft version"
        );
        assert_ne!(
            preview.loader,
            ServerLoader::Unknown,
            "preview should name a loader"
        );
    }
}
