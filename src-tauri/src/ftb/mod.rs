//! The FTB (Feed the Beast) modpack API, and installing a pack version's
//! files onto an instance.
//!
//! This is the counterpart to [`crate::modrinth`] and deliberately shares
//! its update machinery ([`crate::packs`]), so an FTB pack gets the same
//! guarantees a Modrinth one does: the world, `server.properties` and the
//! player lists survive an update, and files a previous version installed
//! are cleaned up while hand-added mods are left alone.
//!
//! What FTB does *not* give us is the server itself. A version's file list
//! is mods/configs/scripts only - the Forge/NeoForge server has to be
//! installed separately, which is what [`crate::loader`] is for. That split
//! is exactly what FTB's own `serverinstall_*.exe` does internally.

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use sha1::{Digest, Sha1};
use tokio::sync::Semaphore;

use crate::models::{
    FtbPack, FtbSearchResponse, FtbTarget, FtbVersionManifest, FtbVersionPreview,
    FtbVersionSummary, ServerLoader,
};
use crate::packs::{apply_staged_pack, is_protected_path, join_relative, STAGING_DIR};

const BASE_URL: &str = "https://api.feed-the-beast.com/v1/modpacks/public";

/// How many files to download at once.
///
/// Unlike an mrpack's few dozen entries, an FTB pack routinely lists 2000+
/// files, so downloading them one at a time is the difference between a
/// couple of minutes and the better part of an hour. Kept modest anyway:
/// this is someone else's CDN, and a home connection saturates long before
/// a higher number would help.
const DOWNLOAD_CONCURRENCY: usize = 6;

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("ModpackPilot/0.1.0 (+https://github.com/RimeValkyris/ModPilot)")
            .build()
            .expect("static reqwest client config is always valid")
    })
}

/// Searches FTB's public modpacks.
///
/// FTB's search returns bare IDs, so every hit costs a second request to
/// become displayable. They're fetched concurrently, and a pack that fails
/// to load is dropped rather than failing the whole search - one unlisted
/// or broken pack shouldn't blank the results.
pub async fn search_packs(query: &str) -> Result<Vec<FtbPack>, String> {
    let ids = fetch_pack_ids(format!("{BASE_URL}/modpack/search/20?term={}", urlencode(query))).await?;
    fetch_packs(ids).await
}

/// The list shown before anyone has typed a search term.
///
/// FTB has no single "browse" endpoint, so this stitches two together:
/// its editorially featured packs first, then the most-installed ones.
/// A pack that appears in both is only shown once, at its earlier
/// position.
pub async fn browse_packs() -> Result<Vec<FtbPack>, String> {
    let featured = fetch_pack_ids(format!("{BASE_URL}/modpack/featured/10"))
        .await
        .unwrap_or_default();
    let popular = fetch_pack_ids(format!("{BASE_URL}/modpack/popular/installs/20"))
        .await
        .unwrap_or_default();

    let mut seen = HashSet::new();
    let ids: Vec<i64> = featured
        .into_iter()
        .chain(popular)
        .filter(|id| seen.insert(*id))
        .take(24)
        .collect();

    if ids.is_empty() {
        return Err("Failed to load modpacks from FTB".to_string());
    }
    fetch_packs(ids).await
}

/// FTB's list endpoints all answer with bare pack IDs under `packs`.
async fn fetch_pack_ids(url: String) -> Result<Vec<i64>, String> {
    let response = client()
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Failed to reach FTB: {e}"))?
        .error_for_status()
        .map_err(|e| format!("FTB search failed: {e}"))?;

    let parsed: FtbSearchResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse FTB's response: {e}"))?;
    Ok(parsed.packs)
}

/// Turns bare pack IDs into displayable packs, concurrently.
///
/// A pack that fails to load is dropped rather than failing the whole
/// list - one unlisted or broken pack shouldn't blank the results. The
/// caller's ID order is preserved: it carries FTB's own relevance or
/// popularity ranking, which a `JoinSet`'s completion order would lose.
async fn fetch_packs(ids: Vec<i64>) -> Result<Vec<FtbPack>, String> {
    let mut tasks = tokio::task::JoinSet::new();
    for (rank, id) in ids.into_iter().enumerate() {
        tasks.spawn(async move { get_pack(id).await.ok().map(|pack| (rank, pack)) });
    }

    let mut packs = Vec::new();
    while let Some(result) = tasks.join_next().await {
        if let Ok(Some(ranked)) = result {
            packs.push(ranked);
        }
    }
    packs.sort_by_key(|(rank, _)| *rank);

    Ok(packs.into_iter().map(|(_, pack)| pack).collect())
}

