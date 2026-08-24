use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

use super::detect::detect_wrapper_folder;

/// Resolves a ZIP entry's internal path to a safe path under `dest_root`,
/// or `None` if the entry tries to escape it - the "zip slip" attack, where
/// a crafted `../../etc/whatever` entry name writes outside the intended
/// directory. Imported archives are untrusted input, so every entry is
/// checked before anything is written.
fn safe_join(dest_root: &Path, entry_name: &str) -> Option<PathBuf> {
    let normalized = entry_name.replace('\\', "/");
    let mut out = dest_root.to_path_buf();

    for part in normalized.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        // Reject a smuggled drive letter or UNC-looking segment (e.g. "C:").
        if part.contains(':') {
            return None;
        }
        out.push(part);
    }

    Some(out)
}

/// Extracts a ZIP archive into `dest_root`, skipping (and reporting) any
/// entry that would escape the destination directory.
///
/// This only ever writes bytes to disk - extracted `.bat`/`.sh` files are
/// never executed here or anywhere else in the import flow.
pub fn extract_zip_safely(zip_path: &Path, dest_root: &Path) -> std::io::Result<Vec<String>> {
    let file = File::open(zip_path)?;
    let mut archive =
        ZipArchive::new(file).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    // Detect a shared wrapping folder (see `detect_wrapper_folder`) up front
    // from the file entries alone, so it can be stripped as everything is
    // written - the extracted layout needs to match what `detect_from_zip`
    // already showed the user during review.
    let file_names: Vec<String> = (0..archive.len())
        .filter_map(|i| {
            let entry = archive.by_index(i).ok()?;
            (!entry.is_dir()).then(|| entry.name().replace('\\', "/"))
        })
        .collect();
    let wrapper_prefix = detect_wrapper_folder(&file_names).map(|w| format!("{w}/"));

    let mut warnings = Vec::new();

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let raw_name = entry.name().to_string();
        let normalized = raw_name.replace('\\', "/");
        let name = match &wrapper_prefix {
            Some(prefix) => normalized.strip_prefix(prefix.as_str()).unwrap_or(&normalized),
            None => &normalized,
        };
        if name.is_empty() {
            continue; // the wrapper folder's own directory entry
        }

        let Some(out_path) = safe_join(dest_root, name) else {
            warnings.push(format!("Skipped unsafe archive entry: {raw_name}"));
            continue;
        };

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out_file = File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out_file)?;
    }

    Ok(warnings)
}

/// Zips a directory tree into `dest_zip` - used for world backups. Not
/// import-specific, but lives alongside `extract_zip_safely` since the two
/// are natural inverses of each other.
pub fn create_zip_from_dir(src_dir: &Path, dest_zip: &Path) -> std::io::Result<()> {
    let file = File::create(dest_zip)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    for entry in walkdir::WalkDir::new(src_dir) {
        let entry = entry.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let rel_path = entry
            .path()
            .strip_prefix(src_dir)
            .expect("walkdir entries are always under src_dir");
        if rel_path.as_os_str().is_empty() {
            continue;
        }
        let name = rel_path.to_string_lossy().replace('\\', "/");

        if entry.file_type().is_dir() {
            zip.add_directory(format!("{name}/"), options)?;
        } else if entry.file_type().is_file() {
            zip.start_file(name, options)?;
            let bytes = std::fs::read(entry.path())?;
            zip.write_all(&bytes)?;
        }
    }

    zip.finish()?;
    Ok(())
}

/// Recursively copies a directory tree into `dest_root`. Used for "import
/// from folder" - the source folder is only ever read, never modified.
///
/// Symlinks are skipped deliberately: following one could copy files from
/// outside the folder the user actually picked.
pub fn copy_dir_recursive(src_root: &Path, dest_root: &Path) -> std::io::Result<()> {
    // Same wrapping-folder check as the ZIP path (see `detect_wrapper_folder`)
    // - e.g. the user picked a folder that's itself just an extracted
    // archive still wrapped in a directory matching the pack's name.
    let file_names: Vec<String> = walkdir::WalkDir::new(src_root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            e.path()
                .strip_prefix(src_root)
                .ok()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
        })
        .collect();
    let wrapper_prefix = detect_wrapper_folder(&file_names).map(|w| format!("{w}/"));

    for entry in walkdir::WalkDir::new(src_root) {
        let entry = entry.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let rel = entry
            .path()
            .strip_prefix(src_root)
            .expect("walkdir entries are always under src_root")
            .to_string_lossy()
            .replace('\\', "/");
        let rel = match &wrapper_prefix {
            Some(prefix) => rel.strip_prefix(prefix.as_str()).unwrap_or(&rel).to_string(),
            None => rel,
        };
        if rel.is_empty() {
            continue; // src_root itself, or the wrapper folder's own entry
        }

        let mut out_path = dest_root.to_path_buf();
        out_path.extend(rel.split('/'));

        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &out_path)?;
        }
    }
    Ok(())
}
