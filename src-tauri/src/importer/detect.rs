use std::cell::RefCell;
use std::path::Path;

use regex::Regex;

use super::script::parse_start_script;
use crate::models::{DetectedServerInfo, ServerLoader};

/// Reads one of the analyzed files by its (wrapper-stripped, forward-slash)
/// path, as text. `None` for anything missing or not valid UTF-8.
///
/// Detection is mostly a path-listing exercise, but a pack that ships only
/// a start script keeps the answer *inside* a file - so `analyze` gets a
/// way to open the handful of files it actually needs to read, without
/// caring whether they live in a directory or still inside a ZIP.
type ReadFile<'a> = dyn Fn(&str) -> Option<String> + 'a;

/// How large a file this is willing to read as a start script. Real ones
/// are a few hundred bytes; anything vastly larger is not a script, and
/// imported archives are untrusted input.
const MAX_SCRIPT_BYTES: usize = 256 * 1024;

const START_SCRIPT_NAMES: &[&str] = &[
    "start.sh",
    "start.bat",
    "run.sh",
    "run.bat",
    "startserver.sh",
    "startserver.bat",
    "launch.sh",
    "launch.bat",
];

/// If every file in the list lives under one shared top-level folder - and
/// none sit directly at the root - returns that folder's name. Many
/// modpack export tools zip (or fold into an extracted folder) a server
/// pack wrapped in a directory matching the pack's name, e.g.
/// `MyServerPack-1.0/mods/...`, `MyServerPack-1.0/server.properties`. Without
/// unwrapping that, every fixed path the rest of the app relies on
/// (`server/mods`, `server/server.properties`, the server JAR itself) ends
/// up one directory too deep for anything to find - the server still gets
/// imported, but shows up looking empty (no mods, no detected JAR).
pub(crate) fn detect_wrapper_folder(file_paths: &[String]) -> Option<String> {
    if file_paths.is_empty() {
        return None;
    }
    let mut wrapper: Option<&str> = None;
    for path in file_paths {
        let (first, _rest) = path.split_once('/')?;
        if first.is_empty() {
            return None;
        }
        match wrapper {
            None => wrapper = Some(first),
            Some(existing) if existing == first => {}
            _ => return None,
        }
    }
    wrapper.map(str::to_string)
}

fn strip_wrapper_folder(file_paths: Vec<String>) -> (Vec<String>, Option<String>) {
    match detect_wrapper_folder(&file_paths) {
        Some(wrapper) => {
            let prefix = format!("{wrapper}/");
            let stripped = file_paths
                .into_iter()
                .map(|p| p.strip_prefix(&prefix).unwrap_or(&p).to_string())
                .collect();
            (stripped, Some(wrapper))
        }
        None => (file_paths, None),
    }
}

