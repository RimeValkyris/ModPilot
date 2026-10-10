//! Installing a mod loader's server files into an instance.
//!
//! A modpack's file list is mods and configs - never the server itself.
//! Forge and NeoForge ship an installer jar that has to be *run* to produce
//! a runnable server (that is the entire reason FTB's own
//! `serverinstall_*.exe` exists), and Fabric publishes a ready-made launch
//! jar instead. This module is what turns "we have the pack files" into
//! "there is something to launch".
//!
//! Installer URLs are always built here from the official maven
//! coordinates, never taken from a modpack API's response: this downloads
//! and executes a jar, so where that jar comes from is not something a
//! third-party payload gets to decide.

use std::path::{Path, PathBuf};

use sha2::Digest;
use crate::importer;
use crate::models::ServerLoader;

const FORGE_MAVEN: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge";
const NEOFORGE_MAVEN: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge";
const FABRIC_META: &str = "https://meta.fabricmc.net/v2/versions";
const PAPER_FILL: &str = "https://fill.papermc.io/v3/projects/paper";
const PURPUR_API: &str = "https://api.purpurmc.org/v2/purpur";
const SPIGOT_VERSIONS: &str = "https://hub.spigotmc.org/versions/";
const BUILDTOOLS_URL: &str =
    "https://hub.spigotmc.org/jenkins/job/BuildTools/lastSuccessfulBuild/artifact/target/BuildTools.jar";
const MOJANG_MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

/// The Fabric *installer* version used to build a server launch jar URL.
/// Pinned deliberately - Fabric's meta endpoint wants an explicit version
/// here, and silently tracking "latest" would change what gets installed
/// without anything in ModpackPilot changing.
const FABRIC_INSTALLER_VERSION: &str = "1.0.1";

