//! Reads what a mod jar declares about itself.
//!
//! Every loader stores this in its own file and its own format, so this
//! module's job is to flatten four dialects into one [`ModMetadata`]:
//!
//! | Loader              | File                          | Format |
//! |---------------------|-------------------------------|--------|
//! | Forge 1.13+         | `META-INF/mods.toml`          | TOML   |
//! | NeoForge            | `META-INF/neoforge.mods.toml` | TOML   |
//! | Fabric              | `fabric.mod.json`             | JSON   |
//! | Quilt               | `quilt.mod.json`              | JSON   |
//! | Forge 1.12 and older| `mcmod.info`                  | JSON   |
//!
//! What a loader does *not* declare is left as `None` rather than inferred.
//! The clearest case is `environment`: Fabric and Quilt state a mod's side
//! outright, and Forge simply has no equivalent field - so a Forge mod's
//! side is genuinely unknown, and reporting it as "both" would turn a gap
//! in the format into a confident-looking claim.

use std::io::Read;
use std::path::Path;

use serde::Serialize;

/// Caps how much of a manifest is read into memory. These files are a few
/// KB; anything approaching this is malformed or hostile, and an unbounded
/// read of an attacker-chosen entry is a trivial way to exhaust memory.
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

/// Which loader a jar is built for, as the jar itself declares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModLoaderKind {
    Forge,
    NeoForge,
    Fabric,
    Quilt,
}

impl ModLoaderKind {
    /// Whether a jar built for `self` will load on a server running
    /// `instance_loader` (the `loader` column's spelling).
    ///
    /// NeoForge is deliberately not treated as Forge-compatible in either
    /// direction. They diverged at 1.20.1 and a Forge jar dropped into a
    /// NeoForge server is one of the most common causes of a modpack that
    /// will not boot, which is exactly the failure this check exists to
    /// catch.
    pub fn runs_on(self, instance_loader: &str) -> bool {
        matches!(
            (self, instance_loader.to_ascii_lowercase().as_str()),
            (ModLoaderKind::Forge, "forge")
                | (ModLoaderKind::NeoForge, "neoforge")
                | (ModLoaderKind::Fabric, "fabric")
                | (ModLoaderKind::Quilt, "quilt")
                // Quilt is explicitly a drop-in host for Fabric mods.
                | (ModLoaderKind::Fabric, "quilt")
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            ModLoaderKind::Forge => "Forge",
            ModLoaderKind::NeoForge => "NeoForge",
            ModLoaderKind::Fabric => "Fabric",
            ModLoaderKind::Quilt => "Quilt",
        }
    }
}

/// Which side of the game a mod is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModEnvironment {
    /// Runs on both sides.
    Both,
    /// Client only - on a server it is dead weight at best.
    Client,
    /// Server only.
    Server,
    /// The loader's format has no field for this (Forge, NeoForge), so no
    /// claim is made either way.
    Unknown,
}

/// One entry from a mod's declared dependency list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModDependency {
    pub mod_id: String,
    /// False for Forge's `mandatory=false` and Fabric's
    /// `recommends`/`suggests` - a missing optional dependency is not a
    /// problem and is never reported as one.
    pub required: bool,
    /// The raw range as written. Only interpreted for Maven syntax; see
    /// [`super::version::maven_range_contains`].
    pub version_range: Option<String>,
}

/// What one jar declares about itself.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModMetadata {
    /// The name on disk, including any `.disabled` suffix - this is the
    /// handle `toggle_mod`/`delete_mod` expect back.
    pub file_name: String,
    pub display_name: Option<String>,
    pub mod_id: Option<String>,
    pub version: Option<String>,
    pub loader: Option<ModLoaderKind>,
    pub environment: ModEnvironment,
    pub dependencies: Vec<ModDependency>,
    pub enabled: bool,
    pub size_bytes: u64,
    /// Why this jar could not be understood, if it couldn't. A jar with a
    /// reason set is reported as unreadable rather than silently ignored.
    pub error: Option<String>,
}

