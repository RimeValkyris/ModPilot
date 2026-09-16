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
/// A shared wrapping folder is stripped (see `detect_wrapper_folder`), so
/// the extracted layout matches what `detect_from_zip` already showed the
/// user during review. That is right for an *import*; a restore must not
/// reshape what it restores, so it uses [`extract_zip_verbatim`] instead.
///
/// This only ever writes bytes to disk - extracted `.bat`/`.sh` files are
/// never executed here or anywhere else in the import flow.
pub fn extract_zip_safely(zip_path: &Path, dest_root: &Path) -> std::io::Result<Vec<String>> {
    extract_zip_inner(zip_path, dest_root, true)
}

/// Extracts a ZIP archive into `dest_root` exactly as it was stored, with
/// the same zip-slip protection as [`extract_zip_safely`] but no wrapper
/// folder stripping.
///
/// Used for world-backup restores. A world whose every file happens to sit
/// under one top-level folder (a `region/`-only save, say) would otherwise
/// have that folder silently dissolved, turning a restore into a subtly
/// different world than the one that was backed up.
pub fn extract_zip_verbatim(zip_path: &Path, dest_root: &Path) -> std::io::Result<Vec<String>> {
    extract_zip_inner(zip_path, dest_root, false)
}

fn extract_zip_inner(
    zip_path: &Path,
    dest_root: &Path,
    strip_wrapper: bool,
) -> std::io::Result<Vec<String>> {
    let file = File::open(zip_path)?;
    let mut archive =
        ZipArchive::new(file).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    // Detected up front from the file entries alone, so it can be stripped
    // as everything is written rather than in a second pass.
    let wrapper_prefix = if strip_wrapper {
        let file_names: Vec<String> = (0..archive.len())
            .filter_map(|i| {
                let entry = archive.by_index(i).ok()?;
                (!entry.is_dir()).then(|| entry.name().replace('\\', "/"))
            })
            .collect();
        detect_wrapper_folder(&file_names).map(|w| format!("{w}/"))
    } else {
        None
    };

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

/// What [`verify_zip`] found in an archive it read all the way through.
#[derive(Debug, Clone, Copy, Default)]
pub struct ZipContents {
    pub file_count: u64,
    pub uncompressed_bytes: u64,
}

/// Reads every byte of every entry in an archive to prove it can actually
/// be extracted, without writing anything.
///
/// Draining each entry to a sink is what makes this a real check rather
/// than a directory listing: the `zip` crate validates an entry's CRC32
/// when its reader hits EOF, so a truncated or bit-rotted archive fails
/// here instead of halfway through overwriting somebody's world.
///
/// The cost is reading (not writing) the archive once, which is why this is
/// run on demand and immediately after a backup is written, rather than for
/// every row of a backup listing.
pub fn verify_zip(zip_path: &Path) -> std::io::Result<ZipContents> {
    let file = File::open(zip_path)?;
    let mut archive =
        ZipArchive::new(file).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let mut contents = ZipContents::default();

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        if entry.is_dir() {
            continue;
        }
        // An entry that escapes its destination would be skipped on
        // extraction, so an archive full of them would "verify" into
        // nothing. Fail the whole archive instead.
        if safe_join(Path::new(""), &entry.name().replace('\\', "/")).is_none() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Archive contains an unsafe entry path: {}", entry.name()),
            ));
        }
        let copied = std::io::copy(&mut entry, &mut std::io::sink())?;
        contents.file_count += 1;
        contents.uncompressed_bytes += copied;
    }

    Ok(contents)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway directory under the OS temp dir, removed on drop so a
    /// failing test can't leave a tree behind.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("mpp-test-{tag}-{unique}"));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write(path: &Path, contents: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    /// The round trip a world backup actually performs.
    #[test]
    fn zips_verifies_and_extracts_a_tree() {
        let dir = TempDir::new("roundtrip");
        let world = dir.path().join("world");
        write(&world.join("level.dat"), b"root file");
        write(&world.join("region/r.0.0.mca"), b"chunk data");
        write(&world.join("playerdata/steve.dat"), b"player");

        let zip_path = dir.path().join("backup.zip");
        create_zip_from_dir(&world, &zip_path).unwrap();

        let contents = verify_zip(&zip_path).unwrap();
        assert_eq!(contents.file_count, 3);
        assert_eq!(contents.uncompressed_bytes, 9 + 10 + 6);

        let restored = dir.path().join("restored");
        extract_zip_verbatim(&zip_path, &restored).unwrap();
        assert_eq!(std::fs::read(restored.join("level.dat")).unwrap(), b"root file");
        assert_eq!(
            std::fs::read(restored.join("region/r.0.0.mca")).unwrap(),
            b"chunk data"
        );
    }

    /// A truncated archive must fail verification, which is what keeps a
    /// restore from starting at all - see `commands::backup::restore_world_backup`.
    #[test]
    fn verification_rejects_a_truncated_archive() {
        let dir = TempDir::new("truncated");
        let world = dir.path().join("world");
        write(&world.join("level.dat"), &vec![7u8; 4096]);

        let zip_path = dir.path().join("backup.zip");
        create_zip_from_dir(&world, &zip_path).unwrap();
        assert!(verify_zip(&zip_path).is_ok());

        // Lop off the tail, as a half-written or bit-rotted file would be.
        let bytes = std::fs::read(&zip_path).unwrap();
        std::fs::write(&zip_path, &bytes[..bytes.len() / 2]).unwrap();

        assert!(verify_zip(&zip_path).is_err(), "truncated archive verified");
    }

    /// The whole reason restore doesn't share the import extractor: a world
    /// whose files all sit under one folder must keep that folder.
    #[test]
    fn verbatim_extraction_keeps_a_single_top_level_folder() {
        let dir = TempDir::new("wrapper");
        let src = dir.path().join("src");
        write(&src.join("region/r.0.0.mca"), b"only region");
        write(&src.join("region/r.0.1.mca"), b"more region");

        let zip_path = dir.path().join("w.zip");
        create_zip_from_dir(&src, &zip_path).unwrap();

        // The import path treats the shared folder as a wrapper and strips it.
        let imported = dir.path().join("imported");
        extract_zip_safely(&zip_path, &imported).unwrap();
        assert!(imported.join("r.0.0.mca").is_file());

        // The restore path must not.
        let restored = dir.path().join("restored");
        extract_zip_verbatim(&zip_path, &restored).unwrap();
        assert!(restored.join("region/r.0.0.mca").is_file());
    }

    #[test]
    fn rejects_paths_that_escape_the_destination() {
        let root = Path::new("/instances/demo");
        assert!(safe_join(root, "../../etc/passwd").is_none());
        assert!(safe_join(root, "region/../../../x").is_none());
        assert!(safe_join(root, "C:/windows/system32").is_none());
        assert_eq!(
            safe_join(root, "region/r.0.0.mca"),
            Some(root.join("region").join("r.0.0.mca"))
        );
    }
}