/// What an install produced, ready to be written onto the instance.
#[derive(Debug)]
pub struct InstalledLoader {
    /// Relative to the instance's `server/` directory.
    pub server_jar: String,
    /// `"jar"`, `"argfile"`, or `"script"` - see `Instance::launch_mode`.
    pub launch_mode: String,
    /// The exact build the install resolved to, when the loader has one
    /// worth recording (Paper's "latest" becomes a concrete build number).
    pub loader_version: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct PaperBuild {
    id: u32,
    downloads: std::collections::HashMap<String, PaperDownload>,
}

#[derive(Debug, serde::Deserialize)]
struct PaperDownload {
    url: String,
    checksums: PaperChecksums,
}

#[derive(Debug, serde::Deserialize)]
struct PaperChecksums {
    sha256: String,
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("ModpackPilot/", env!("CARGO_PKG_VERSION"), " (+https://github.com/RimeValkyris/ModPilot)"))
        // Every download here is checked to start with https://, which a
        // redirect would otherwise sidestep by bouncing to plain http.
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.url().scheme() == "https" && attempt.previous().len() < 10 {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .expect("static reqwest client config is always valid")
}

/// Whether this loader can be installed automatically at all. Used to fail
/// an FTB import early, before anything is downloaded, rather than after.
pub fn is_supported(loader: ServerLoader) -> bool {
    matches!(
        loader,
        ServerLoader::Forge
            | ServerLoader::NeoForge
            | ServerLoader::Fabric
            | ServerLoader::Paper
            | ServerLoader::Purpur
    )
}

/// Whether a version string is safe to put in a URL path or a file name:
/// short, made only of letters, digits and `.`/`_`/`-`/`+`, and unable to
/// form `..` or start as a command-line option. Real versions
/// (`1.21.4`, `24w05a`, `47.4.0`, `1.7.10-10.13.4.1614`) all pass; path
/// separators, query characters and traversal do not.
pub fn is_safe_version_string(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && !value.starts_with(['-', '.'])
        && !value.contains("..")
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
}

/// Sort key that orders `1.9` before `1.21.4` and `26.1`, which plain
/// string comparison gets wrong.
fn version_key(version: &str) -> Vec<u32> {
    version.split('.').map(|part| part.parse().unwrap_or(0)).collect()
}

/// Release versions only - snapshots, pre-releases and release candidates
/// all carry a `-` or a letter, and none of them belong in a server picker.
fn is_release(version: &str) -> bool {
    !version.is_empty() && version.chars().all(|c| c.is_ascii_digit() || c == '.')
}

fn newest_first(mut versions: Vec<String>) -> Vec<String> {
    versions.retain(|v| is_release(v));
    versions.sort_by_key(|v| std::cmp::Reverse(version_key(v)));
    versions.dedup();
    versions
}

async fn get_json<T: serde::de::DeserializeOwned>(url: &str, what: &str) -> Result<T, String> {
    client()
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Failed to reach {what}: {e}"))?
        .error_for_status()
        .map_err(|e| format!("{what} returned an error: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Failed to read {what}'s response: {e}"))
}

/// Minecraft versions the vendor actually publishes a server for, newest
/// first. This is what the Create Server dropdown shows, so it comes from
/// the vendor rather than a hardcoded list that goes stale.
pub async fn list_versions(loader: ServerLoader) -> Result<Vec<String>, String> {
    match loader {
        ServerLoader::Vanilla => {
            #[derive(serde::Deserialize)]
            struct Manifest {
                versions: Vec<ManifestEntry>,
            }
            #[derive(serde::Deserialize)]
            struct ManifestEntry {
                id: String,
                #[serde(rename = "type")]
                kind: String,
            }
            let manifest: Manifest = get_json(MOJANG_MANIFEST, "Mojang").await?;
            Ok(newest_first(
                manifest
                    .versions
                    .into_iter()
                    .filter(|v| v.kind == "release")
                    .map(|v| v.id)
                    .collect(),
            ))
        }
        ServerLoader::Paper => {
            #[derive(serde::Deserialize)]
            struct Project {
                // Grouped by major line ("1.21": ["1.21.4", ...]).
                versions: std::collections::HashMap<String, Vec<String>>,
            }
            let project: Project = get_json(PAPER_FILL, "PaperMC").await?;
            Ok(newest_first(project.versions.into_values().flatten().collect()))
        }
        ServerLoader::Purpur => {
            #[derive(serde::Deserialize)]
            struct Project {
                versions: Vec<String>,
            }
            let project: Project = get_json(PURPUR_API, "PurpurMC").await?;
            Ok(newest_first(project.versions))
        }
        ServerLoader::Spigot | ServerLoader::Bukkit => {
            // Spigot has no JSON API; its version index is a plain directory
            // listing of `<version>.json` files, one per buildable release.
            let html = client()
                .get(SPIGOT_VERSIONS)
                .send()
                .await
                .map_err(|e| format!("Failed to reach SpigotMC: {e}"))?
                .error_for_status()
                .map_err(|e| format!("SpigotMC returned an error: {e}"))?
                .text()
                .await
                .map_err(|e| format!("Failed to read SpigotMC's response: {e}"))?;
            let re = regex::Regex::new(r#"href="(\d+(?:\.\d+)+)\.json""#).expect("static regex");
            Ok(newest_first(
                re.captures_iter(&html).map(|c| c[1].to_string()).collect(),
            ))
        }
        other => Err(format!("ModpackPilot can't list {} versions yet.", other.as_str())),
    }
}

/// Spigot and CraftBukkit have no official server download: the vendor ships
/// a build tool that compiles the jar on the operator's machine.
pub fn needs_build_tools(loader: ServerLoader) -> bool {
    matches!(loader, ServerLoader::Spigot | ServerLoader::Bukkit)
}

/// How long a BuildTools run may take before it is abandoned. A cold build
/// (git clone, decompile, patch, compile) takes 5-15 minutes on a modern
/// machine; this only exists so a hung build can't wedge the instance.
const BUILDTOOLS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45 * 60);

/// Builds a Spigot or CraftBukkit server jar with SpigotMC's BuildTools.
///
/// BuildTools is downloaded from SpigotMC's own build server and executed.
/// SpigotMC publishes no checksum for it, so unlike every other download in
/// this module the jar cannot be verified - the trust here is HTTPS plus the
/// host, the same trust the vendor's own instructions ask for. The work
/// happens in a scratch folder next to the instance and is deleted
/// afterwards; only the finished jar is kept.
pub async fn build_with_build_tools(
    server_dir: &Path,
    loader: ServerLoader,
    minecraft_version: &str,
    java_path: &str,
    on_step: Option<InstallerStepFn<'_>>,
) -> Result<InstalledLoader, String> {
    if !needs_build_tools(loader) {
        return Err(format!("{} isn't built with BuildTools", loader.as_str()));
    }
    // The version becomes a command-line argument; digits and dots only
    // means it can never be read as an option (`--something`).
    if !is_release(minecraft_version) {
        return Err(format!("\"{minecraft_version}\" is not a valid Minecraft release version"));
    }

    let work_dir = server_dir
        .parent()
        .ok_or_else(|| "The server folder has no parent folder to build in".to_string())?
        .join("buildtools");
    if work_dir.exists() {
        tokio::fs::remove_dir_all(&work_dir)
            .await
            .map_err(|e| format!("Failed to clear an old BuildTools folder: {e}"))?;
    }
    tokio::fs::create_dir_all(&work_dir)
        .await
        .map_err(|e| format!("Failed to create the BuildTools folder: {e}"))?;
    tokio::fs::create_dir_all(server_dir)
        .await
        .map_err(|e| format!("Failed to create the server folder: {e}"))?;

    let result = run_build_tools(&work_dir, server_dir, loader, minecraft_version, java_path, on_step).await;
    // Always remove the scratch folder: it holds hundreds of MB of
    // decompiled sources, and a stale one would only get in the next build's way.
    let _ = tokio::fs::remove_dir_all(&work_dir).await;
    result
}

async fn run_build_tools(
    work_dir: &Path,
    server_dir: &Path,
    loader: ServerLoader,
    mc: &str,
    java_path: &str,
    on_step: Option<InstallerStepFn<'_>>,
) -> Result<InstalledLoader, String> {
    if let Some(on_step) = on_step {
        on_step("Downloading BuildTools…");
    }
    let bytes = download_bytes(BUILDTOOLS_URL, "BuildTools").await?;
    // Not a checksum, but enough to refuse an HTML error page saved as a jar.
    if bytes.len() < 100_000 || !bytes.starts_with(b"PK") {
        return Err("The BuildTools download isn't a valid jar".to_string());
    }
    let build_tools = work_dir.join("BuildTools.jar");
    tokio::fs::write(&build_tools, &bytes)
        .await
        .map_err(|e| format!("Failed to write BuildTools.jar: {e}"))?;

    let (compile_target, jar_prefix) = match loader {
        ServerLoader::Bukkit => ("craftbukkit", "craftbukkit"),
        _ => ("spigot", "spigot"),
    };

    let mut command = tokio::process::Command::new(java_path);
    // BuildTools runs Maven (and git) as child processes, and Maven's own JVM
    // downloads from repo.maven.apache.org using the JVM truststore - so a
    // flag on this JVM alone doesn't reach it. `JAVA_TOOL_OPTIONS` is read by
    // every JVM in the process tree. See `run_installer_jar_reporting` for
    // why the Windows root store is the right one to trust.
    #[cfg(windows)]
    {
        let trust = "-Djavax.net.ssl.trustStoreType=WINDOWS-ROOT";
        let options = match std::env::var("JAVA_TOOL_OPTIONS") {
            Ok(existing) if !existing.trim().is_empty() => format!("{existing} {trust}"),
            _ => trust.to_string(),
        };
        command.env("JAVA_TOOL_OPTIONS", options);
    }
    command
        .arg("-jar")
        .arg(&build_tools)
        .arg("--rev")
        .arg(mc)
        .arg("--compile")
        .arg(compile_target)
        .arg("--output-dir")
        .arg(server_dir)
        .current_dir(work_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        // If the timeout below fires (or the app closes), the build dies with it.
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);

    let mut child = command
        .spawn()
        .map_err(|e| format!("Failed to run BuildTools: {e}"))?;
    let stdout_pipe = child.stdout.take().expect("stdout is piped");
    let stderr_pipe = child.stderr.take().expect("stderr is piped");

    let run = async {
        let (stdout, stderr) = tokio::join!(
            collect_installer_output(stdout_pipe, on_step),
            collect_installer_output(stderr_pipe, on_step),
        );
        let status = child
            .wait()
            .await
            .map_err(|e| format!("Failed to run BuildTools: {e}"))?;
        Ok::<_, String>((status, stdout, stderr))
    };
    let (status, stdout, stderr) = tokio::time::timeout(BUILDTOOLS_TIMEOUT, run)
        .await
        .map_err(|_| "BuildTools took longer than 45 minutes and was stopped.".to_string())??;

    if !status.success() {
        let combined = format!("{stdout}{stderr}");
        // Same translation as the Forge installer: the raw message says
        // nothing about TLS interception, which is the usual cause.
        if combined.contains("PKIX") || combined.contains("unable to find valid certification path")
        {
            return Err(
                "BuildTools couldn't verify the HTTPS connections it needs (Maven Central, GitHub). This is usually antivirus TLS inspection or a corporate proxy using its own certificate. Allow-list repo.maven.apache.org and hub.spigotmc.org in that software, then try again."
                    .to_string(),
            );
        }
        // The last lines are usually a bare stack trace; the lines that say
        // what went wrong are the `[ERROR]` ones, so prefer those.
        let errors: Vec<&str> = combined
            .lines()
            .filter(|l| l.starts_with("[ERROR]") && l.trim() != "[ERROR]")
            .take(3)
            .collect();
        let detail = if errors.is_empty() {
            let tail = if stderr.trim().is_empty() { &stdout } else { &stderr };
            tail.lines().rev().take(5).collect::<Vec<_>>().join(" / ")
        } else {
            errors.join(" / ")
        };
        return Err(format!(
            "BuildTools failed (exit code {:?}): {detail}",
            status.code(),
        ));
    }

    let jar_name = format!("{jar_prefix}-{mc}.jar");
    if !server_dir.join(&jar_name).is_file() {
        return Err(format!(
            "BuildTools finished, but {jar_name} wasn't produced. Check the server folder."
        ));
    }
    Ok(InstalledLoader {
        server_jar: jar_name,
        launch_mode: "jar".to_string(),
        loader_version: None,
    })
}

async fn write_server_jar(server_dir: &Path, jar_name: &str, bytes: &[u8]) -> Result<(), String> {
    tokio::fs::create_dir_all(server_dir)
        .await
        .map_err(|e| format!("Failed to create the server folder: {e}"))?;
    tokio::fs::write(server_dir.join(jar_name), bytes)
        .await
        .map_err(|e| format!("Failed to write {jar_name}: {e}"))
}

async fn download_bytes(url: &str, what: &str) -> Result<Vec<u8>, String> {
    if !url.starts_with("https://") {
        return Err(format!("Refusing to download {what} over an insecure link"));
    }
    client()
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Failed to download {what}: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Failed to download {what}: {e}"))?
        .bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| format!("Failed to read the {what} download: {e}"))
}

