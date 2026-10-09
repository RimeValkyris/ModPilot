use std::io::Cursor;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

const ADOPTIUM_ASSETS_API: &str = "https://api.adoptium.net/v3/assets/latest";

#[derive(Debug, Deserialize)]
struct AdoptiumAsset {
    binary: AdoptiumBinary,
}

#[derive(Debug, Deserialize)]
struct AdoptiumBinary {
    package: AdoptiumPackage,
}

#[derive(Debug, Deserialize)]
struct AdoptiumPackage {
    checksum: String,
    link: String,
}

pub async fn install_temurin(
    destination_root: &Path,
    major: u32,
) -> Result<PathBuf, String> {
    if !matches!(major, 8 | 17 | 21 | 25) {
        return Err("Java installer supports Java 8, 17, 21, and 25".to_string());
    }
    if !cfg!(windows) {
        return Err("Managed Java installation is currently supported on Windows only".to_string());
    }

    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "aarch64",
        _ => return Err("This Windows architecture is not supported by the Java installer".to_string()),
    };
    let client = reqwest::Client::new();
    let metadata_url = format!(
        "{ADOPTIUM_ASSETS_API}/{major}/hotspot?architecture={arch}&image_type=jdk&jvm_impl=hotspot&os=windows&vendor=eclipse&release_type=ga"
    );
    let asset = client
        .get(metadata_url)
        .send()
        .await
        .map_err(|e| format!("Failed to query Temurin Java {major} releases: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Temurin Java {major} release lookup failed: {e}"))?
        .json::<Vec<AdoptiumAsset>>()
        .await
        .map_err(|e| format!("Failed to parse Temurin Java {major} release metadata: {e}"))?
        .into_iter()
        .next()
        .ok_or_else(|| format!("No Temurin Java {major} release is available for Windows {arch}"))?;
    let bytes = client
        .get(&asset.binary.package.link)
        .send()
        .await
        .map_err(|e| format!("Failed to download Temurin Java {major}: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Temurin Java {major} download failed: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("Failed to read Temurin Java {major} download: {e}"))?;

    if bytes.len() < 1024 {
        return Err("Temurin download was unexpectedly small".to_string());
    }
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if !digest.eq_ignore_ascii_case(&asset.binary.package.checksum) {
        return Err(format!(
            "Temurin Java {major} checksum mismatch (expected {}, got {digest})",
            asset.binary.package.checksum
        ));
    }
    tracing::info!(java_major = major, sha256 = %digest, "Verified Temurin archive");

    let install_dir = destination_root.join(format!("temurin-{major}"));
    let install_dir_for_task = install_dir.clone();
    tokio::task::spawn_blocking(move || extract_archive(bytes, &install_dir_for_task))
        .await
        .map_err(|e| format!("Java extraction task failed: {e}"))??;

    let java = find_java_executable(&install_dir)
        .ok_or_else(|| "Temurin archive did not contain a Java executable".to_string())?;
    Ok(java)
}

fn extract_archive(bytes: impl AsRef<[u8]>, destination: &Path) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| format!("Invalid Temurin ZIP archive: {e}"))?;
    let staging = destination.with_extension("installing");
    if staging.exists() {
        std::fs::remove_dir_all(&staging)
            .map_err(|e| format!("Failed to clear incomplete Java installation: {e}"))?;
    }
    std::fs::create_dir_all(&staging)
        .map_err(|e| format!("Failed to create Java installation directory: {e}"))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| format!("Failed to read Java archive entry: {e}"))?;
        let name = entry
            .enclosed_name()
            .ok_or_else(|| "Temurin archive contains an unsafe path".to_string())?
            .to_path_buf();
        let target = staging.join(name);
        if entry.is_dir() {
            std::fs::create_dir_all(&target)
                .map_err(|e| format!("Failed to create Java directory: {e}"))?;
        } else {
            if target.parent().is_none() {
                return Err("Temurin archive contains an invalid file path".to_string());
            }
            std::fs::create_dir_all(target.parent().unwrap())
                .map_err(|e| format!("Failed to create Java archive directory: {e}"))?;
            let mut output = std::fs::File::create(&target)
                .map_err(|e| format!("Failed to create Java archive file: {e}"))?;
            std::io::copy(&mut entry, &mut output)
                .map_err(|e| format!("Failed to extract Java archive: {e}"))?;
        }
    }

    let root = std::fs::read_dir(&staging)
        .map_err(|e| format!("Failed to inspect Java installation: {e}"))?
        .filter_map(Result::ok)
        .find(|entry| entry.path().join("bin").join("java.exe").is_file())
        .map(|entry| entry.path())
        .ok_or_else(|| "Extracted Java installation has no bin\\java.exe".to_string())?;

    // Move the old install aside instead of deleting it, so a failure (or a
    // JDK still in use) leaves the previous one intact.
    let backup = destination.with_extension("old");
    let had_previous = destination.exists();
    if had_previous {
        if backup.exists() {
            std::fs::remove_dir_all(&backup)
                .map_err(|e| format!("Failed to clear an old Java backup: {e}"))?;
        }
        std::fs::rename(destination, &backup).map_err(|e| {
            format!("Failed to replace existing Java installation (is it in use?): {e}")
        })?;
    }
    if let Err(e) = std::fs::rename(&root, destination) {
        if had_previous {
            let _ = std::fs::rename(&backup, destination);
        }
        return Err(format!("Failed to finalize Java installation: {e}"));
    }
    if had_previous {
        let _ = std::fs::remove_dir_all(&backup);
    }
    std::fs::remove_dir_all(&staging)
        .map_err(|e| format!("Failed to clean Java installation staging directory: {e}"))?;
    Ok(())
}

fn find_java_executable(root: &Path) -> Option<PathBuf> {
    if root.join("bin").join("java.exe").is_file() {
        return Some(root.join("bin").join("java.exe"));
    }
    std::fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find_map(|path| {
            let java = path.join("bin").join("java.exe");
            java.is_file().then_some(java)
        })
}
