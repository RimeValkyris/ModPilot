//! Turns a folder of mod jars into a list of findings an operator can act on.
//!
//! Every check here is derived from what the jars themselves declare - no
//! network calls, no curated compatibility database. That keeps the feature
//! local-first and keeps its warnings honest: if a jar does not state
//! something, the corresponding check reports that it could not be run
//! rather than producing a verdict.
//!
//! Nothing in this module changes anything on disk. Acting on a finding is
//! always the operator's own decision, taken through the existing
//! toggle/delete commands.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use super::metadata::{ModEnvironment, ModMetadata};
use super::version::{maven_range_contains, Version};

/// Dependency ids that are satisfied by the platform rather than by a jar
/// in `mods/`.
///
/// Without this list every single mod would be reported as missing
/// `minecraft` and its loader, which is noise that would bury the real
/// findings. Note what is deliberately *absent*: `fabric` and `fabric-api`
/// are a real mod that really does have to be installed, and a pack missing
/// it is a genuine and very common failure.
const PLATFORM_PROVIDED: &[&str] = &[
    "minecraft",
    "java",
    "forge",
    "neoforge",
    "fabricloader",
    "quilt_loader",
    "quilt_base",
    "mcp",
    "fml",
];

/// How serious a finding is.
///
/// `Critical` is reserved for things that stop a server booting or that
/// have already gone wrong. A mod that is merely questionable is a
/// `Warning` - overstating severity trains people to ignore the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    Warning,
    Info,
}

/// What kind of problem a finding describes, so the UI can group and filter
/// without parsing the message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FindingKind {
    MissingDependency,
    DuplicateMod,
    InvalidJar,
    LoaderMismatch,
    MinecraftVersionMismatch,
    ClientOnlyMod,
    DisabledMod,
}

/// One actionable problem.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub kind: FindingKind,
    pub severity: Severity,
    /// A one-line statement of the problem.
    pub summary: String,
    /// Why it matters and what to do about it, in plain language.
    pub detail: String,
    /// The jars this finding concerns - what the UI highlights in the mod
    /// list.
    pub file_names: Vec<String>,
}

/// Whether a check ran at all, and what it concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CheckStatus {
    Ok,
    Warning,
    Critical,
    /// The information needed simply was not available - e.g. the instance
    /// has no recorded Minecraft version, or no jar declares a side.
    NotChecked,
}

/// One row of the summary table.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckSummary {
    pub label: String,
    /// The measured value, as text ("184", "1.20.1", "183/184").
    pub value: String,
    pub status: CheckStatus,
    /// Present when `status` is `NotChecked`, explaining what was missing.
    pub note: Option<String>,
}

/// The whole report.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModpackHealth {
    pub total_mods: usize,
    pub enabled_mods: usize,
    pub disabled_mods: usize,
    pub unreadable_mods: usize,
    pub checks: Vec<CheckSummary>,
    pub findings: Vec<Finding>,
    /// Every jar's parsed metadata, so the Mods tab can show what each one
    /// actually declares instead of just a filename.
    pub mods: Vec<ModMetadata>,
}