/// Percent-encodes a search term for a query string. Pulling in a crate
/// for this would be overkill - a modpack name only ever needs the
/// unreserved set left alone.
fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn not_found_message(what: &str) -> String {
    format!(
        "{what} could not be found on FTB - it may have been removed or made private. Try \
         searching for it again."
    )
}

/// Fetches a pack, mapping FTB's "HTTP 200 with an error body" convention
/// onto a real error.
///
/// FTB answers a missing pack with `{"status":"error"}` and a 200 rather
/// than a 404, so deserializing straight into [`FtbPack`] would fail with a
/// confusing "missing field `name`" instead of saying what happened.
async fn get_json(url: String, missing: &str) -> Result<serde_json::Value, String> {
    let response = client()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to reach FTB: {e}"))?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(not_found_message(missing));
    }
    let body: serde_json::Value = response
        .error_for_status()
        .map_err(|e| format!("Failed to load from FTB: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse FTB's response: {e}"))?;

    if body.get("status").and_then(|s| s.as_str()) == Some("error") {
        return Err(not_found_message(missing));
    }
    Ok(body)
}

pub async fn get_pack(pack_id: i64) -> Result<FtbPack, String> {
    let body = get_json(format!("{BASE_URL}/modpack/{pack_id}"), "This modpack").await?;
    let mut pack: FtbPack = serde_json::from_value(body)
        .map_err(|e| format!("Failed to parse FTB's response: {e}"))?;
    pack.icon_url = pack.pick_icon();
    Ok(pack)
}

pub async fn get_version(pack_id: i64, version_id: i64) -> Result<FtbVersionManifest, String> {
    let body = get_json(
        format!("{BASE_URL}/modpack/{pack_id}/{version_id}"),
        "This modpack version",
    )
    .await?;
    serde_json::from_value(body).map_err(|e| format!("Failed to parse FTB's response: {e}"))
}

/// Maps an FTB modloader target onto ModpackPilot's own loader enum.
pub fn loader_from_target(target: &FtbTarget) -> ServerLoader {
    ServerLoader::from(target.name.to_lowercase().as_str())
}

/// Summarizes what installing a version would do, without downloading a
/// byte of it. Everything here comes from the manifest's `targets` and
/// `files` metadata, which is why an FTB import can show a review screen
/// as fast as a local folder can.
pub fn preview(manifest: &FtbVersionManifest) -> FtbVersionPreview {
    let loader_target = manifest.loader_target();
    let loader = loader_target
        .map(loader_from_target)
        .unwrap_or(ServerLoader::Vanilla);

    let server_files: Vec<_> = manifest.server_files().collect();
    let mut warnings = Vec::new();

    if loader == ServerLoader::Unknown {
        warnings.push(format!(
            "This pack targets \"{}\", which ModpackPilot doesn't know how to install \
             automatically. Its files will still be downloaded, but you'll have to set up the \
             server jar yourself.",
            loader_target.map(|t| t.name.as_str()).unwrap_or("an unknown loader"),
        ));
    }

    let unreachable = server_files
        .iter()
        .filter(|f| f.download_urls().is_empty())
        .count();
    if unreachable > 0 {
        warnings.push(format!(
            "{unreachable} of this version's files have no download link published by FTB. The \
             install will stop rather than leave the pack half-installed."
        ));
    }

    FtbVersionPreview {
        version_id: manifest.id,
        version_name: manifest.name.clone(),
        minecraft_version: manifest.minecraft_version(),
        loader,
        loader_version: loader_target.map(|t| t.version.clone()),
        java_major: manifest
            .target("java")
            .and_then(|t| crate::java::parse_java_major(&t.version)),
        mod_count: server_files.iter().filter(|f| f.file_type == "mod").count(),
        total_files: server_files.len(),
        download_size_bytes: server_files.iter().map(|f| f.size.max(0)).sum(),
        warnings,
    }
}