impl ModMetadata {
    /// A jar that exists but told us nothing usable.
    fn unreadable(file_name: String, enabled: bool, size_bytes: u64, error: String) -> Self {
        Self {
            file_name,
            display_name: None,
            mod_id: None,
            version: None,
            loader: None,
            environment: ModEnvironment::Unknown,
            dependencies: Vec::new(),
            enabled,
            size_bytes,
            error: Some(error),
        }
    }

    /// The best human-facing name available: what the mod calls itself,
    /// falling back to its id and then to the filename.
    pub fn best_name(&self) -> &str {
        self.display_name
            .as_deref()
            .filter(|n| !n.trim().is_empty())
            .or(self.mod_id.as_deref())
            .unwrap_or(&self.file_name)
    }
}

/// Parses one manifest dialect. Takes the file's contents and the entry
/// name it came from, since Forge and NeoForge share a format and are told
/// apart only by which filename carried it.
type ManifestParser = fn(&str, &str) -> Option<ModMetadata>;

/// Reads one jar's metadata.
///
/// Blocking, and intended to be called from `spawn_blocking`: a modded
/// server has hundreds to thousands of jars, and each one is a zip that
/// must be opened and read.
///
/// Never returns `Err`. A jar that cannot be opened, is not a zip, or
/// carries no recognizable manifest comes back as a `ModMetadata` with
/// `error` set, because "this file in your mods folder is not a loadable
/// mod" is itself one of the findings worth surfacing - dropping it would
/// hide exactly the thing an operator needs to see.
pub fn read_jar(path: &Path) -> ModMetadata {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let enabled = file_name.ends_with(".jar");
    let size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) => {
            return ModMetadata::unreadable(file_name, enabled, size_bytes, format!("Cannot open file: {e}"))
        }
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(archive) => archive,
        Err(e) => {
            return ModMetadata::unreadable(
                file_name,
                enabled,
                size_bytes,
                format!("Not a valid JAR archive: {e}"),
            )
        }
    };

    // Ordered so the most specific manifest wins. A NeoForge jar that also
    // ships a legacy `mods.toml` for cross-compatibility should be read as
    // NeoForge, and a Quilt mod shipping a `fabric.mod.json` fallback as
    // Quilt.
    let sources: &[(&str, ManifestParser)] = &[
        ("META-INF/neoforge.mods.toml", parse_forge_toml),
        ("META-INF/mods.toml", parse_forge_toml),
        ("quilt.mod.json", parse_quilt_json),
        ("fabric.mod.json", parse_fabric_json),
        ("mcmod.info", parse_mcmod_info),
    ];

    for (entry_name, parse) in sources {
        let Some(contents) = read_entry(&mut archive, entry_name) else {
            continue;
        };
        if let Some(mut metadata) = parse(&contents, entry_name) {
            metadata.file_name = file_name;
            metadata.enabled = enabled;
            metadata.size_bytes = size_bytes;
            return metadata;
        }
    }

    ModMetadata::unreadable(
        file_name,
        enabled,
        size_bytes,
        "No mod manifest found (not a Forge, NeoForge, Fabric or Quilt mod)".to_string(),
    )
}

fn read_entry(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    let entry = archive.by_name(name).ok()?;
    if entry.size() > MAX_MANIFEST_BYTES {
        return None;
    }
    let mut contents = String::new();
    entry.take(MAX_MANIFEST_BYTES).read_to_string(&mut contents).ok()?;
    Some(contents)
}

// ---------------------------------------------------------------------------
// Forge / NeoForge
// ---------------------------------------------------------------------------