async fn install_vanilla(server_dir: &Path, mc: &str) -> Result<InstalledLoader, String> {
    #[derive(serde::Deserialize)]
    struct Manifest {
        versions: Vec<Entry>,
    }
    #[derive(serde::Deserialize)]
    struct Entry {
        id: String,
        url: String,
    }
    #[derive(serde::Deserialize)]
    struct VersionMeta {
        downloads: Downloads,
    }
    #[derive(serde::Deserialize)]
    struct Downloads {
        server: Option<ServerDownload>,
    }
    #[derive(serde::Deserialize)]
    struct ServerDownload {
        url: String,
        sha1: String,
    }

    let manifest: Manifest = get_json(MOJANG_MANIFEST, "Mojang").await?;
    let entry = manifest
        .versions
        .into_iter()
        .find(|v| v.id == mc)
        .ok_or_else(|| format!("Mojang has no Minecraft version {mc}"))?;
    let meta: VersionMeta = get_json(&entry.url, "Mojang").await?;
    let download = meta
        .downloads
        .server
        .ok_or_else(|| format!("Mojang published no server download for Minecraft {mc}"))?;

    let bytes = download_bytes(&download.url, "the Minecraft server").await?;
    let actual = format!("{:x}", <sha1::Sha1 as sha1::Digest>::digest(&bytes));
    if actual != download.sha1.to_lowercase() {
        return Err(format!(
            "The Minecraft server download failed its SHA-1 checksum (expected {}, got {actual})",
            download.sha1
        ));
    }
    // `server.jar` is the name import detection already recognizes as vanilla.
    write_server_jar(server_dir, "server.jar", &bytes).await?;
    Ok(InstalledLoader {
        server_jar: "server.jar".to_string(),
        launch_mode: "jar".to_string(),
        loader_version: None,
    })
}

