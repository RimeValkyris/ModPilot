use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;

use crate::models::DetectedJava;

const JAVA_EXE: &str = if cfg!(windows) { "java.exe" } else { "java" };

/// Directories that commonly contain one JDK install per subdirectory,
/// e.g. `C:\Program Files\Java\jdk-21\bin\java.exe`. Best-effort: covers
/// the common installers (Oracle, Temurin/Adoptium, Zulu, Microsoft build
/// of OpenJDK) without needing registry access or an auto-installer.
fn common_install_roots() -> Vec<PathBuf> {
    if cfg!(windows) {
        vec![
            PathBuf::from(r"C:\Program Files\Java"),
            PathBuf::from(r"C:\Program Files\Eclipse Adoptium"),
            PathBuf::from(r"C:\Program Files\Zulu"),
            PathBuf::from(r"C:\Program Files\Microsoft"),
            PathBuf::from(r"C:\Program Files (x86)\Java"),
        ]
    } else {
        vec![
            PathBuf::from("/usr/lib/jvm"),
            PathBuf::from("/opt"),
            PathBuf::from(format!(
                "{}/.sdkman/candidates/java",
                std::env::var("HOME").unwrap_or_default()
            )),
        ]
    }
}

/// Finds every `java`/`java.exe` executable worth probing: `JAVA_HOME`,
/// every directory on `PATH`, and one level into each common install root.
/// Callers are responsible for deduping and validating each candidate.
fn find_candidate_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        candidates.push(Path::new(&java_home).join("bin").join(JAVA_EXE));
    }

    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            candidates.push(dir.join(JAVA_EXE));
        }
    }

    for root in common_install_roots() {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                candidates.push(entry.path().join("bin").join(JAVA_EXE));
            }
        }
    }

    candidates
}

/// Runs `java -version` and parses its output. Every JDK prints this to
/// stderr, not stdout, in a near-universal three-line format across
/// vendors - see the format examples in the parsing helpers below.
fn probe(java_path: &Path) -> Option<DetectedJava> {
    let output = Command::new(java_path).arg("-version").output().ok()?;
    // `-version` exits 0 on every JDK we've seen; still fall back to
    // checking stdout in case some vendor's build differs.
    let text = if !output.stderr.is_empty() {
        String::from_utf8_lossy(&output.stderr).to_string()
    } else {
        String::from_utf8_lossy(&output.stdout).to_string()
    };
    if text.is_empty() {
        return None;
    }

    let version = parse_version(&text)?;
    let architecture = parse_architecture(&text);
    let vendor = parse_vendor(&text);

    Some(DetectedJava {
        version,
        vendor,
        path: java_path.to_string_lossy().to_string(),
        architecture,
    })
}

/// Extracts the quoted version string, e.g. `"21.0.2"` or `"1.8.0_392"`,
/// and normalizes it to a plain major-version-led string (`"21.0.2"`,
/// `"8.0.392"`) so both old and new numbering schemes display consistently.
fn parse_version(text: &str) -> Option<String> {
    let re = Regex::new(r#"version "([^"]+)""#).unwrap();
    let raw = re.captures(text)?.get(1)?.as_str().to_string();

    if let Some(rest) = raw.strip_prefix("1.") {
        // Legacy scheme: "1.8.0_392" -> "8.0.392"
        Some(rest.replacen('_', ".", 1))
    } else {
        Some(raw)
    }
}

fn parse_architecture(text: &str) -> String {
    if text.contains("64-Bit") || text.contains("amd64") || text.contains("x86_64") {
        "x64".to_string()
    } else if text.contains("32-Bit") {
        "x86".to_string()
    } else if text.contains("aarch64") || text.contains("arm64") {
        "arm64".to_string()
    } else {
        "unknown".to_string()
    }
}

fn parse_vendor(text: &str) -> Option<String> {
    const KNOWN_VENDORS: &[&str] = &[
        "Temurin",
        "Eclipse Adoptium",
        "GraalVM",
        "Zulu",
        "Corretto",
        "Microsoft",
        "OpenJ9",
        "Oracle",
        "OpenJDK",
    ];
    KNOWN_VENDORS
        .iter()
        .find(|v| text.contains(*v))
        .map(|v| v.to_string())
}

/// Oracle's installer keeps `PATH` pointing at "whichever JDK is currently
/// active" by adding a `javapath` folder to `PATH` containing hard-linked
/// copies of `java.exe`/`javaw.exe`, physically placed in a per-install
/// `javapath_target_<id>` subfolder. Hard links have no single "original"
/// path for `canonicalize()` to resolve them back to, so without this
/// filter every Oracle JDK shows up twice: once through this redirector,
/// once through its real install folder under `Program Files\Java`.
pub(crate) fn is_oracle_path_redirector(path: &Path) -> bool {
    path.components().any(|c| {
        c.as_os_str()
            .to_str()
            .map(|s| {
                let lower = s.to_lowercase();
                lower == "javapath" || lower.starts_with("javapath_target")
            })
            .unwrap_or(false)
    })
}

/// Scans the system for Java installations, deduplicated by resolved path.
/// Runs `java -version` once per unique candidate found, so this does
/// blocking I/O and should be called from `spawn_blocking`.
pub fn detect_java_installations() -> Vec<DetectedJava> {
    let mut seen = HashSet::new();
    let mut results = Vec::new();

    for candidate in find_candidate_paths() {
        if is_oracle_path_redirector(&candidate) {
            continue;
        }
        if !candidate.is_file() {
            continue;
        }
        let canonical = std::fs::canonicalize(&candidate).unwrap_or(candidate);
        if !seen.insert(canonical.clone()) {
            continue;
        }
        if let Some(detected) = probe(&canonical) {
            results.push(detected);
        }
    }

    results
}