/// Runs every check against an already-parsed set of jars.
///
/// Split from the I/O so the whole analysis is testable against
/// hand-written metadata rather than needing real jars on disk.
///
/// Only *enabled* jars take part in the compatibility checks: a disabled
/// mod is not loaded, so it cannot be missing a dependency and cannot
/// satisfy one either.
pub fn analyze(
    mods: Vec<ModMetadata>,
    instance_loader: &str,
    minecraft_version: Option<&str>,
) -> ModpackHealth {
    let enabled: Vec<&ModMetadata> = mods.iter().filter(|m| m.enabled).collect();
    let unreadable: Vec<&ModMetadata> = enabled.iter().copied().filter(|m| m.error.is_some()).collect();
    let readable: Vec<&ModMetadata> = enabled.iter().copied().filter(|m| m.error.is_none()).collect();

    let mut findings = Vec::new();
    let mut checks = Vec::new();

    // --- invalid jars ------------------------------------------------------
    for m in &unreadable {
        findings.push(Finding {
            kind: FindingKind::InvalidJar,
            severity: Severity::Critical,
            summary: format!("{} is not a loadable mod", m.file_name),
            detail: format!(
                "{}. A file in mods/ that the loader cannot read will usually stop the server \
                 starting. If it is a library the pack put there on purpose, leave it; otherwise \
                 it is most likely a truncated download and should be re-downloaded.",
                m.error.as_deref().unwrap_or("Unreadable")
            ),
            file_names: vec![m.file_name.clone()],
        });
    }
    checks.push(CheckSummary {
        label: "Readable JARs".to_string(),
        value: format!("{}/{}", readable.len(), enabled.len()),
        status: if unreadable.is_empty() { CheckStatus::Ok } else { CheckStatus::Critical },
        note: None,
    });

    // --- duplicates --------------------------------------------------------
    let mut by_id: HashMap<&str, Vec<&ModMetadata>> = HashMap::new();
    for m in &readable {
        if let Some(id) = m.mod_id.as_deref() {
            by_id.entry(id).or_default().push(m);
        }
    }
    let mut duplicate_count = 0;
    // Sorted so the report is stable between runs rather than reordering
    // with HashMap iteration order.
    let mut duplicate_ids: Vec<&&str> = by_id.keys().filter(|id| by_id[**id].len() > 1).collect();
    duplicate_ids.sort();
    for id in duplicate_ids {
        let group = &by_id[*id];
        duplicate_count += 1;
        let versions: Vec<String> = group
            .iter()
            .map(|m| {
                format!(
                    "{} ({})",
                    m.file_name,
                    m.version.as_deref().unwrap_or("unknown version")
                )
            })
            .collect();
        findings.push(Finding {
            kind: FindingKind::DuplicateMod,
            severity: Severity::Critical,
            summary: format!("\"{id}\" is installed {} times", group.len()),
            detail: format!(
                "Two or more JARs declare the same mod id, which normally stops the server \
                 starting. Keep one and disable the rest: {}.",
                versions.join(", ")
            ),
            file_names: group.iter().map(|m| m.file_name.clone()).collect(),
        });
    }
    checks.push(CheckSummary {
        label: "Duplicates".to_string(),
        value: if duplicate_count == 0 {
            "None".to_string()
        } else {
            format!("{duplicate_count} mod id(s)")
        },
        status: if duplicate_count == 0 { CheckStatus::Ok } else { CheckStatus::Critical },
        note: None,
    });

    // --- dependencies ------------------------------------------------------
    let provided: HashSet<&str> = readable.iter().filter_map(|m| m.mod_id.as_deref()).collect();
    let platform: HashSet<&str> = PLATFORM_PROVIDED.iter().copied().collect();

    let mut satisfied_mods = 0;
    for m in &readable {
        let missing: Vec<&str> = m
            .dependencies
            .iter()
            .filter(|d| d.required)
            .map(|d| d.mod_id.as_str())
            .filter(|id| !provided.contains(id) && !platform.contains(id))
            .collect();

        if missing.is_empty() {
            satisfied_mods += 1;
            continue;
        }
        findings.push(Finding {
            kind: FindingKind::MissingDependency,
            severity: Severity::Critical,
            summary: format!("{} is missing {}", m.best_name(), plural_deps(&missing)),
            detail: format!(
                "{} requires {}, which no installed mod provides. The server will usually refuse \
                 to start until it is added. Optional dependencies are not counted here.",
                m.best_name(),
                missing.join(", ")
            ),
            file_names: vec![m.file_name.clone()],
        });
    }
    checks.push(CheckSummary {
        label: "Dependencies".to_string(),
        value: format!("{}/{}", satisfied_mods, readable.len()),
        status: if satisfied_mods == readable.len() { CheckStatus::Ok } else { CheckStatus::Critical },
        note: None,
    });

    // --- loader ------------------------------------------------------------
    // An undetected loader means the check cannot run, not that every JAR is
    // wrong. Without this, an instance imported from a folder that detection
    // couldn't classify reports one Critical finding *per mod* - hundreds of
    // them, all false. `report::check_ram` already treats "unknown" this way.
    let loader_known = !matches!(instance_loader, "unknown" | "");
    let mut loader_mismatches = Vec::new();
    if loader_known {
        for m in &readable {
            let Some(loader) = m.loader else { continue };
            if !loader.runs_on(instance_loader) {
                loader_mismatches.push((*m, loader));
            }
        }
    }
    for (m, loader) in &loader_mismatches {
        findings.push(Finding {
            kind: FindingKind::LoaderMismatch,
            severity: Severity::Critical,
            summary: format!("{} is a {} mod", m.best_name(), loader.label()),
            detail: format!(
                "This server runs {instance_loader}, but {} declares itself as a {} mod. It will                  not load, and on {instance_loader} it may stop the server starting.",
                m.file_name,
                loader.label()
            ),
            file_names: vec![m.file_name.clone()],
        });
    }
    checks.push(CheckSummary {
        label: "Loader".to_string(),
        value: if loader_known {
            instance_loader.to_string()
        } else {
            "Unknown".to_string()
        },
        status: match (loader_known, loader_mismatches.is_empty()) {
            (false, _) => CheckStatus::NotChecked,
            (true, true) => CheckStatus::Ok,
            (true, false) => CheckStatus::Critical,
        },
        note: (!loader_known).then(|| {
            "This instance's mod loader hasn't been detected, so per-JAR loader compatibility              could not be checked. Setting it in the instance's settings enables this check."
                .to_string()
        }),
    });

    // --- minecraft version -------------------------------------------------
    // Only Maven ranges are evaluated (Forge/NeoForge). Fabric writes semver
    // ranges, which this deliberately does not interpret - see
    // `version::maven_range_contains`.
    let parsed_mc = minecraft_version.and_then(Version::parse);
    let mut mc_mismatches = 0;
    let mut mc_checked = 0;
    if let Some(mc) = parsed_mc.as_ref() {
        for m in &readable {
            let Some(dep) = m.dependencies.iter().find(|d| d.mod_id == "minecraft") else {
                continue;
            };
            let Some(range) = dep.version_range.as_deref() else { continue };
            let Some(in_range) = maven_range_contains(range, mc) else {
                continue;
            };
            mc_checked += 1;
            if in_range {
                continue;
            }
            mc_mismatches += 1;
            findings.push(Finding {
                kind: FindingKind::MinecraftVersionMismatch,
                severity: Severity::Critical,
                summary: format!(
                    "{} does not support Minecraft {}",
                    m.best_name(),
                    minecraft_version.unwrap_or("?")
                ),
                detail: format!(
                    "{} declares it needs Minecraft {range}, but this server is {}. Install the \
                     build of this mod made for {}.",
                    m.file_name,
                    minecraft_version.unwrap_or("an unknown version"),
                    minecraft_version.unwrap_or("this version"),
                ),
                file_names: vec![m.file_name.clone()],
            });
        }
    }
    checks.push(CheckSummary {
        label: "Minecraft".to_string(),
        value: minecraft_version.unwrap_or("Unknown").to_string(),
        status: match (parsed_mc.is_some(), mc_checked, mc_mismatches) {
            (false, _, _) => CheckStatus::NotChecked,
            (true, 0, _) => CheckStatus::NotChecked,
            (true, _, 0) => CheckStatus::Ok,
            _ => CheckStatus::Critical,
        },
        note: match (parsed_mc.is_some(), mc_checked) {
            (false, _) => Some(
                "This instance has no recorded Minecraft version, so per-mod version ranges \
                 could not be checked."
                    .to_string(),
            ),
            (true, 0) => Some(
                "No installed mod declares a Minecraft version range in a format this check \
                 understands. Fabric mods use semantic-version ranges, which are not evaluated."
                    .to_string(),
            ),
            _ => None,
        },
    });

    // --- client-only mods --------------------------------------------------
    let client_only: Vec<&&ModMetadata> = readable
        .iter()
        .filter(|m| m.environment == ModEnvironment::Client)
        .collect();
    for m in &client_only {
        findings.push(Finding {
            kind: FindingKind::ClientOnlyMod,
            severity: Severity::Warning,
            summary: format!("{} is a client-only mod", m.best_name()),
            detail: format!(
                "{} declares itself client-side only, so it does nothing on a server. Most \
                 client mods are harmless here and simply idle; a few refuse to load and take \
                 the server down with them. Disabling it is safe.",
                m.file_name
            ),
            file_names: vec![m.file_name.clone()],
        });
    }
    // Forge and NeoForge have no side field at all, so on a Forge pack there
    // is nothing to check rather than nothing wrong - the report must say
    // which of the two it means.
    let any_side_declared = readable
        .iter()
        .any(|m| m.environment != ModEnvironment::Unknown);
    checks.push(CheckSummary {
        label: "Client-only mods".to_string(),
        value: if any_side_declared {
            client_only.len().to_string()
        } else {
            "Unknown".to_string()
        },
        status: match (any_side_declared, client_only.len()) {
            (false, _) => CheckStatus::NotChecked,
            (true, 0) => CheckStatus::Ok,
            _ => CheckStatus::Warning,
        },
        note: (!any_side_declared).then(|| {
            "Forge and NeoForge mods do not declare which side they run on, so client-only \
             mods cannot be detected from the JARs alone."
                .to_string()
        }),
    });

    // --- disabled ----------------------------------------------------------
    let disabled: Vec<&ModMetadata> = mods.iter().filter(|m| !m.enabled).collect();
    if !disabled.is_empty() {
        findings.push(Finding {
            kind: FindingKind::DisabledMod,
            severity: Severity::Info,
            summary: format!("{} mod(s) are disabled", disabled.len()),
            detail:
                "These JARs are renamed to .disabled and are not loaded. Listed so a mod \
                 disabled during troubleshooting and then forgotten does not stay a mystery."
                    .to_string(),
            file_names: disabled.iter().map(|m| m.file_name.clone()).collect(),
        });
    }

    // Most serious first, so the top of the list is always what matters
    // most. Ties keep the order the checks produced them in, which groups
    // findings of the same kind together.
    findings.sort_by_key(|f| f.severity);

    ModpackHealth {
        total_mods: mods.len(),
        enabled_mods: enabled.len(),
        disabled_mods: disabled.len(),
        unreadable_mods: unreadable.len(),
        checks,
        findings,
        mods,
    }
}