async fn install_purpur(
    server_dir: &Path,
    mc: &str,
    loader_version: &str,
) -> Result<InstalledLoader, String> {
    #[derive(serde::Deserialize)]
    struct Build {
        build: String,
    }
    let build = if loader_version.is_empty() || loader_version.eq_ignore_ascii_case("latest") {
        let latest: Build = get_json(&format!("{PURPUR_API}/{mc}/latest"), "PurpurMC")
            .await
            .map_err(|e| format!("PurpurMC has no build for Minecraft {mc}: {e}"))?;
        latest.build
    } else {
        loader_version.to_string()
    };
    // Build ids are numeric; anything else would be spliced into a URL.
    if build.is_empty() || !build.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("\"{build}\" is not a valid Purpur build number"));
    }
    // PurpurMC publishes only an MD5 per build. It is weaker than SHA-256,
    // but it still catches a truncated or swapped download.
    #[derive(serde::Deserialize)]
    struct BuildInfo {
        md5: String,
    }
    let info: BuildInfo = get_json(&format!("{PURPUR_API}/{mc}/{build}"), "PurpurMC")
        .await
        .map_err(|e| format!("PurpurMC has no build {build} for Minecraft {mc}: {e}"))?;
    let bytes = download_bytes(&format!("{PURPUR_API}/{mc}/{build}/download"), "Purpur").await?;
    let actual = format!("{:x}", <md5::Md5 as md5::Digest>::digest(&bytes));
    if actual != info.md5.to_lowercase() {
        return Err(format!(
            "The Purpur download failed its MD5 checksum (expected {}, got {actual})",
            info.md5
        ));
    }
    let jar_name = format!("purpur-{mc}-{build}.jar");
    write_server_jar(server_dir, &jar_name, &bytes).await?;
    Ok(InstalledLoader {
        server_jar: jar_name,
        launch_mode: "jar".to_string(),
        loader_version: Some(build),
    })
}

pub fn unsupported_message(loader: ServerLoader) -> String {
    format!(
        "ModpackPilot can't install a {} server automatically yet. The pack's files were \
         downloaded, but you'll need to put the server jar in place yourself.",
        loader.as_str(),
    )
}

/// Downloads the first URL that exists, for cases where the same artifact
/// has more than one possible coordinate (see `forge_installer_urls`).
///
/// Only a 404 moves on to the next candidate - a network error or a 500 is
/// reported as-is, since retrying a different name would just replace a real
/// failure with a misleading "version doesn't exist".
async fn download_first_available(urls: &[String], dest: &Path) -> Result<(), String> {
    let mut last: Option<String> = None;
    for url in urls {
        match download_to(url, dest).await {
            Ok(()) => return Ok(()),
            Err(e) if e.contains(NOT_PUBLISHED_MARKER) => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| "No installer URL to try".to_string()))
}

/// Marks the "maven has no such artifact" case so `download_first_available`
/// can tell it apart from a genuine failure.
const NOT_PUBLISHED_MARKER: &str = "doesn't exist on its official maven";

/// Downloads a URL to a path, failing loudly rather than leaving a partial
/// file behind for the installer to choke on.
async fn download_to(url: &str, dest: &Path) -> Result<(), String> {
    let response = client()
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Failed to download the installer: {e}"))?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(format!(
            "That loader version {NOT_PUBLISHED_MARKER} ({url}). The modpack may list a version \
             that was never published."
        ));
    }
    let bytes = response
        .error_for_status()
        .map_err(|e| format!("Failed to download the installer: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("Failed to read the downloaded installer: {e}"))?;

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
    }
    tokio::fs::write(dest, &bytes)
        .await
        .map_err(|e| format!("Failed to write {}: {e}", dest.display()))
}

/// Drains one of the installer's output pipes, reporting each line as it
/// arrives and returning the whole stream for error reporting.
async fn collect_installer_output<R>(reader: R, on_step: Option<InstallerStepFn<'_>>) -> String
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncBufReadExt;

    let mut lines = tokio::io::BufReader::new(reader).lines();
    let mut collected = String::new();
    while let Ok(Some(line)) = lines.next_line().await {
        let trimmed = line.trim();
        if let Some(on_step) = on_step {
            if !trimmed.is_empty() {
                on_step(trimmed);
            }
        }
        collected.push_str(&line);
        collected.push('\n');
    }
    collected
}

/// Runs a Forge/NeoForge installer jar in `--installServer` mode.
///
/// Shared by the FTB install path and the manual "Install Forge Server"
/// button, so both fail the same way with the same message. The installer's
/// own GUI is exactly what `--installServer` exists to skip, so this runs
/// fully headless.
pub async fn run_installer_jar(
    java_path: &str,
    installer_path: &Path,
    server_dir: &Path,
) -> Result<(), String> {
    run_installer_jar_reporting(java_path, installer_path, server_dir, None).await
}

/// A callback invoked with each line the installer prints, for callers that
/// want to show what it is doing while it runs.
pub type InstallerStepFn<'a> = &'a (dyn Fn(&str) + Send + Sync);