/// Picks the version to treat as "the latest" for an instance: the newest
/// one whose targets match its loader and Minecraft version.
///
/// Deliberately mirrors `modrinth::pick_latest_for_instance`, including its
/// refusal to fall back to "newest overall" - silently offering a 1.21
/// build to a 1.20.1 server is worse than reporting that nothing
/// compatible was published, and `list_ftb_versions` still lets the
/// operator pick one by hand.
pub fn pick_latest_for_instance(
    versions: &[FtbVersionSummary],
    loader: Option<&str>,
    minecraft_version: Option<&str>,
) -> Option<FtbVersionSummary> {
    let mut ordered = versions.to_vec();
    ordered.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| b.id.cmp(&a.id)));
    ordered.into_iter().find(|v| {
        let loader_ok = loader.is_none_or(|l| {
            v.loader_target()
                .is_some_and(|t| t.name.eq_ignore_ascii_case(l))
        });
        let mc_ok = minecraft_version.is_none_or(|mc| v.minecraft_version() == Some(mc));
        loader_ok && mc_ok
    })
}

/// Downloads a file, trying each published mirror before giving up.
async fn download_bytes(urls: &[String]) -> Result<Vec<u8>, String> {
    let mut last_error = "no download URL was published for this file".to_string();
    for url in urls {
        match client().get(url).send().await {
            Ok(response) => match response.error_for_status() {
                Ok(response) => match response.bytes().await {
                    Ok(bytes) => return Ok(bytes.to_vec()),
                    Err(e) => last_error = e.to_string(),
                },
                Err(e) => last_error = e.to_string(),
            },
            Err(e) => last_error = e.to_string(),
        }
    }
    Err(last_error)
}

fn sha1_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Downloads a pack version's server files into an instance and records
/// them in the shared pack manifest.
///
/// Used by both the initial import and an in-place update: the only
/// difference is whether `server/` already has anything in it, which
/// `prune_stale` handles on its own. Protected paths are skipped, so this
/// can never overwrite a live world or the operator's own
/// `server.properties`.
///
/// Everything is downloaded into a staging directory first and only moved
/// into `server/` once every file has arrived and passed its checksum. That
/// ordering is what makes a failed update survivable: the overwhelming
/// majority of failures here are network or checksum failures, and those
/// now happen while the live instance is still completely untouched.
///
/// `on_progress` is called with `(done, total)` as each file lands.
pub async fn install_files<F>(
    instance_dir: &Path,
    manifest: &FtbVersionManifest,
    world_folder_name: &str,
    on_progress: F,
) -> Result<(), String>
where
    F: Fn(usize, usize) + Send + Sync + 'static,
{
    let server_dir = instance_dir.join("server");
    let staging = instance_dir.join(STAGING_DIR);
    let _ = tokio::fs::remove_dir_all(&staging).await;

    // Resolve everything worth installing up front, so the total the
    // progress bar counts against is exact rather than an estimate.
    let planned: Vec<_> = manifest
        .server_files()
        .map(|file| (file.relative_path(), file))
        .filter(|(relative, _)| !is_protected_path(relative, world_folder_name))
        .collect();

    // Refuse a pack that can't be installed completely, rather than
    // producing a server missing mods that crashes confusingly on first
    // boot instead of failing here where the cause is obvious.
    if let Some((relative, _)) = planned.iter().find(|(_, f)| f.download_urls().is_empty()) {
        return Err(format!(
            "FTB didn't publish a download link for \"{relative}\", so this version can't be \
             installed completely. This usually means the pack is mid-update on FTB's side - try \
             again later, or pick a different version."
        ));
    }

    let total = planned.len();
    let on_progress = Arc::new(on_progress);
    let completed = Arc::new(AtomicUsize::new(0));
    let semaphore = Arc::new(Semaphore::new(DOWNLOAD_CONCURRENCY));

    let mut tasks = tokio::task::JoinSet::new();
    for (relative, file) in &planned {
        let relative = relative.clone();
        let urls: Vec<String> = file.download_urls().into_iter().map(str::to_string).collect();
        let expected_sha1 = file.sha1.to_lowercase();
        let dest = join_relative(&staging, &relative);
        let semaphore = semaphore.clone();
        let completed = completed.clone();
        let on_progress = on_progress.clone();

        tasks.spawn(async move {
            let _permit = semaphore
                .acquire()
                .await
                .map_err(|e| format!("Download scheduling failed: {e}"))?;

            let bytes = download_bytes(&urls)
                .await
                .map_err(|e| format!("Failed to download {relative}: {e}"))?;

            // FTB publishes a SHA-1 per file. A mismatch means a truncated
            // or tampered download, and writing it anyway produces a
            // corrupt jar that fails at runtime rather than here.
            if !expected_sha1.is_empty() {
                let actual = sha1_hex(&bytes);
                if actual != expected_sha1 {
                    return Err(format!(
                        "{relative} failed its checksum (expected {expected_sha1}, got {actual}) - \
                         the download was corrupted."
                    ));
                }
            }

            if let Some(parent) = dest.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
            }
            tokio::fs::write(&dest, bytes)
                .await
                .map_err(|e| format!("Failed to write {}: {e}", dest.display()))?;

            let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
            on_progress(done, total);
            Ok::<(), String>(())
        });
    }

    // Abort the rest on the first failure, but keep draining the set so no
    // task is left detached in the middle of a write.
    let mut first_error: Option<String> = None;
    while let Some(joined) = tasks.join_next().await {
        let failure = match joined {
            Ok(Ok(())) => continue,
            Ok(Err(e)) => e,
            Err(e) if e.is_cancelled() => continue,
            Err(e) => format!("Download task failed: {e}"),
        };
        if first_error.is_none() {
            first_error = Some(failure);
            tasks.abort_all();
        }
    }
    if let Some(e) = first_error {
        // Nothing has been moved into `server/` yet, so discarding the
        // staged files is the whole cleanup.
        let _ = tokio::fs::remove_dir_all(&staging).await;
        return Err(e);
    }

    let result = apply_staged_pack(instance_dir, &server_dir, &staging, world_folder_name).await;
    let _ = tokio::fs::remove_dir_all(&staging).await;
    result.map(|_installed| ())
}

