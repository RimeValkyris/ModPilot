use std::fs::File;
use std::path::{Path, PathBuf};

use zip::ZipArchive;

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

    let mut warnings = Vec::new();

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let name = entry.name().to_string();

        let Some(out_path) = safe_join(dest_root, &name) else {
            warnings.push(format!("Skipped unsafe archive entry: {name}"));
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

/// Recursively copies a directory tree into `dest_root`. Used for "import
/// from folder" - the source folder is only ever read, never modified.
///
/// Symlinks are skipped deliberately: following one could copy files from
/// outside the folder the user actually picked.
pub fn copy_dir_recursive(src_root: &Path, dest_root: &Path) -> std::io::Result<()> {
    for entry in walkdir::WalkDir::new(src_root) {
        let entry = entry.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let rel = entry
            .path()
            .strip_prefix(src_root)
            .expect("walkdir entries are always under src_root");
        let out_path = dest_root.join(rel);

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