/// `run_installer_jar`, but streaming: every line the installer writes is
/// handed to `on_step` as it arrives instead of being collected silently.
///
/// The installer takes minutes (it downloads Minecraft's libraries and then
/// runs its own bytecode processors) and never reports a completion
/// percentage, so what it prints is the only real signal there is that it
/// hasn't stalled. Output is still buffered as well, because the error
/// messages below are built from it.
pub async fn run_installer_jar_reporting(
    java_path: &str,
    installer_path: &Path,
    server_dir: &Path,
    on_step: Option<InstallerStepFn<'_>>,
) -> Result<(), String> {
    let mut command = tokio::process::Command::new(java_path);

    // The installer downloads Minecraft's libraries over HTTPS using the
    // JVM's own truststore, which is `cacerts` - not the OS one. On any
    // machine where antivirus TLS inspection (AVG, ESET, Kaspersky) or a
    // corporate proxy terminates HTTPS with a private root CA, that CA is
    // installed in the Windows store and nowhere else, so every download
    // fails PKIX validation and the installer aborts with nothing more
    // useful than "A problem installing was detected". Pointing it at the
    // Windows root store is what every other Windows application already
    // trusts, and costs nothing on machines without interception.
    #[cfg(windows)]
    command.arg("-Djavax.net.ssl.trustStoreType=WINDOWS-ROOT");

    command
        .arg("-jar")
        .arg(installer_path)
        .arg("--installServer")
        .current_dir(server_dir);

    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW - nothing here should flash a console window.
        command.creation_flags(0x0800_0000);
    }

    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|e| format!("Failed to run the Forge/NeoForge installer: {e}"))?;

    let stdout_pipe = child.stdout.take().expect("stdout is piped");
    let stderr_pipe = child.stderr.take().expect("stderr is piped");

    let (stdout, stderr) = tokio::join!(
        collect_installer_output(stdout_pipe, on_step),
        collect_installer_output(stderr_pipe, on_step),
    );

    let status = child
        .wait()
        .await
        .map_err(|e| format!("Failed to run the Forge/NeoForge installer: {e}"))?;

    if !status.success() {
        let combined = format!("{stdout}{stderr}");
        let tail = if stderr.trim().is_empty() { &stdout } else { &stderr };

        // The installer's own message for a TLS failure says nothing about
        // TLS, so translate it rather than passing the confusion along.
        if combined.contains("PKIX") || combined.contains("unable to find valid certification path")
        {
            return Err(
                "The loader installer couldn't verify the HTTPS connections it needs to download Minecraft's libraries. This is usually antivirus TLS inspection or a corporate proxy intercepting the connection with its own certificate. Allow-list maven.neoforged.net and maven.minecraftforge.net in that software, or install the loader manually, then try again."
                    .to_string(),
            );
        }

        return Err(format!(
            "Forge/NeoForge installer failed (exit code {:?}): {}",
            status.code(),
            tail.lines().rev().take(5).collect::<Vec<_>>().join(" / "),
        ));
    }

    Ok(())
}

/// Re-reads a server directory after an install and reports what should be
/// launched. Separate from the install itself because the answer is
/// whatever ended up on disk, not whatever we intended to put there.
/// Reads the Minecraft version out of an installer jar's
/// `install_profile.json`.
///
/// This is the authoritative answer, and the only one for a pack whose
/// installer has been renamed: detection reads the version out of the
/// *file name* (`forge-1.20.1-47.4.0-installer.jar`), which a pack author
/// who shipped it as `installer.jar` has thrown away. Without a Minecraft
/// version there is nothing to pick a Java version from, and the install
/// is refused rather than run under whatever `java` is first on PATH.
///
/// Both layouts are handled: modern installers put `minecraft` at the top
/// level, 1.12-era ones nest it under `install`. Anything unreadable is
/// `None` - this is a best-effort improvement on the file name, never a
/// hard requirement.
pub fn minecraft_version_from_installer(installer_path: &Path) -> Option<String> {
    let file = std::fs::File::open(installer_path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let entry = archive.by_name("install_profile.json").ok()?;

    let profile: serde_json::Value = serde_json::from_reader(entry).ok()?;
    let version = profile
        .get("minecraft")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            profile
                .get("install")
                .and_then(|install| install.get("minecraft"))
                .and_then(serde_json::Value::as_str)
        })?;

    let version = version.trim();
    (!version.is_empty()).then(|| version.to_string())
}

pub async fn detect_installed(server_dir: &Path) -> Result<InstalledLoader, String> {
    let dir = server_dir.to_path_buf();
    let detected = tauri::async_runtime::spawn_blocking(move || importer::detect_from_dir(&dir))
        .await
        .map_err(|e| format!("Detection task failed: {e}"))?;

    let launch_mode = detected.launch_mode().to_string();
    let server_jar = detected.server_jar.ok_or_else(|| {
        "The installer finished, but ModpackPilot couldn't find the resulting server files. \
         Check this instance's server folder manually."
            .to_string()
    })?;

    Ok(InstalledLoader {
        server_jar,
        launch_mode,
        loader_version: None,
    })
}