/// Live-network checks against FTB's real API and the loader mavens.
///
/// Ignored by default so an offline build (or a CI box with no egress)
/// doesn't fail over someone else's uptime - run with
/// `cargo test -- --ignored` when touching the response shapes these
/// parse, which is exactly when a stale assumption would otherwise ship.
#[cfg(test)]
mod live_tests {
    use super::*;

    /// The browse list is what the import wizard shows before anyone types,
    /// so an empty one would leave the picker looking broken.
    #[tokio::test]
    #[ignore]
    async fn browses_packs_without_a_search_term() {
        let packs = browse_packs().await.expect("ftb browse");
        assert!(!packs.is_empty());
        assert!(packs.iter().any(|p| p.icon_url.is_some()), "packs should resolve icons");
    }
    /// FTB Presents Direwolf20 1.21, and a version of it known to exist.
    const PACK_ID: i64 = 126;
    const VERSION_ID: i64 = 100464;

    #[tokio::test]
    #[ignore]
    async fn reads_a_real_pack_and_version() {
        let pack = get_pack(PACK_ID).await.expect("pack fetch");
        assert_eq!(pack.id, PACK_ID);
        assert!(!pack.name.is_empty());
        assert!(!pack.versions.is_empty(), "pack should list versions");
        assert!(pack.icon_url.is_some(), "pack should resolve an icon");

        let manifest = get_version(PACK_ID, VERSION_ID).await.expect("version fetch");
        assert!(!manifest.files.is_empty(), "version should list files");

        let preview = preview(&manifest);
        assert_eq!(preview.loader, ServerLoader::NeoForge);
        assert_eq!(preview.minecraft_version.as_deref(), Some("1.21.1"));
        assert!(preview.loader_version.is_some());
        assert!(preview.mod_count > 0, "a kitchen-sink pack has mods");
        assert!(preview.download_size_bytes > 0);
        assert!(
            preview.warnings.is_empty(),
            "unexpected warnings: {:?}",
            preview.warnings,
        );

        // Every server file must be reachable - this is the assumption the
        // installer refuses to proceed without.
        for file in manifest.server_files() {
            assert!(
                !file.download_urls().is_empty(),
                "{} has no download URL",
                file.relative_path(),
            );
        }
    }

