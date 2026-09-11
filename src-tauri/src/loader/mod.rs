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

use crate::importer;
use crate::models::ServerLoader;

const FORGE_MAVEN: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge";
const NEOFORGE_MAVEN: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge";
const FABRIC_META: &str = "https://meta.fabricmc.net/v2/versions";

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
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("ModpackPilot/0.1.0 (+https://github.com/RimeValkyris/ModPilot)")
        .build()
        .expect("static reqwest client config is always valid")
}

/// Whether this loader can be installed automatically at all. Used to fail
/// an FTB import early, before anything is downloaded, rather than after.
pub fn is_supported(loader: ServerLoader) -> bool {
    matches!(
        loader,
        ServerLoader::Forge | ServerLoader::NeoForge | ServerLoader::Fabric
    )
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

    let output = command
        .output()
        .await
        .map_err(|e| format!("Failed to run the Forge/NeoForge installer: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let combined = format!("{stdout}{stderr}");
        let tail = if stderr.trim().is_empty() { &stdout } else { &stderr };

        // The installer's own message for a TLS failure says nothing about
        // TLS, so translate it rather than passing the confusion along.
        if combined.contains("PKIX") || combined.contains("unable to find valid certification path")
        {
            return Err(
                "The loader installer couldn't verify the HTTPS connections it needs to download                  Minecraft's libraries. This is usually antivirus TLS inspection or a corporate                  proxy intercepting the connection with its own certificate. Allow-list                  maven.neoforged.net and maven.minecraftforge.net in that software, or install                  the loader manually, then try again."
                    .to_string(),
            );
        }

        return Err(format!(
            "Forge/NeoForge installer failed (exit code {:?}): {}",
            output.status.code(),
            tail.lines().rev().take(5).collect::<Vec<_>>().join(" / "),
        ));
    }

    Ok(())
}

/// Re-reads a server directory after an install and reports what should be
/// launched. Separate from the install itself because the answer is
/// whatever ended up on disk, not whatever we intended to put there.
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
    match loader {
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
            })
        }
        other => Err(unsupported_message(other)),
    }
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