/// Installs a loader's server into `server_dir`.
///
/// Forge and NeoForge download their official installer and run it;
/// Fabric downloads a ready-made server launch jar from Fabric's meta
/// service and needs no Java to install at all. The installer jar is
/// removed afterwards so it can't later be mistaken for the server itself
/// (`importer::detect` deliberately refuses to launch an `*-installer.jar`).
pub async fn install(
    server_dir: &Path,
    loader: ServerLoader,
    loader_version: &str,
    minecraft_version: Option<&str>,
    java_path: &str,
) -> Result<InstalledLoader, String> {
    // Both values are spliced into download URLs and (for Paper and Purpur)
    // into the jar's file name. They come from the database, which holds
    // whatever an import, a modpack API or a typed-in field supplied - so
    // check them once here rather than trusting each arm to.
    for (label, value) in [
        ("Minecraft version", minecraft_version.unwrap_or("")),
        ("loader version", loader_version),
    ] {
        if !value.is_empty() && !is_safe_version_string(value) {
            return Err(format!("\"{value}\" is not a valid {label}"));
        }
    }
    return match loader {
        ServerLoader::Forge => {
            let mc = minecraft_version.ok_or_else(|| {
                "Forge needs to know which Minecraft version to install for, and this pack \
                 didn't say."
                    .to_string()
            })?;
            install_via_installer(
                server_dir,
                &forge_installer_urls(mc, loader_version),
                "forge-installer.jar",
                java_path,
            )
            .await
        }
        ServerLoader::NeoForge => {
            // NeoForge versions already encode the Minecraft version
            // (21.1.248 is 1.21.1), so it takes no separate coordinate.
            let url =
                format!("{NEOFORGE_MAVEN}/{loader_version}/neoforge-{loader_version}-installer.jar");
            install_via_installer(server_dir, &[url], "neoforge-installer.jar", java_path).await
        }
        ServerLoader::Fabric => {
            let mc = minecraft_version.ok_or_else(|| {
                "Fabric needs to know which Minecraft version to install for, and this pack \
                 didn't say."
                    .to_string()
            })?;
            // Fabric publishes a prebuilt launch jar, so there's nothing to
            // execute here - just a download.
            let url = format!(
                "{FABRIC_META}/loader/{mc}/{loader_version}/{FABRIC_INSTALLER_VERSION}/server/jar"
            );
            let jar_name = "fabric-server-launch.jar";
            download_to(&url, &server_dir.join(jar_name)).await?;
            Ok(InstalledLoader {
                server_jar: jar_name.to_string(),
                launch_mode: "jar".to_string(),
                loader_version: None,
            })
        }
        ServerLoader::Paper => {
            let mc = minecraft_version.ok_or_else(|| {
                "Paper needs to know which Minecraft version to install for, and this instance \
                 didn't say."
                    .to_string()
            })?;
            let build = if loader_version.is_empty() || loader_version.eq_ignore_ascii_case("latest") {
                "latest".to_string()
            } else {
                loader_version.to_string()
            };
            let response = client()
                .get(format!("{PAPER_FILL}/versions/{mc}/builds/{build}"))
                .send()
                .await
                .map_err(|e| format!("Failed to reach PaperMC: {e}"))?
                .error_for_status()
                .map_err(|e| format!("PaperMC did not publish a build for Minecraft {mc}: {e}"))?;
            let metadata: PaperBuild = response
                .json()
                .await
                .map_err(|e| format!("Failed to parse PaperMC's response: {e}"))?;
            let download = metadata
                .downloads
                .get("server:default")
                .ok_or_else(|| format!("PaperMC has no server download for Minecraft {mc}"))?;
            if !download.url.starts_with("https://") {
                return Err("Refusing to download Paper over an insecure link".to_string());
            }
            let expected = download.checksums.sha256.to_lowercase();
            let bytes = client()
                .get(&download.url)
                .send()
                .await
                .map_err(|e| format!("Failed to download Paper: {e}"))?
                .error_for_status()
                .map_err(|e| format!("Failed to download Paper: {e}"))?
                .bytes()
                .await
                .map_err(|e| format!("Failed to read the Paper download: {e}"))?;
            let actual = format!("{:x}", sha2::Sha256::digest(&bytes));
            if actual != expected {
                return Err(format!(
                    "Paper download failed its SHA-256 checksum (expected {expected}, got {actual})"
                ));
            }
            let jar_name = format!("paper-{mc}-{}.jar", metadata.id);
            tokio::fs::create_dir_all(server_dir)
                .await
                .map_err(|e| format!("Failed to create the server folder: {e}"))?;
            tokio::fs::write(server_dir.join(&jar_name), &bytes)
                .await
                .map_err(|e| format!("Failed to write Paper server: {e}"))?;
            Ok(InstalledLoader {
                server_jar: jar_name,
                launch_mode: "jar".to_string(),
                loader_version: Some(metadata.id.to_string()),
            })
        }
        ServerLoader::Purpur => {
            let mc = minecraft_version.ok_or_else(|| {
                "Purpur needs to know which Minecraft version to install for, and this instance \
                 didn't say."
                    .to_string()
            })?;
            install_purpur(server_dir, mc, loader_version).await
        }
        ServerLoader::Vanilla => {
            let mc = minecraft_version.ok_or_else(|| {
                "Vanilla needs to know which Minecraft version to install, and this instance \
                 didn't say."
                    .to_string()
            })?;
            install_vanilla(server_dir, mc).await
        }
        other => Err(unsupported_message(other)),
    };
}

/// Every coordinate a Forge installer might live under, most likely first.
///
/// Forge's maven keys artifacts by `<mc>-<forge>`, but part of the 1.7.10
/// era also appends the Minecraft version a second time
/// (`1.7.10-10.13.4.1614-1.7.10`) - and the two forms are not
/// interchangeable: whichever one a given build used, the other 404s.
/// FTB still publishes packs from both eras (SkyFactory 2.5 needs the long
/// form, FTB Resurrection the short one), and the API reports only the bare
/// Forge version, so the coordinate has to be discovered rather than
/// derived.
fn forge_installer_urls(minecraft_version: &str, loader_version: &str) -> Vec<String> {
    let short = format!("{minecraft_version}-{loader_version}");
    let long = format!("{short}-{minecraft_version}");
    [short, long]
        .into_iter()
        .map(|c| format!("{FORGE_MAVEN}/{c}/forge-{c}-installer.jar"))
        .collect()
}

async fn install_via_installer(
    server_dir: &Path,
    urls: &[String],
    installer_name: &str,
    java_path: &str,
) -> Result<InstalledLoader, String> {
    let installer_path: PathBuf = server_dir.join(installer_name);
    download_first_available(urls, &installer_path).await?;

    let result = run_installer_jar(java_path, &installer_path, server_dir).await;

    // Always clean the installer up, success or failure: leaving it behind
    // makes `importer::detect` warn about an installer jar on every later
    // scan of this instance.
    let _ = tokio::fs::remove_file(&installer_path).await;
    result?;

    detect_installed(server_dir).await
}