    #[tokio::test]
    #[ignore]
    async fn a_missing_pack_reports_itself_as_missing() {
        // FTB answers this with HTTP 200 and an error body rather than a
        // 404, which is the whole reason `get_json` exists.
        let err = get_pack(999_999_999).await.expect_err("should not resolve");
        assert!(err.contains("could not be found"), "unhelpful error: {err}");
    }

    /// A scratch directory that cleans up after itself.
    fn temp_instance_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("modpackpilot-test-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("server")).expect("temp dir");
        dir
    }

    /// Keeps the `n` smallest server files, so a real end-to-end install
    /// can be exercised without pulling a multi-gigabyte modpack.
    fn trimmed(manifest: &FtbVersionManifest, n: usize) -> FtbVersionManifest {
        let mut files: Vec<_> = manifest.server_files().cloned().collect();
        files.sort_by_key(|f| f.size);
        files.truncate(n);
        FtbVersionManifest { files, ..manifest.clone() }
    }

    /// The real download path: concurrency, checksum verification, the
    /// path layout, the manifest, and stale cleanup - against FTB's actual
    /// CDN rather than a mock that could drift from it.
    #[tokio::test]
    #[ignore]
    async fn installs_real_files_and_cleans_up_stale_ones() {
        let manifest = get_version(PACK_ID, VERSION_ID).await.expect("version fetch");
        let mut small = trimmed(&manifest, 5);
        assert_eq!(small.files.len(), 5);

        // A file the operator's live server owns, which an install must
        // never overwrite even though the pack ships its own copy.
        let mut protected = small.files[0].clone();
        protected.path = ".".to_string();
        protected.name = "server.properties".to_string();
        small.files.push(protected);

        let instance_dir = temp_instance_dir("install");
        let server_dir = instance_dir.join("server");
        std::fs::write(server_dir.join("server.properties"), b"motd=mine").expect("seed");

        install_files(&instance_dir, &small, "world", |_, _| {})
            .await
            .expect("install");

        for file in small.files.iter().take(5) {
            let landed = server_dir.join(file.relative_path().replace('/', "\\"));
            assert!(landed.is_file(), "{} was not written", file.relative_path());
        }
        assert_eq!(
            std::fs::read(server_dir.join("server.properties")).expect("read"),
            b"motd=mine",
            "an install overwrote live server state",
        );

        // Installing a version that no longer lists a file removes it,
        // because the manifest recorded that we were the ones who put it
        // there.
        let dropped = small.files[4].relative_path();
        let shrunk = FtbVersionManifest { files: small.files[..4].to_vec(), ..small.clone() };
        install_files(&instance_dir, &shrunk, "world", |_, _| {})
            .await
            .expect("second install");
        assert!(
            !server_dir.join(dropped.replace('/', "\\")).exists(),
            "{dropped} should have been pruned",
        );

        let _ = std::fs::remove_dir_all(&instance_dir);
    }

    /// A corrupted download must fail loudly here rather than become a
    /// broken jar that only explodes at server start - and, because the
    /// pack is staged before it is committed, it must leave the live
    /// instance completely untouched rather than half-updated.
    #[tokio::test]
    #[ignore]
    async fn a_failed_download_leaves_the_instance_untouched() {
        let manifest = get_version(PACK_ID, VERSION_ID).await.expect("version fetch");
        // Several files, with only the largest (downloaded last, most
        // likely) corrupted - so the others really have been fetched by the
        // time the failure hits.
        let mut small = trimmed(&manifest, 4);
        small.files[3].sha1 = "0000000000000000000000000000000000000000".to_string();

        let instance_dir = temp_instance_dir("checksum");
        let server_dir = instance_dir.join("server");
        std::fs::write(server_dir.join("server.properties"), b"motd=mine").expect("seed");

        let err = install_files(&instance_dir, &small, "world", |_, _| {})
            .await
            .expect_err("should reject a bad checksum");
        assert!(err.contains("checksum"), "unexpected error: {err}");

        // Nothing was committed: the seeded file is the only thing there,
        // and the staging area cleaned up after itself.
        let left: Vec<_> = std::fs::read_dir(&server_dir)
            .expect("read dir")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(left, vec!["server.properties".to_string()], "server/ was modified");
        assert!(!instance_dir.join(crate::packs::STAGING_DIR).exists(), "staging left behind");

        let _ = std::fs::remove_dir_all(&instance_dir);
    }