/// Parses `META-INF/mods.toml` (Forge) or `neoforge.mods.toml` (NeoForge).
///
/// Shape:
///
/// ```toml
/// modLoader = "javafml"
/// [[mods]]
/// modId = "jei"
/// version = "15.2.0.27"
/// displayName = "Just Enough Items"
/// [[dependencies.jei]]
/// modId = "minecraft"
/// mandatory = true
/// versionRange = "[1.20.1,1.21)"
/// ```
///
/// Only the first `[[mods]]` entry is taken as the jar's identity. A jar
/// declaring several is a bundle, and the first is the one whose name the
/// file is published under.
fn parse_forge_toml(contents: &str, entry_name: &str) -> Option<ModMetadata> {
    let value: toml::Value = toml::from_str(contents).ok()?;

    let first_mod = value.get("mods")?.as_array()?.first()?;
    let mod_id = first_mod.get("modId").and_then(|v| v.as_str()).map(str::to_string);

    let loader = if entry_name.contains("neoforge") {
        ModLoaderKind::NeoForge
    } else {
        ModLoaderKind::Forge
    };

    // `dependencies` is a table keyed by the depending mod's id, each
    // holding an array of dependency tables. Only this jar's own key is
    // relevant; a bundle's other entries describe other mods.
    let mut dependencies = Vec::new();
    if let Some(table) = value.get("dependencies").and_then(|v| v.as_table()) {
        let own = mod_id
            .as_deref()
            .and_then(|id| table.get(id))
            .or_else(|| (table.len() == 1).then(|| table.values().next()).flatten());

        for entry in own.and_then(|v| v.as_array()).into_iter().flatten() {
            let Some(dep_id) = entry.get("modId").and_then(|v| v.as_str()) else {
                continue;
            };
            dependencies.push(ModDependency {
                mod_id: dep_id.to_ascii_lowercase(),
                // Forge's default when the key is absent is `true`.
                required: entry.get("mandatory").and_then(|v| v.as_bool()).unwrap_or(true),
                version_range: entry
                    .get("versionRange")
                    .and_then(|v| v.as_str())
                    .filter(|r| !r.trim().is_empty())
                    .map(str::to_string),
            });
        }
    }

    Some(ModMetadata {
        file_name: String::new(),
        display_name: first_mod
            .get("displayName")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        mod_id: mod_id.map(|id| id.to_ascii_lowercase()),
        version: first_mod.get("version").and_then(|v| v.as_str()).map(str::to_string),
        loader: Some(loader),
        // Forge has no side field. Saying "Both" here would be an
        // invention; see the module docs.
        environment: ModEnvironment::Unknown,
        dependencies,
        enabled: true,
        size_bytes: 0,
        error: None,
    })
}

// ---------------------------------------------------------------------------
// Fabric
// ---------------------------------------------------------------------------

/// Parses `fabric.mod.json`.
///
/// `depends` is required, `recommends` and `suggests` are not - only the
/// first produces a finding when unmet. `environment` is one of `"*"`,
/// `"client"` or `"server"`, and is the one authoritative side declaration
/// any loader makes.
fn parse_fabric_json(contents: &str, _entry_name: &str) -> Option<ModMetadata> {
    let value: serde_json::Value = serde_json::from_str(contents).ok()?;
    let id = value.get("id")?.as_str()?.to_ascii_lowercase();

    let mut dependencies = Vec::new();
    for (key, required) in [("depends", true), ("recommends", false), ("suggests", false)] {
        collect_json_dependency_map(value.get(key), required, &mut dependencies);
    }

    Some(ModMetadata {
        file_name: String::new(),
        display_name: value.get("name").and_then(|v| v.as_str()).map(str::to_string),
        mod_id: Some(id),
        version: value.get("version").and_then(|v| v.as_str()).map(str::to_string),
        loader: Some(ModLoaderKind::Fabric),
        environment: match value.get("environment").and_then(|v| v.as_str()) {
            Some("client") => ModEnvironment::Client,
            Some("server") => ModEnvironment::Server,
            Some("*") => ModEnvironment::Both,
            // Fabric's default when the key is omitted is "*".
            None => ModEnvironment::Both,
            Some(_) => ModEnvironment::Unknown,
        },
        dependencies,
        enabled: true,
        size_bytes: 0,
        error: None,
    })
}

/// Reads Fabric's `{ "modid": "range" }` dependency shape. The value may
/// also be an array of ranges, which is treated as "any of", so only the
/// id is kept.
fn collect_json_dependency_map(
    value: Option<&serde_json::Value>,
    required: bool,
    out: &mut Vec<ModDependency>,
) {
    let Some(map) = value.and_then(|v| v.as_object()) else {
        return;
    };
    for (mod_id, range) in map {
        out.push(ModDependency {
            mod_id: mod_id.to_ascii_lowercase(),
            required,
            version_range: range.as_str().map(str::to_string),
        });
    }
}

// ---------------------------------------------------------------------------
// Quilt
// ---------------------------------------------------------------------------

