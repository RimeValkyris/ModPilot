use std::collections::HashSet;
use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

use crate::models::{
    ModrinthProject, ModrinthSearchHit, ModrinthSearchResponse, ModrinthVersion,
    ModrinthVersionFile, MrpackIndex,
};
use crate::packs::{is_protected_path, join_relative, prune_stale, write_pack_manifest};

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
    let response = client()
        .get(format!("{BASE_URL}/search"))
        .query(&[
            ("query", query),
            ("facets", r#"[["project_type:modpack"]]"#),
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
/// every mod listed in `modrinth.index.json` gets downloaded into place,
/// and the pack's `overrides`/`server-overrides` are extracted on top -
/// except anything in `is_protected_path`. Afterwards, any file the
/// *previous* update installed that this one no longer lists gets removed
/// (see `PACK_MANIFEST_FILE`) - a mod the operator added by hand was never
/// recorded there, so it's never touched.
pub async fn apply_update(instance_dir: &Path, version: &ModrinthVersion, world_folder_name: &str) -> Result<(), String> {
    let server_dir = instance_dir.join("server");

    let mrpack_file = find_mrpack_file(&version.files)
        .ok_or_else(|| "This version doesn't have a server-installable pack file".to_string())?;

    let mrpack_bytes = download_bytes(&mrpack_file.url).await?;

    let cursor = std::io::Cursor::new(mrpack_bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| format!("Failed to read downloaded pack: {e}"))?;

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

    let mut installed: HashSet<String> = HashSet::new();

    // Download every file the manifest lists - `.mrpack` only bundles a
    // manifest of download URLs, not the mod jars themselves.
    for file in &index.files {
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
        let dest = join_relative(&server_dir, &relative);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
        }
        tokio::fs::write(&dest, bytes)
            .await
            .map_err(|e| format!("Failed to write {}: {e}", dest.display()))?;
        installed.insert(relative);
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
            let dest = join_relative(&server_dir, &relative);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
            }
            let mut out = std::fs::File::create(&dest)
                .map_err(|e| format!("Failed to write {}: {e}", dest.display()))?;
            std::io::copy(&mut entry, &mut out)
                .map_err(|e| format!("Failed to write {}: {e}", dest.display()))?;
            installed.insert(relative);
        }
    }

    // Anything the previous update installed that this one didn't was
    // removed from the pack - clean it up. Best-effort: a failed removal
    // (e.g. the server has the jar open) shouldn't fail the whole update.
    prune_stale(instance_dir, &server_dir, &installed).await;

    write_pack_manifest(instance_dir, &installed).await?;

    Ok(())
}