#[cfg(test)]
mod install_profile_tests {
    use super::*;
    use std::io::Write;

    /// Builds a jar shaped like a real installer: a ZIP with an
    /// `install_profile.json` at its root.
    fn installer_jar(label: &str, profile: &str) -> PathBuf {
        let path = std::env::temp_dir()
            .join(format!("modpackpilot-installer-{label}-{}.jar", uuid::Uuid::new_v4()));
        let file = std::fs::File::create(&path).expect("create jar");
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file(
            "install_profile.json",
            zip::write::SimpleFileOptions::default(),
        )
        .expect("start entry");
        zip.write_all(profile.as_bytes()).expect("write profile");
        zip.finish().expect("finish jar");
        path
    }

    /// The case this exists for: an installer renamed to `installer.jar`,
    /// whose file name no longer carries the version.
    #[test]
    fn reads_the_version_from_a_modern_installer() {
        let jar = installer_jar(
            "modern",
            r#"{"spec":1,"minecraft":"1.20.1","version":"1.20.1-forge-47.4.0"}"#,
        );
        assert_eq!(
            minecraft_version_from_installer(&jar).as_deref(),
            Some("1.20.1")
        );
    }

    /// 1.12-era installers nest it under `install` instead.
    #[test]
    fn reads_the_version_from_a_legacy_installer() {
        let jar = installer_jar(
            "legacy",
            r#"{"install":{"profileName":"forge","minecraft":"1.12.2"},"versionInfo":{}}"#,
        );
        assert_eq!(
            minecraft_version_from_installer(&jar).as_deref(),
            Some("1.12.2")
        );
    }

    /// Anything unreadable is `None`, never a guess - the caller then
    /// refuses the install rather than picking a Java at random.
    #[test]
    fn returns_none_for_anything_it_cannot_read() {
        let jar = installer_jar("empty-profile", r#"{"spec":1}"#);
        assert_eq!(minecraft_version_from_installer(&jar), None);

        let not_a_jar = std::env::temp_dir()
            .join(format!("modpackpilot-not-a-jar-{}.jar", uuid::Uuid::new_v4()));
        std::fs::write(&not_a_jar, b"this is not a zip").expect("write file");
        assert_eq!(minecraft_version_from_installer(&not_a_jar), None);

        assert_eq!(
            minecraft_version_from_installer(Path::new("no-such-file.jar")),
            None
        );
    }
}

/// Live-network checks. Ignored by default - these download real installers
/// and run them, which needs both egress and a JDK.
#[cfg(test)]
mod live_tests {
    use super::*;

    fn temp_server_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("modpackpilot-loader-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// The whole point of this module: running NeoForge's installer must
    /// leave behind something ModpackPilot can actually launch. This is the
    /// step FTB's `serverinstall_*.exe` exists to perform, so if the
    /// argfile isn't produced (or detection doesn't find it), an FTB
    /// install silently yields an unstartable instance.
    #[tokio::test]
    #[ignore]
    async fn installs_a_real_neoforge_server() {
        let server_dir = temp_server_dir("neoforge");

        let installed = install(
            &server_dir,
            ServerLoader::NeoForge,
            "21.1.248",
            Some("1.21.1"),
            "java",
        )
        .await
        .expect("neoforge install");

        assert_eq!(installed.launch_mode, "argfile", "modern NeoForge launches via an argfile");
        assert!(
            server_dir.join(&installed.server_jar).is_file(),
            "{} does not exist",
            installed.server_jar,
        );
        assert!(
            installed.server_jar.contains("neoforge"),
            "unexpected server target: {}",
            installed.server_jar,
        );
        // The installer jar must not be left behind - detection refuses to
        // launch one, and a stray copy makes every later scan warn.
        let leftovers: Vec<_> = std::fs::read_dir(&server_dir)
            .expect("read dir")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_lowercase())
            .filter(|n| n.ends_with("installer.jar"))
            .collect();
        assert!(leftovers.is_empty(), "installer left behind: {leftovers:?}");

        let _ = std::fs::remove_dir_all(&server_dir);
    }

    /// Every direct-download type must list versions, newest first and
    /// release-only, and install one of them to a real jar.
    #[tokio::test]
    #[ignore]
    async fn direct_download_loaders_list_and_install() {
        for loader in [ServerLoader::Vanilla, ServerLoader::Paper, ServerLoader::Purpur] {
            let versions = list_versions(loader).await.expect("list versions");
            assert!(!versions.is_empty(), "{loader:?} listed nothing");
            assert!(versions.iter().all(|v| is_release(v)), "{loader:?}: {versions:?}");
            assert!(
                version_key(&versions[0]) >= version_key(&versions[versions.len() - 1]),
                "{loader:?} not newest-first"
            );

            let server_dir = temp_server_dir("direct");
            let installed = install(&server_dir, loader, "latest", Some("1.21.4"), "java")
                .await
                .expect("install");
            let jar = server_dir.join(&installed.server_jar);
            assert!(jar.metadata().expect("jar exists").len() > 1_000_000, "{loader:?} jar too small");
            let _ = std::fs::remove_dir_all(&server_dir);
        }
    }