    /// The version list carries `targets`, which is what lets an update
    /// check match loader/Minecraft version without fetching every version
    /// individually. If FTB ever drops them, update checks would silently
    /// stop finding anything.
    #[tokio::test]
    #[ignore]
    async fn published_versions_carry_their_targets() {
        let pack = get_pack(PACK_ID).await.expect("pack fetch");
        let newest = pack.versions_newest_first();
        let latest = newest.first().expect("at least one version");

        assert!(latest.minecraft_version().is_some(), "no minecraft target");
        assert!(latest.loader_target().is_some(), "no modloader target");

        // And the matcher agrees with what the manifest itself reports.
        let manifest = get_version(PACK_ID, latest.id).await.expect("version fetch");
        let preview = preview(&manifest);
        let picked = pick_latest_for_instance(
            &pack.versions,
            Some(preview.loader.as_str()),
            preview.minecraft_version.as_deref(),
        )
        .expect("its own version should match");
        assert_eq!(picked.id, latest.id);
    }

    /// The loader installers are downloaded from official mavens, so a
    /// changed URL layout there breaks every FTB install. Checked with a
    /// real request rather than assumed.
    #[tokio::test]
    #[ignore]
    async fn loader_installers_are_where_we_expect() {
        let urls = [
            "https://maven.minecraftforge.net/net/minecraftforge/forge/1.20.1-47.2.20/forge-1.20.1-47.2.20-installer.jar",
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.248/neoforge-21.1.248-installer.jar",
            "https://meta.fabricmc.net/v2/versions/loader/1.20.1/0.16.9/1.0.1/server/jar",
        ];
        for url in urls {
            let status = client().head(url).send().await.expect("request").status();
            assert!(status.is_success(), "{url} answered {status}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(id: i64, updated: i64, mc: &str, loader: Option<(&str, &str)>) -> FtbVersionSummary {
        let mut targets = vec![FtbTarget {
            name: "minecraft".to_string(),
            version: mc.to_string(),
            target_type: "game".to_string(),
        }];
        if let Some((name, ver)) = loader {
            targets.push(FtbTarget {
                name: name.to_string(),
                version: ver.to_string(),
                target_type: "modloader".to_string(),
            });
        }
        FtbVersionSummary {
            id,
            name: format!("v{id}"),
            version_type: "release".to_string(),
            updated,
            targets,
        }
    }

    #[test]
    fn picks_the_newest_compatible_version() {
        let versions = vec![
            version(1, 100, "1.20.1", Some(("forge", "47.2.0"))),
            version(3, 300, "1.21.1", Some(("neoforge", "21.1.248"))),
            version(2, 200, "1.20.1", Some(("forge", "47.3.0"))),
        ];

        // Newest matching wins, not newest overall.
        let picked = pick_latest_for_instance(&versions, Some("forge"), Some("1.20.1"));
        assert_eq!(picked.map(|v| v.id), Some(2));

        // A loader mismatch is never silently upgraded across - offering the
        // 1.21.1 NeoForge build to a 1.20.1 Forge server would break it.
        assert!(pick_latest_for_instance(&versions, Some("fabric"), Some("1.20.1")).is_none());
        assert!(pick_latest_for_instance(&versions, Some("forge"), Some("1.19.2")).is_none());

        // An instance whose loader/version is unknown constrains nothing.
        assert_eq!(pick_latest_for_instance(&versions, None, None).map(|v| v.id), Some(3));
    }

    #[test]
    fn a_vanilla_version_has_no_loader_to_match() {
        let versions = vec![version(1, 100, "1.21.1", None)];
        assert!(pick_latest_for_instance(&versions, Some("forge"), Some("1.21.1")).is_none());
        assert_eq!(
            pick_latest_for_instance(&versions, None, Some("1.21.1")).map(|v| v.id),
            Some(1),
        );
    }
}