/// Parses `quilt.mod.json`, whose fields live under a `quilt_loader` object
/// and whose `depends` is an array of objects rather than a map.
fn parse_quilt_json(contents: &str, _entry_name: &str) -> Option<ModMetadata> {
    let value: serde_json::Value = serde_json::from_str(contents).ok()?;
    let loader_block = value.get("quilt_loader")?;
    let id = loader_block.get("id")?.as_str()?.to_ascii_lowercase();

    let mut dependencies = Vec::new();
    for entry in loader_block.get("depends").and_then(|v| v.as_array()).into_iter().flatten() {
        // An entry is either a bare id string or an object with `id`,
        // `versions` and an `optional` flag.
        let (dep_id, required, range) = match entry {
            serde_json::Value::String(id) => (id.clone(), true, None),
            serde_json::Value::Object(_) => {
                let Some(dep_id) = entry.get("id").and_then(|v| v.as_str()) else {
                    continue;
                };
                (
                    dep_id.to_string(),
                    !entry.get("optional").and_then(|v| v.as_bool()).unwrap_or(false),
                    entry.get("versions").and_then(|v| v.as_str()).map(str::to_string),
                )
            }
            _ => continue,
        };
        dependencies.push(ModDependency {
            mod_id: dep_id.to_ascii_lowercase(),
            required,
            version_range: range,
        });
    }

    let metadata_block = loader_block.get("metadata");

    Some(ModMetadata {
        file_name: String::new(),
        display_name: metadata_block
            .and_then(|m| m.get("name"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        mod_id: Some(id),
        version: loader_block.get("version").and_then(|v| v.as_str()).map(str::to_string),
        loader: Some(ModLoaderKind::Quilt),
        environment: match value
            .get("minecraft")
            .and_then(|m| m.get("environment"))
            .and_then(|v| v.as_str())
        {
            Some("client") => ModEnvironment::Client,
            Some("dedicated_server") => ModEnvironment::Server,
            Some("*") | None => ModEnvironment::Both,
            Some(_) => ModEnvironment::Unknown,
        },
        dependencies,
        enabled: true,
        size_bytes: 0,
        error: None,
    })
}

// ---------------------------------------------------------------------------
// Legacy Forge (1.12 and older)
// ---------------------------------------------------------------------------

/// Parses `mcmod.info`, which is either a bare array of mod objects or a
/// `{ "modList": [...] }` wrapper.
///
/// Its dependency fields are free-form strings that mod authors used
/// inconsistently, so only `requiredMods` is read, and only for ids -
/// there is no reliable version range to be had.
fn parse_mcmod_info(contents: &str, _entry_name: &str) -> Option<ModMetadata> {
    let value: serde_json::Value = serde_json::from_str(contents).ok()?;
    let list = match &value {
        serde_json::Value::Array(items) => items.clone(),
        serde_json::Value::Object(_) => value.get("modList")?.as_array()?.clone(),
        _ => return None,
    };
    let first = list.first()?;
    let id = first.get("modid")?.as_str()?.to_ascii_lowercase();

    let dependencies = first
        .get("requiredMods")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str())
        // Entries are written as "modid@[1.0,)"; only the id is dependable.
        .map(|raw| ModDependency {
            mod_id: raw.split('@').next().unwrap_or(raw).trim().to_ascii_lowercase(),
            required: true,
            version_range: None,
        })
        .filter(|d| !d.mod_id.is_empty())
        .collect();

    Some(ModMetadata {
        file_name: String::new(),
        display_name: first.get("name").and_then(|v| v.as_str()).map(str::to_string),
        mod_id: Some(id),
        version: first.get("version").and_then(|v| v.as_str()).map(str::to_string),
        loader: Some(ModLoaderKind::Forge),
        environment: ModEnvironment::Unknown,
        dependencies,
        enabled: true,
        size_bytes: 0,
        error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_forge_manifest() {
        let toml = r#"
modLoader = "javafml"
loaderVersion = "[47,)"
license = "MIT"

[[mods]]
modId = "jei"
version = "15.2.0.27"
displayName = "Just Enough Items"

[[dependencies.jei]]
modId = "forge"
mandatory = true
versionRange = "[47,)"

[[dependencies.jei]]
modId = "minecraft"
mandatory = true
versionRange = "[1.20.1,1.21)"

[[dependencies.jei]]
modId = "someoptional"
mandatory = false
"#;
        let m = parse_forge_toml(toml, "META-INF/mods.toml").unwrap();
        assert_eq!(m.mod_id.as_deref(), Some("jei"));
        assert_eq!(m.display_name.as_deref(), Some("Just Enough Items"));
        assert_eq!(m.loader, Some(ModLoaderKind::Forge));
        // Forge declares no side, and none is invented.
        assert_eq!(m.environment, ModEnvironment::Unknown);
        assert_eq!(m.dependencies.len(), 3);
        assert!(m.dependencies.iter().any(|d| d.mod_id == "minecraft" && d.required));
        assert!(m.dependencies.iter().any(|d| d.mod_id == "someoptional" && !d.required));
    }

    /// The same format under a different filename is NeoForge, and must not
    /// be reported as Forge - mixing the two is a top cause of a pack that
    /// won't boot.
    #[test]
    fn distinguishes_neoforge_by_manifest_name() {
        let toml = "[[mods]]\nmodId = \"example\"\n";
        let forge = parse_forge_toml(toml, "META-INF/mods.toml").unwrap();
        let neo = parse_forge_toml(toml, "META-INF/neoforge.mods.toml").unwrap();
        assert_eq!(forge.loader, Some(ModLoaderKind::Forge));
        assert_eq!(neo.loader, Some(ModLoaderKind::NeoForge));
        assert!(!ModLoaderKind::Forge.runs_on("neoforge"));
        assert!(!ModLoaderKind::NeoForge.runs_on("forge"));
    }

    #[test]
    fn reads_a_fabric_manifest_including_its_side() {
        let json = r#"{
            "schemaVersion": 1,
            "id": "sodium",
            "version": "0.5.3",
            "name": "Sodium",
            "environment": "client",
            "depends": { "minecraft": ">=1.20.1", "fabricloader": ">=0.14.0" },
            "recommends": { "fabric-api": "*" }
        }"#;
        let m = parse_fabric_json(json, "fabric.mod.json").unwrap();
        assert_eq!(m.mod_id.as_deref(), Some("sodium"));
        assert_eq!(m.loader, Some(ModLoaderKind::Fabric));
        assert_eq!(m.environment, ModEnvironment::Client);
        assert!(m.dependencies.iter().any(|d| d.mod_id == "minecraft" && d.required));
        assert!(m.dependencies.iter().any(|d| d.mod_id == "fabric-api" && !d.required));
    }

    #[test]
    fn a_fabric_mod_without_an_environment_runs_on_both() {
        let json = r#"{"id":"example","version":"1.0","depends":{}}"#;
        let m = parse_fabric_json(json, "fabric.mod.json").unwrap();
        assert_eq!(m.environment, ModEnvironment::Both);
    }

    #[test]
    fn reads_a_quilt_manifest() {
        let json = r#"{
            "schema_version": 1,
            "quilt_loader": {
                "id": "example",
                "version": "1.0.0",
                "metadata": { "name": "Example Mod" },
                "depends": [
                    { "id": "quilt_base", "versions": ">=1.0.0" },
                    { "id": "optionalthing", "optional": true },
                    "barestring"
                ]
            },
            "minecraft": { "environment": "dedicated_server" }
        }"#;
        let m = parse_quilt_json(json, "quilt.mod.json").unwrap();
        assert_eq!(m.mod_id.as_deref(), Some("example"));
        assert_eq!(m.display_name.as_deref(), Some("Example Mod"));
        assert_eq!(m.environment, ModEnvironment::Server);
        assert!(m.dependencies.iter().any(|d| d.mod_id == "quilt_base" && d.required));
        assert!(m.dependencies.iter().any(|d| d.mod_id == "optionalthing" && !d.required));
        assert!(m.dependencies.iter().any(|d| d.mod_id == "barestring" && d.required));
    }

    #[test]
    fn reads_a_legacy_mcmod_info() {
        let json = r#"[{
            "modid": "oldmod",
            "name": "Old Mod",
            "version": "1.2.3",
            "requiredMods": ["forge@[14.23,)", "anothermod"]
        }]"#;
        let m = parse_mcmod_info(json, "mcmod.info").unwrap();
        assert_eq!(m.mod_id.as_deref(), Some("oldmod"));
        assert_eq!(m.loader, Some(ModLoaderKind::Forge));
        // The "@range" suffix is stripped; only the id is dependable here.
        assert!(m.dependencies.iter().any(|d| d.mod_id == "forge"));
        assert!(m.dependencies.iter().any(|d| d.mod_id == "anothermod"));
    }

    #[test]
    fn rejects_manifests_it_cannot_understand() {
        assert!(parse_forge_toml("this is not toml {{{", "META-INF/mods.toml").is_none());
        // Valid TOML, but no [[mods]] block - not a mod manifest.
        assert!(parse_forge_toml("modLoader = \"javafml\"", "META-INF/mods.toml").is_none());
        assert!(parse_fabric_json("{}", "fabric.mod.json").is_none());
        assert!(parse_quilt_json(r#"{"id":"x"}"#, "quilt.mod.json").is_none());
    }

    /// Exercises the real path: build an actual JAR on disk, then read it
    /// back through `read_jar`. The parser tests above never open a zip, so
    /// without this the entry lookup itself is untested.
    #[test]
    fn reads_metadata_out_of_a_real_jar_on_disk() {
        use std::io::Write;

        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("mpp-jar-test-{unique}"));
        std::fs::create_dir_all(&dir).unwrap();

        let jar_path = dir.join("examplemod-1.0.0.jar");
        {
            let file = std::fs::File::create(&jar_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("META-INF/mods.toml", options).unwrap();
            zip.write_all(
                br#"
modLoader = "javafml"
[[mods]]
modId = "examplemod"
version = "1.0.0"
displayName = "Example Mod"
[[dependencies.examplemod]]
modId = "minecraft"
mandatory = true
versionRange = "[1.20.1,1.21)"
"#,
            )
            .unwrap();
            // A class file, so the jar looks like a real mod rather than a
            // manifest in a zip.
            zip.start_file("com/example/Mod.class", options).unwrap();
            zip.write_all(b"class file contents").unwrap();
            zip.finish().unwrap();
        }

        let metadata = read_jar(&jar_path);
        assert_eq!(metadata.error, None, "{:?}", metadata.error);
        assert_eq!(metadata.file_name, "examplemod-1.0.0.jar");
        assert_eq!(metadata.mod_id.as_deref(), Some("examplemod"));
        assert_eq!(metadata.display_name.as_deref(), Some("Example Mod"));
        assert_eq!(metadata.loader, Some(ModLoaderKind::Forge));
        assert!(metadata.enabled);
        assert!(metadata.size_bytes > 0);
        assert_eq!(metadata.dependencies.len(), 1);

        // A disabled mod is recognized by its suffix, and still parses.
        let disabled_path = dir.join("examplemod-1.0.0.jar.disabled");
        std::fs::copy(&jar_path, &disabled_path).unwrap();
        let disabled = read_jar(&disabled_path);
        assert!(!disabled.enabled);
        assert_eq!(disabled.mod_id.as_deref(), Some("examplemod"));

        // Something that is not a zip at all must come back as a finding,
        // not be silently skipped.
        let junk_path = dir.join("truncated.jar");
        std::fs::write(&junk_path, b"this is not a zip file").unwrap();
        let junk = read_jar(&junk_path);
        assert!(junk.error.is_some());
        assert_eq!(junk.mod_id, None);

        // A valid zip with no mod manifest is also a finding.
        let plain_path = dir.join("plain.jar");
        {
            let file = std::fs::File::create(&plain_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file("readme.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"nothing here").unwrap();
            zip.finish().unwrap();
        }
        let plain = read_jar(&plain_path);
        assert!(plain.error.as_deref().unwrap().contains("No mod manifest"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quilt_hosts_fabric_mods_but_not_the_reverse() {
        assert!(ModLoaderKind::Fabric.runs_on("quilt"));
        assert!(!ModLoaderKind::Quilt.runs_on("fabric"));
        assert!(ModLoaderKind::Fabric.runs_on("fabric"));
    }
}