    /// A real BuildTools run: slow (minutes) and needs Java 21 plus network,
    /// so it is only run by hand. Proves the jar lands where the instance
    /// expects it and the scratch folder is cleaned up.
    #[tokio::test]
    #[ignore]
    async fn builds_a_real_spigot_server() {
        let instance_dir = temp_server_dir("spigot");
        let server_dir = instance_dir.join("server");

        let steps = std::sync::Mutex::new(Vec::<String>::new());
        let report = |s: &str| steps.lock().unwrap().push(s.to_string());
        let built = build_with_build_tools(&server_dir, ServerLoader::Spigot, "1.21.4", "java", Some(&report))
            .await
            .unwrap_or_else(|e| {
                let log = steps.lock().unwrap();
                let tail = log.iter().rev().take(60).rev().cloned().collect::<Vec<_>>().join("\n");
                panic!("build failed: {e}\n--- last BuildTools output ---\n{tail}");
            });

        assert_eq!(built.server_jar, "spigot-1.21.4.jar");
        assert!(server_dir.join(&built.server_jar).metadata().expect("jar").len() > 1_000_000);
        assert!(!instance_dir.join("buildtools").exists(), "scratch folder left behind");
        assert!(!steps.lock().unwrap().is_empty(), "no progress was reported");

        let _ = std::fs::remove_dir_all(&instance_dir);
    }

    #[tokio::test]
    #[ignore]
    async fn spigot_lists_versions_and_rejects_option_injection() {
        let versions = list_versions(ServerLoader::Spigot).await.expect("versions");
        assert!(versions.iter().any(|v| v == "1.21.4"), "{versions:?}");
        assert!(versions.iter().all(|v| is_release(v)));

        let dir = temp_server_dir("inject");
        let err = build_with_build_tools(&dir, ServerLoader::Spigot, "--help", "java", None)
            .await
            .expect_err("must refuse");
        assert!(err.contains("not a valid Minecraft release version"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn version_strings_cannot_escape_a_url_or_folder() {
        for ok in ["1.21.4", "24w05a", "47.4.0", "1.7.10-10.13.4.1614", "latest"] {
            assert!(is_safe_version_string(ok), "{ok} should be accepted");
        }
        for bad in [
            "", "..", "../x", "..\\..\\evil", "1.21/../../x", "1.21?x=1", "1.21#", "a b",
            "--help", "-rf", ".hidden", "1..2", "x%2f", &"9".repeat(65),
        ] {
            assert!(!is_safe_version_string(bad), "{bad:?} should be rejected");
        }
    }

    /// The traversal this guards: a stored Minecraft version of
    /// `..\..\evil` would have made Paper write `paper-..\..\evil-N.jar`
    /// outside the instance's server folder.
    #[tokio::test]
    async fn install_refuses_a_traversal_version_before_touching_disk() {
        let dir = std::env::temp_dir().join(format!("modpackpilot-trav-{}", uuid::Uuid::new_v4()));
        for loader in [ServerLoader::Paper, ServerLoader::Purpur, ServerLoader::Vanilla] {
            let err = install(&dir, loader, "latest", Some("..\\..\\evil"), "java")
                .await
                .expect_err("must refuse");
            assert!(err.contains("not a valid Minecraft version"), "{err}");
        }
        let err = install(&dir, ServerLoader::Paper, "../1", Some("1.21.4"), "java")
            .await
            .expect_err("must refuse");
        assert!(err.contains("not a valid loader version"), "{err}");
        assert!(!dir.exists(), "nothing should have been created");
    }

    #[test]
    fn newest_first_orders_numerically_and_drops_prereleases() {
        let sorted = newest_first(
            ["1.9", "1.21.4", "26.1", "1.21.11-rc1", "1.21.11", "1.21.4"]
                .map(String::from)
                .to_vec(),
        );
        assert_eq!(sorted, ["26.1", "1.21.11", "1.21.4", "1.9"]);
    }

    /// Fabric needs no installer run at all - its meta service hands back a
    /// ready-made launch jar.
    #[tokio::test]
    #[ignore]
    async fn installs_a_real_fabric_server() {
        let server_dir = temp_server_dir("fabric");

        let installed = install(
            &server_dir,
            ServerLoader::Fabric,
            "0.16.9",
            Some("1.20.1"),
            "java",
        )
        .await
        .expect("fabric install");

        assert_eq!(installed.launch_mode, "jar");
        assert!(server_dir.join(&installed.server_jar).is_file());

        let _ = std::fs::remove_dir_all(&server_dir);
    }

    /// Both Forge coordinate shapes must resolve, or installing a pack from
    /// the era that uses the other one fails *after* its whole download.
    #[tokio::test]
    #[ignore]
    async fn forge_coordinates_resolve_for_both_naming_eras() {
        // (minecraft, forge) pairs taken from real FTB packs: SkyFactory 2.5
        // uses the long form, FTB Resurrection and Direwolf20 1.12 the short.
        let packs = [
            ("1.7.10", "10.13.4.1614"),
            ("1.7.10", "10.13.2.1291"),
            ("1.12.2", "14.23.5.2860"),
            ("1.20.1", "47.2.20"),
        ];
        for (mc, forge) in packs {
            let urls = forge_installer_urls(mc, forge);
            let mut found = false;
            for url in &urls {
                if client().head(url).send().await.expect("request").status().is_success() {
                    found = true;
                    break;
                }
            }
            assert!(found, "no coordinate resolved for Forge {forge} on {mc}: {urls:?}");
        }
    }

    /// A version the pack claims but that was never published should say so
    /// plainly, not fail somewhere deep in the installer.
    #[tokio::test]
    #[ignore]
    async fn a_nonexistent_loader_version_is_reported_clearly() {
        let server_dir = temp_server_dir("missing");
        let err = install(
            &server_dir,
            ServerLoader::NeoForge,
            "0.0.0-does-not-exist",
            Some("1.21.1"),
            "java",
        )
        .await
        .expect_err("should not resolve");
        assert!(err.contains("doesn't exist"), "unexpected error: {err}");

        let _ = std::fs::remove_dir_all(&server_dir);
    }
}