fn plural_deps(missing: &[&str]) -> String {
    match missing.len() {
        1 => format!("dependency \"{}\"", missing[0]),
        n => format!("{n} dependencies"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::metadata::{ModDependency, ModLoaderKind};

    fn mod_with(
        file_name: &str,
        mod_id: Option<&str>,
        loader: Option<ModLoaderKind>,
        deps: Vec<(&str, bool, Option<&str>)>,
    ) -> ModMetadata {
        ModMetadata {
            file_name: file_name.to_string(),
            display_name: None,
            mod_id: mod_id.map(str::to_string),
            version: Some("1.0.0".to_string()),
            loader,
            environment: ModEnvironment::Unknown,
            dependencies: deps
                .into_iter()
                .map(|(id, required, range)| ModDependency {
                    mod_id: id.to_string(),
                    required,
                    version_range: range.map(str::to_string),
                })
                .collect(),
            enabled: true,
            size_bytes: 1024,
            error: None,
        }
    }

    fn kinds(health: &ModpackHealth) -> Vec<FindingKind> {
        health.findings.iter().map(|f| f.kind).collect()
    }

    #[test]
    fn a_healthy_pack_produces_no_findings() {
        let mods = vec![
            mod_with("jei.jar", Some("jei"), Some(ModLoaderKind::Forge), vec![("minecraft", true, Some("[1.20.1,1.21)"))]),
            mod_with("lib.jar", Some("lib"), Some(ModLoaderKind::Forge), vec![]),
        ];
        let health = analyze(mods, "forge", Some("1.20.1"));
        assert!(health.findings.is_empty(), "{:?}", health.findings);
        // Everything that *can* be checked on a Forge pack passes. The side
        // check is the exception, and it reports "not checked" rather than a
        // pass, because Forge jars declare no side at all.
        for check in &health.checks {
            let expected = if check.label == "Client-only mods" {
                CheckStatus::NotChecked
            } else {
                CheckStatus::Ok
            };
            assert_eq!(check.status, expected, "check: {}", check.label);
        }
    }

    #[test]
    fn reports_a_missing_required_dependency_but_not_an_optional_one() {
        let mods = vec![
            mod_with("a.jar", Some("a"), Some(ModLoaderKind::Forge), vec![("missinglib", true, None)]),
            mod_with("b.jar", Some("b"), Some(ModLoaderKind::Forge), vec![("nicetohave", false, None)]),
        ];
        let health = analyze(mods, "forge", Some("1.20.1"));
        assert_eq!(kinds(&health), vec![FindingKind::MissingDependency]);
        assert!(health.findings[0].summary.contains("missinglib"));
        assert_eq!(health.findings[0].file_names, vec!["a.jar"]);
    }

    /// `minecraft` and the loader come from the platform, not from a jar -
    /// counting them as missing would bury every real finding.
    #[test]
    fn platform_dependencies_are_never_missing() {
        let mods = vec![mod_with(
            "a.jar",
            Some("a"),
            Some(ModLoaderKind::Forge),
            vec![("minecraft", true, None), ("forge", true, None), ("java", true, None)],
        )];
        assert!(analyze(mods, "forge", Some("1.20.1")).findings.is_empty());
    }

    /// Fabric API is a real mod that really must be installed.
    #[test]
    fn fabric_api_is_not_treated_as_platform_provided() {
        let mods = vec![mod_with(
            "a.jar",
            Some("a"),
            Some(ModLoaderKind::Fabric),
            vec![("fabric-api", true, None)],
        )];
        let health = analyze(mods, "fabric", Some("1.20.1"));
        assert_eq!(kinds(&health), vec![FindingKind::MissingDependency]);
    }

    #[test]
    fn reports_duplicate_mod_ids() {
        let mods = vec![
            mod_with("jei-15.0.jar", Some("jei"), Some(ModLoaderKind::Forge), vec![]),
            mod_with("jei-15.2.jar", Some("jei"), Some(ModLoaderKind::Forge), vec![]),
        ];
        let health = analyze(mods, "forge", Some("1.20.1"));
        assert_eq!(kinds(&health), vec![FindingKind::DuplicateMod]);
        assert_eq!(health.findings[0].file_names.len(), 2);
    }

    /// An instance whose loader was never detected must not report every
    /// single JAR as a mismatch - that is hundreds of false criticals.
    #[test]
    fn an_unknown_instance_loader_is_not_checked_rather_than_all_wrong() {
        let mods = vec![
            mod_with("a.jar", Some("a"), Some(ModLoaderKind::Forge), vec![]),
            mod_with("b.jar", Some("b"), Some(ModLoaderKind::Fabric), vec![]),
        ];
        let health = analyze(mods, "unknown", Some("1.20.1"));
        assert!(health.findings.is_empty(), "{:?}", health.findings);
        let check = health.checks.iter().find(|c| c.label == "Loader").unwrap();
        assert_eq!(check.status, CheckStatus::NotChecked);
        assert!(check.note.is_some());
    }

    #[test]
    fn reports_a_forge_mod_on_a_neoforge_server() {
        let mods = vec![mod_with("old.jar", Some("old"), Some(ModLoaderKind::Forge), vec![])];
        let health = analyze(mods, "neoforge", Some("1.21.1"));
        assert_eq!(kinds(&health), vec![FindingKind::LoaderMismatch]);
    }

    #[test]
    fn reports_a_minecraft_version_a_mod_does_not_support() {
        let mods = vec![mod_with(
            "old.jar",
            Some("old"),
            Some(ModLoaderKind::Forge),
            vec![("minecraft", true, Some("[1.19.2,1.20)"))],
        )];
        let health = analyze(mods, "forge", Some("1.20.1"));
        assert_eq!(kinds(&health), vec![FindingKind::MinecraftVersionMismatch]);
    }

    /// The instance has no recorded version, so the check must report that
    /// it did not run - not that everything is fine.
    #[test]
    fn an_unknown_minecraft_version_is_not_checked_rather_than_passed() {
        let mods = vec![mod_with(
            "a.jar",
            Some("a"),
            Some(ModLoaderKind::Forge),
            vec![("minecraft", true, Some("[1.19.2,1.20)"))],
        )];
        let health = analyze(mods, "forge", None);
        let check = health.checks.iter().find(|c| c.label == "Minecraft").unwrap();
        assert_eq!(check.status, CheckStatus::NotChecked);
        assert!(check.note.is_some());
        assert!(health.findings.is_empty());
    }

    /// A Forge-only pack declares no sides at all, so "0 client-only mods"
    /// would be a fabricated pass.
    #[test]
    fn client_only_is_not_checked_when_no_mod_declares_a_side() {
        let mods = vec![mod_with("a.jar", Some("a"), Some(ModLoaderKind::Forge), vec![])];
        let health = analyze(mods, "forge", Some("1.20.1"));
        let check = health.checks.iter().find(|c| c.label == "Client-only mods").unwrap();
        assert_eq!(check.status, CheckStatus::NotChecked);
        assert_eq!(check.value, "Unknown");
    }

    #[test]
    fn reports_a_client_only_fabric_mod() {
        let mut client = mod_with("sodium.jar", Some("sodium"), Some(ModLoaderKind::Fabric), vec![]);
        client.environment = ModEnvironment::Client;
        let health = analyze(vec![client], "fabric", Some("1.20.1"));
        assert_eq!(kinds(&health), vec![FindingKind::ClientOnlyMod]);
        assert_eq!(health.findings[0].severity, Severity::Warning);
    }

    /// A disabled jar is not loaded, so it neither needs its dependencies
    /// nor satisfies anyone else's.
    #[test]
    fn disabled_mods_take_no_part_in_compatibility_checks() {
        let mut disabled = mod_with("broken.jar.disabled", Some("broken"), Some(ModLoaderKind::Fabric), vec![("nothing", true, None)]);
        disabled.enabled = false;
        let mods = vec![
            mod_with("good.jar", Some("good"), Some(ModLoaderKind::Forge), vec![]),
            disabled,
        ];
        let health = analyze(mods, "forge", Some("1.20.1"));
        // Only the informational "some mods are disabled" note.
        assert_eq!(kinds(&health), vec![FindingKind::DisabledMod]);
        assert_eq!(health.disabled_mods, 1);
        assert_eq!(health.enabled_mods, 1);
    }

    #[test]
    fn an_unreadable_jar_is_reported_rather_than_skipped() {
        let mut broken = mod_with("truncated.jar", None, None, vec![]);
        broken.error = Some("Not a valid JAR archive".to_string());
        broken.mod_id = None;
        let health = analyze(vec![broken], "forge", Some("1.20.1"));
        assert_eq!(kinds(&health), vec![FindingKind::InvalidJar]);
        assert_eq!(health.unreadable_mods, 1);
    }

    #[test]
    fn findings_are_ordered_most_serious_first() {
        let mut client = mod_with("sodium.jar", Some("sodium"), Some(ModLoaderKind::Fabric), vec![]);
        client.environment = ModEnvironment::Client;
        let mut disabled = mod_with("off.jar.disabled", Some("off"), Some(ModLoaderKind::Fabric), vec![]);
        disabled.enabled = false;
        let missing = mod_with("needs.jar", Some("needs"), Some(ModLoaderKind::Fabric), vec![("gone", true, None)]);

        let health = analyze(vec![client, disabled, missing], "fabric", Some("1.20.1"));
        let severities: Vec<Severity> = health.findings.iter().map(|f| f.severity).collect();
        assert_eq!(
            severities,
            vec![Severity::Critical, Severity::Warning, Severity::Info]
        );
    }
}
