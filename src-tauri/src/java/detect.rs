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
        let mut roots = vec![
            PathBuf::from(r"C:\Program Files\Java"),
            PathBuf::from(r"C:\Program Files\Eclipse Adoptium"),
            PathBuf::from(r"C:\Program Files\Zulu"),
            PathBuf::from(r"C:\Program Files\Microsoft"),
            PathBuf::from(r"C:\Program Files\Amazon Corretto"),
            PathBuf::from(r"C:\Program Files\BellSoft"),
            PathBuf::from(r"C:\Program Files\Semeru"),
            PathBuf::from(r"C:\Program Files (x86)\Java"),
        ];
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            roots.push(PathBuf::from(local_app_data).join(r"Programs\Eclipse Adoptium"));
        }
        if let Ok(user_profile) = std::env::var("USERPROFILE") {
            roots.push(PathBuf::from(user_profile).join(".jdks"));
        }
        roots
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
        let java_home = java_home.trim();
        if !java_home.is_empty() {
            let java_home = PathBuf::from(java_home);
            candidates.push(if java_home.is_file() {
                java_home
            } else {
                java_home.join("bin").join(JAVA_EXE)
            });
        }
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

    for java_home in registry_java_homes() {
        candidates.push(java_home.join("bin").join(JAVA_EXE));
    }

    candidates
}

/// Windows installers commonly register Java homes even when they do not
/// update PATH. Query the registry through the built-in `reg.exe` command so
/// detection does not need a platform-specific crate or elevated access.
fn registry_java_homes() -> Vec<PathBuf> {
    if !cfg!(windows) {
        return Vec::new();
    }

    let keys = [
        r"HKLM\SOFTWARE\JavaSoft",
        r"HKLM\SOFTWARE\WOW6432Node\JavaSoft",
        r"HKCU\SOFTWARE\JavaSoft",
    ];
    let mut homes = Vec::new();
    for key in keys {
        let Ok(output) = Command::new("reg.exe")
            .args(["query", key, "/s"])
            .output()
        else {
            continue;
        };
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let Some((name, value)) = line.split_once("REG_SZ") else {
                continue;
            };
            if name.trim() == "JavaHome" {
                let value = value.trim();
                if !value.is_empty() {
                    homes.push(PathBuf::from(value));
                }
            }
        }
    }
    homes
}

/// Runs Java's diagnostic properties and parses its output. The banner format
/// varies between vendors; these properties are more stable and also expose
/// vendor and architecture directly.
fn probe(java_path: &Path) -> Option<DetectedJava> {
    let output = Command::new(java_path)
        .args(["-XshowSettings:properties", "-version"])
        .output()
        .ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    if text.is_empty() {
        return None;
    }

    let version = parse_property(&text, "java.version")
        .map(|value| normalize_version(&value))
        .or_else(|| parse_version(&text))?;
    let architecture = parse_property(&text, "os.arch")
        .map(|value| normalize_architecture(&value))
        .unwrap_or_else(|| parse_architecture(&text));
    let vendor = parse_property(&text, "java.vendor").or_else(|| parse_vendor(&text));

    Some(DetectedJava {
        version,
        vendor,
        path: java_path.to_string_lossy().to_string(),
        architecture,
    })
}

fn parse_property(text: &str, property: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == property).then(|| value.trim().to_string())
    })
}

fn normalize_architecture(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    if lower.contains("aarch64") || lower.contains("arm64") {
        "arm64".to_string()
    } else if lower.contains("64") || lower.contains("amd64") || lower.contains("x86_64") {
        "x64".to_string()
    } else if lower.contains("86") || lower.contains("x32") {
        "x86".to_string()
    } else {
        "unknown".to_string()
    }
}

/// Finds Java executables bundled inside an imported server pack. Pack
/// launchers commonly place these under `runtime`, `jre`, or `jdk` folders,
/// several levels below the server root.
pub fn detect_java_installations_under(root: &Path) -> Vec<DetectedJava> {
    let mut seen = HashSet::new();
    let mut results = Vec::new();

    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let is_java = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case(JAVA_EXE));
        let looks_bundled = path.components().any(|component| {
            matches!(
                component.as_os_str().to_str().map(str::to_ascii_lowercase).as_deref(),
                Some("runtime") | Some("jre") | Some("jdk")
            )
        });
        if !is_java || !looks_bundled {
            continue;
        }
        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if !seen.insert(canonical.clone()) {
            continue;
        }
        if let Some(detected) = probe(&canonical) {
            results.push(detected);
        }
    }

    results
}

/// Extracts the quoted version string, e.g. `"21.0.2"` or `"1.8.0_392"`,
/// and normalizes it to a plain major-version-led string (`"21.0.2"`,
/// `"8.0.392"`) so both old and new numbering schemes display consistently.
fn parse_version(text: &str) -> Option<String> {
    let re = Regex::new(r#"version "([^"]+)""#).unwrap();
    let raw = re.captures(text)?.get(1)?.as_str().to_string();

    Some(normalize_version(&raw))
}

fn normalize_version(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix("1.") {
        // Legacy scheme: "1.8.0_392" -> "8.0.392"
        rest.replacen('_', ".", 1)
    } else {
        raw.to_string()
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
///
/// The folder name isn't always literally "javapath" - Oracle versions it
/// per major Java release too (observed in the wild: `java8path_target_*`
/// for JRE 8, alongside plain `javapath_target_*` for newer JDKs), so this
/// matches `java<digits?>path(_target...)?` rather than one exact string.
pub(crate) fn is_oracle_path_redirector(path: &Path) -> bool {
    path.components().any(|c| {
        c.as_os_str()
            .to_str()
            .is_some_and(|s| {
                let lower = s.to_lowercase();
                let Some(rest) = lower.strip_prefix("java") else {
                    return false;
                };
                let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
                rest == "path" || rest.starts_with("path_target")
            })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_java_diagnostic_properties() {
        let text = r#"
    java.vendor = Eclipse Adoptium
    java.version = 21.0.8
    os.arch = amd64
openjdk version "21.0.8" 2025-07-15
"#;

        assert_eq!(parse_property(text, "java.version").as_deref(), Some("21.0.8"));
        assert_eq!(parse_property(text, "java.vendor").as_deref(), Some("Eclipse Adoptium"));
        assert_eq!(normalize_architecture("amd64"), "x64");
    }

    #[test]
    fn normalizes_legacy_java_version() {
        assert_eq!(parse_version("java version \"1.8.0_392\""), Some("8.0.392".to_string()));
    }
}