/// Runs detection against a real, already-extracted directory (either a
/// freshly-imported instance's `server/` folder, or a folder the user
/// picked directly for "import from folder").
pub fn detect_from_dir(root: &Path) -> DetectedServerInfo {
    let mut file_paths = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.file_type().is_file() {
            if let Ok(rel) = entry.path().strip_prefix(root) {
                file_paths.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let (file_paths, wrapper) = strip_wrapper_folder(file_paths);
    let read_root = match &wrapper {
        Some(wrapper) => root.join(wrapper),
        None => root.to_path_buf(),
    };
    let mut info = analyze(&file_paths, &|rel: &str| read_text_file(&read_root.join(rel)));
    if let Some(wrapper) = wrapper {
        info.warnings.insert(
            0,
            format!("Removed wrapping folder \"{wrapper}\" - its contents were treated as the server root."),
        );
    }
    info
}

fn read_text_file(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_SCRIPT_BYTES as u64 {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// Runs detection against a ZIP archive's file listing, without extracting
/// anything - used for the importer wizard's "review before importing"
/// step, so a multi-gigabyte modpack doesn't have to be unpacked twice.
pub fn detect_from_zip(zip_path: &Path) -> std::io::Result<DetectedServerInfo> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let mut file_paths = Vec::new();
    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            if !entry.is_dir() {
                file_paths.push(entry.name().replace('\\', "/"));
            }
        }
    }

    let (file_paths, wrapper) = strip_wrapper_folder(file_paths);
    let prefix = wrapper.as_ref().map(|w| format!("{w}/")).unwrap_or_default();
    // `ZipArchive::by_name` needs `&mut self`, but `analyze` only ever
    // reads one file at a time - a `RefCell` keeps that borrow local
    // instead of threading mutability through the whole signature.
    let archive = RefCell::new(archive);
    let mut info = analyze(&file_paths, &|rel: &str| {
        read_zip_entry(&mut archive.borrow_mut(), &format!("{prefix}{rel}"))
    });
    if let Some(wrapper) = wrapper {
        info.warnings.insert(
            0,
            format!("Removed wrapping folder \"{wrapper}\" - its contents were treated as the server root."),
        );
    }
    Ok(info)
}

fn read_zip_entry(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> Option<String> {
    use std::io::Read;

    // Entry names were normalized to forward slashes for analysis, so a
    // (rare) archive written with backslashes needs its own spelling back.
    let mut entry = match archive.index_for_name(name) {
        Some(index) => archive.by_index(index).ok()?,
        None => archive.by_name(&name.replace('/', "\\")).ok()?,
    };
    if entry.size() > MAX_SCRIPT_BYTES as u64 {
        return None;
    }
    let mut buf = String::new();
    entry.read_to_string(&mut buf).ok()?;
    Some(buf)
}

/// Shared heuristics: given every file's path (forward-slash, relative to
/// the server root), guess loader/version/JAR/mods/world/etc.
///
/// This is deliberately best-effort. Server layouts vary a lot in the wild,
/// so anything not clearly identifiable is left as `None`/`false` plus a
/// warning, rather than guessed with false confidence.
fn analyze(file_paths: &[String], read: &ReadFile) -> DetectedServerInfo {
    let mut info = DetectedServerInfo::default();
    let lower_paths: Vec<String> = file_paths.iter().map(|p| p.to_lowercase()).collect();

    // An "-installer.jar" is never itself a runnable server - it's a GUI
    // wizard that has to be run once (see `commands::forge_install`) to
    // generate the real server files. Picking it as `server_jar` doesn't
    // fail loudly: `java -jar` on it just pops up that installer's GUI
    // every time "Start" is pressed, which looks like the server is
    // broken rather than simply not installed yet.
    let root_jars: Vec<&str> = file_paths
        .iter()
        .zip(&lower_paths)
        .filter(|(_, lower)| {
            !lower.contains('/') && lower.ends_with(".jar") && !lower.contains("installer")
        })
        .map(|(original, _)| original.as_str())
        .collect();

    let installer_jar_present = file_paths
        .iter()
        .zip(&lower_paths)
        .any(|(_, lower)| !lower.contains('/') && lower.ends_with("installer.jar"));

    info.has_mods_folder = lower_paths.iter().any(|p| p.starts_with("mods/"));
    info.mod_count = lower_paths
        .iter()
        .filter(|p| p.starts_with("mods/") && p.ends_with(".jar"))
        .count();

    info.has_config_folder = lower_paths.iter().any(|p| p.starts_with("config/"));

    info.has_server_properties = lower_paths.iter().any(|p| p == "server.properties");

    info.start_scripts = file_paths
        .iter()
        .zip(&lower_paths)
        .filter(|(_, lower)| !lower.contains('/') && START_SCRIPT_NAMES.contains(&lower.as_str()))
        .map(|(original, _)| original.clone())
        .collect();

    // A world folder is recognized by a `level.dat` sitting exactly one
    // directory deep - i.e. `<worldname>/level.dat`.
    for (original, lower) in file_paths.iter().zip(&lower_paths) {
        if lower.ends_with("/level.dat") {
            if let Some((folder, _)) = original.split_once('/') {
                info.has_world_folder = true;
                info.world_folder_name = Some(folder.to_string());
                break;
            }
        }
    }

    detect_loader_and_version(&lower_paths, &root_jars, &mut info);

    // A modern (1.17+) Forge/NeoForge server, once actually installed,
    // launches via an argfile instead of a plain jar - takes priority over
    // whatever `detect_loader_and_version` picked from `root_jars` above,
    // since an installed server directory can still have a leftover
    // installer jar sitting alongside the real thing.
    if let Some(argfile) = find_loader_argfile(file_paths, &lower_paths) {
        info.server_jar = Some(argfile);
        info.server_jar_is_argfile = true;
    }

    // Nothing launchable found from paths alone. Plenty of server packs
    // ship only a `run.bat`/`start.sh`, so before giving up, read what the
    // pack's own script says it launches - and failing that, fall back to
    // running that script itself.
    if info.server_jar.is_none() && !info.start_scripts.is_empty() {
        resolve_from_start_scripts(file_paths, &lower_paths, read, &mut info);
    }

    if info.server_jar.is_none() && installer_jar_present {
        info.warnings.push(
            "This is a Forge/NeoForge installer, not a ready-to-run server yet. Import it, \
             then use \"Install Forge/NeoForge Server\" on the instance to finish setting it up."
                .to_string(),
        );
    } else if info.server_jar.is_none() && info.start_scripts.is_empty() {
        info.warnings.push(
            "No server JAR or start script detected. You'll need to configure this instance manually.".to_string(),
        );
    } else if info.server_jar_is_script {
        info.warnings.push(format!(
            "No server JAR found, so this instance will be started through the pack's own \
             \"{}\" script. Its console still appears here, but the RAM settings below come \
             from that script rather than ModpackPilot.",
            info.server_jar.as_deref().unwrap_or_default()
        ));
    }

    if !info.has_server_properties {
        info.warnings
            .push("No server.properties found.".to_string());
    }

    info
}

/// The start-script flavour this build can actually execute. A `run.sh`
/// on Windows (or a `run.bat` on Linux) is still worth *reading* - the jar
/// it names is the same either way - but it can never be the fallback that
/// gets run directly.
#[cfg(windows)]
const PLATFORM_SCRIPT_EXT: &str = ".bat";
#[cfg(not(windows))]
const PLATFORM_SCRIPT_EXT: &str = ".sh";

/// Last-resort launch resolution for packs that ship a start script and
/// nothing else recognizable.
///
/// Reading the script is tried first and preferred by a wide margin:
/// a parsed jar/argfile is launched as a direct child of ModpackPilot,
/// which is what makes console input, graceful `stop`, and reliable
/// process control work. Running the script itself is only for scripts
/// this can't parse.
fn resolve_from_start_scripts(
    file_paths: &[String],
    lower_paths: &[String],
    read: &ReadFile,
    info: &mut DetectedServerInfo,
) {
    // Same-platform scripts first: both usually name the same jar, but
    // Forge/NeoForge argfiles are platform-specific, and only a
    // same-platform script is runnable as the fallback below.
    let mut scripts = info.start_scripts.clone();
    scripts.sort_by_key(|s| !s.to_lowercase().ends_with(PLATFORM_SCRIPT_EXT));

    for script in &scripts {
        let Some(contents) = read(script) else {
            continue;
        };
        let Some(launch) = parse_start_script(&contents) else {
            continue;
        };
        // A parsed path that isn't in the pack means the script builds or
        // downloads it at runtime - not something to point `-jar` at.
        let target = existing_path(&launch.target, file_paths, lower_paths).or_else(|| {
            launch
                .is_argfile
                .then(|| swapped_platform_argfile(&launch.target, file_paths, lower_paths))
                .flatten()
        });
        let Some(target) = target else {
            continue;
        };

        info.server_jar = Some(target);
        info.server_jar_is_argfile = launch.is_argfile;
        return;
    }

    if let Some(script) = scripts
        .iter()
        .find(|s| s.to_lowercase().ends_with(PLATFORM_SCRIPT_EXT))
    {
        info.server_jar = Some(script.clone());
        info.server_jar_is_argfile = false;
        info.server_jar_is_script = true;
    }
}

/// Returns the pack's own spelling of `candidate` if it exists at all
/// (case-insensitively - Windows packs are routinely inconsistent about
/// case in paths their scripts reference).
fn existing_path(candidate: &str, file_paths: &[String], lower_paths: &[String]) -> Option<String> {
    let needle = candidate.to_lowercase();
    file_paths
        .iter()
        .zip(lower_paths)
        .find(|(_, lower)| **lower == needle)
        .map(|(original, _)| original.clone())
}

/// A `run.sh` read on Windows (or vice versa) names the *other* platform's
/// argfile, whose classpath uses the wrong separator and would fail at
/// launch. The installer always writes both, so swap in this platform's.
fn swapped_platform_argfile(
    target: &str,
    file_paths: &[String],
    lower_paths: &[String],
) -> Option<String> {
    let other = if LOADER_ARGFILE_SUFFIX == "win_args.txt" { "unix_args.txt" } else { "win_args.txt" };
    let stripped = target.to_lowercase().strip_suffix(other).map(str::to_string)?;
    existing_path(&format!("{stripped}{LOADER_ARGFILE_SUFFIX}"), file_paths, lower_paths)
}

fn detect_loader_and_version(
    lower_paths: &[String],
    root_jars: &[&str],
    info: &mut DetectedServerInfo,
) {
    let any_path_contains = |needle: &str| lower_paths.iter().any(|p| p.contains(needle));

    if any_path_contains("neoforge") {
        info.loader = ServerLoader::NeoForge;
        let re = Regex::new(r"neoforge-([\d.]+)").unwrap();
        info.loader_version = find_capture(lower_paths, &re, 1);
    } else if any_path_contains("minecraftforge") || any_path_contains("forge-") {
        info.loader = ServerLoader::Forge;
        let re = Regex::new(r"forge-(\d+\.\d+(?:\.\d+)?)-([\d.]+)").unwrap();
        if let Some(path) = lower_paths.iter().find(|p| re.is_match(p)) {
            if let Some(caps) = re.captures(path) {
                info.minecraft_version = Some(caps[1].to_string());
                info.loader_version = Some(caps[2].to_string());
            }
        }
    } else if any_path_contains("fabric") {
        info.loader = ServerLoader::Fabric;
        let re = Regex::new(r"fabric-server-mc\.([\d.]+)-loader\.([\d.]+)").unwrap();
        if let Some(path) = lower_paths.iter().find(|p| re.is_match(p)) {
            if let Some(caps) = re.captures(path) {
                info.minecraft_version = Some(caps[1].to_string());
                info.loader_version = Some(caps[2].to_string());
            }
        }
    } else if any_path_contains("quilt") {
        info.loader = ServerLoader::Quilt;
    } else if root_jars.iter().any(|j| j.to_lowercase() == "server.jar")
        || any_path_contains("minecraft_server")
    {
        info.loader = ServerLoader::Vanilla;
        let re = Regex::new(r"minecraft_server\.([\d.]+)\.jar").unwrap();
        info.minecraft_version = find_capture(lower_paths, &re, 1);
    }

    info.server_jar = pick_server_jar(root_jars, info.loader);
}

/// The platform-specific argfile suffix Forge/NeoForge's installer writes
/// (`win_args.txt` on Windows, `unix_args.txt` everywhere else) - the same
/// one `run.bat`/`run.sh` itself invokes.
#[cfg(windows)]
const LOADER_ARGFILE_SUFFIX: &str = "win_args.txt";
#[cfg(not(windows))]
const LOADER_ARGFILE_SUFFIX: &str = "unix_args.txt";

/// Finds a modern Forge/NeoForge server's `@`-argfile, e.g.
/// `libraries/net/minecraftforge/forge/1.20.1-47.4.0/win_args.txt` - see
/// `Instance::launch_mode` for what happens once one is found.
fn find_loader_argfile(file_paths: &[String], lower_paths: &[String]) -> Option<String> {
    file_paths
        .iter()
        .zip(lower_paths)
        .find(|(_, lower)| {
            (lower.starts_with("libraries/net/minecraftforge/forge/")
                || lower.starts_with("libraries/net/neoforged/neoforge/"))
                && lower.ends_with(LOADER_ARGFILE_SUFFIX)
        })
        .map(|(original, _)| original.clone())
}

fn find_capture(paths: &[String], re: &Regex, group: usize) -> Option<String> {
    paths
        .iter()
        .find_map(|p| re.captures(p).and_then(|c| c.get(group)).map(|m| m.as_str().to_string()))
}

/// Picks the most likely launchable JAR from the server root, preferring
/// well-known names for the detected loader over an arbitrary guess.
fn pick_server_jar(root_jars: &[&str], loader: ServerLoader) -> Option<String> {
    if root_jars.is_empty() {
        return None;
    }

    let preferred = match loader {
        ServerLoader::Vanilla => root_jars.iter().find(|j| j.to_lowercase() == "server.jar"),
        ServerLoader::Forge => root_jars.iter().find(|j| j.to_lowercase().contains("forge")),
        ServerLoader::NeoForge => root_jars
            .iter()
            .find(|j| j.to_lowercase().contains("neoforge")),
        ServerLoader::Fabric => root_jars.iter().find(|j| j.to_lowercase().contains("fabric")),
        ServerLoader::Quilt => root_jars.iter().find(|j| j.to_lowercase().contains("quilt")),
        ServerLoader::Unknown => None,
    };

    preferred.or(root_jars.first()).map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a throwaway server pack on disk from `(relative path,
    /// contents)` pairs, so detection can be exercised the same way it
    /// runs for real (`detect_from_dir`) rather than against a synthetic
    /// path list.
    fn pack(label: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
        let root = std::env::temp_dir()
            .join(format!("modpackpilot-detect-{label}-{}", uuid::Uuid::new_v4()));
        for (path, contents) in files {
            let full = root.join(path);
            std::fs::create_dir_all(full.parent().expect("parent")).expect("create dirs");
            std::fs::write(full, contents).expect("write file");
        }
        root
    }

    fn platform_script(name: &str) -> String {
        format!("{name}{PLATFORM_SCRIPT_EXT}")
    }

    fn platform_argfile(version: &str) -> String {
        format!("libraries/net/neoforged/neoforge/{version}/{LOADER_ARGFILE_SUFFIX}")
    }

    /// The case this whole path exists for: a server pack whose only clue
    /// is its start script, with the argfile somewhere detection's own
    /// hard-coded lookup would have found - it must still agree with what
    /// the script says.
    #[test]
    fn reads_the_launch_target_out_of_the_start_script() {
        let argfile = platform_argfile("21.1.72");
        let script = platform_script("run");
        let root = pack(
            "script-argfile",
            &[
                (script.as_str(), &format!("java @user_jvm_args.txt @{argfile} %*\n")),
                (argfile.as_str(), "-cp\nlibs\n"),
                ("mods/example.jar", "x"),
                ("user_jvm_args.txt", "-Xmx4G\n"),
            ],
        );

        let info = detect_from_dir(&root);

        assert_eq!(info.server_jar.as_deref(), Some(argfile.as_str()));
        assert!(info.server_jar_is_argfile);
        assert!(!info.server_jar_is_script);
        assert_eq!(info.launch_mode(), "argfile");
    }

    /// A jar the hard-coded root-level scan can't see, because it isn't at
    /// the root - only the script knows where it went.
    #[test]
    fn finds_a_jar_the_script_points_into_a_subfolder() {
        let script = platform_script("run");
        let root = pack(
            "script-subfolder-jar",
            &[
                (script.as_str(), "java -Xmx4G -jar ./server/quilt-server-launch.jar nogui\n"),
                ("server/quilt-server-launch.jar", "x"),
                ("mods/example.jar", "x"),
            ],
        );

        let info = detect_from_dir(&root);

        assert_eq!(info.server_jar.as_deref(), Some("server/quilt-server-launch.jar"));
        assert!(!info.server_jar_is_argfile);
        assert_eq!(info.launch_mode(), "jar");
    }

    /// A script naming something the pack doesn't contain is a script that
    /// builds or downloads it at runtime - launching that path directly
    /// would fail, so the script itself has to be run instead.
    #[test]
    fn falls_back_to_running_the_script_when_its_target_is_missing() {
        let script = platform_script("run");
        let root = pack(
            "script-missing-target",
            &[
                (script.as_str(), "java -jar downloaded-later.jar nogui\n"),
                ("mods/example.jar", "x"),
            ],
        );

        let info = detect_from_dir(&root);

        assert_eq!(info.server_jar.as_deref(), Some(script.as_str()));
        assert!(info.server_jar_is_script);
        assert_eq!(info.launch_mode(), "script");
        assert!(
            info.warnings.iter().any(|w| w.contains(&script)),
            "the user should be told the pack's script is what runs: {:?}",
            info.warnings,
        );
    }

    /// An unparseable script (here: a wrapper around another script) is
    /// still perfectly runnable - it just can't be reduced to a jar.
    #[test]
    fn falls_back_to_running_an_unparseable_script() {
        let script = platform_script("run");
        let root = pack(
            "script-unparseable",
            &[
                (script.as_str(), "call ServerStart.bat\n"),
                ("ServerStart.bat", "java -jar whatever.jar\n"),
                ("mods/example.jar", "x"),
            ],
        );

        let info = detect_from_dir(&root);

        assert_eq!(info.server_jar.as_deref(), Some(script.as_str()));
        assert!(info.server_jar_is_script);
    }

    /// A root jar is unambiguous, and reading a script could only ever
    /// contradict it - so the script is never consulted in that case.
    #[test]
    fn a_real_server_jar_still_wins_over_the_script() {
        let script = platform_script("run");
        let root = pack(
            "root-jar-wins",
            &[
                (script.as_str(), "java -jar something-else.jar nogui\n"),
                ("something-else.jar", "x"),
                ("server.jar", "x"),
            ],
        );

        let info = detect_from_dir(&root);

        assert_eq!(info.server_jar.as_deref(), Some("server.jar"));
        assert!(!info.server_jar_is_script);
    }

    /// Wrapper-folder stripping rewrites every analyzed path; the script
    /// still has to be found on disk under its original location.
    #[test]
    fn reads_a_script_inside_a_wrapper_folder() {
        let script = platform_script("run");
        let root = pack(
            "script-wrapped",
            &[
                (
                    format!("MyPack-1.0/{script}").as_str(),
                    "java -jar fabric-server-launch.jar nogui\n",
                ),
                ("MyPack-1.0/fabric-server-launch.jar", "x"),
                ("MyPack-1.0/mods/example.jar", "x"),
            ],
        );

        let info = detect_from_dir(&root);

        assert_eq!(info.server_jar.as_deref(), Some("fabric-server-launch.jar"));
        assert_eq!(info.launch_mode(), "jar");
    }
}
