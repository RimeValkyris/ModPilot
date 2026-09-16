//! Assembles the diagnostic report.
//!
//! [`build`] is deliberately a pure function over [`DiagnosticInputs`]: all
//! the I/O (database, filesystem, log reading, JAR scanning) happens in
//! `commands::diagnostics`, so every check here is testable against
//! hand-written inputs instead of needing a real server on disk.

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use super::logs::{LogIssue, LogLevel, REPEAT_THRESHOLD};
use crate::java::{parse_java_major, required_java_major};
use crate::models::DiskUsage;
use crate::mods::health::{CheckStatus, Severity};
use crate::mods::ModpackHealth;

/// Leave this much of the machine's RAM for everything that isn't the
/// server: the OS, ModpackPilot itself, and the JVM's own off-heap use
/// (metaspace, GC structures, direct buffers), which is substantial and is
/// *not* counted inside `-Xmx`.
const RAM_HEADROOM_FRACTION: f64 = 0.80;

/// Below this, a modded server will spend its life in garbage collection
/// even if it technically starts. Vanilla is happy with far less, so this
/// only applies when a loader is present.
const MODDED_MIN_SENSIBLE_RAM_MB: i64 = 3072;

/// Free space at which a world save can plausibly fail. A big modded world
/// plus a backup zip is measured in gigabytes.
const DISK_CRITICAL_FREE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const DISK_WARNING_FREE_BYTES: u64 = 10 * 1024 * 1024 * 1024;

/// The window recent crashes are counted within for the crash-history check.
const CRASH_WINDOW: Duration = Duration::hours(24);

/// How many crashes inside [`CRASH_WINDOW`] is a pattern rather than an
/// incident.
const CRASH_PATTERN_THRESHOLD: usize = 3;

/// A run shorter than this never reached a playable state.
///
/// Worth separating out, because it points somewhere quite different: a
/// server that dies during startup is failing on its configuration, its
/// mods or its Java, while one that dies after hours of play is failing on
/// load, memory or a specific in-game trigger. Generous enough to cover a
/// big modded pack's genuinely slow boot.
const STARTUP_CRASH_SECONDS: i64 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticStatus {
    Critical,
    Warning,
    Ok,
    /// The check could not run - the information it needs isn't available.
    /// Deliberately ordered after `Ok` so it never becomes the overall
    /// verdict.
    NotApplicable,
}

/// One check's result.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    /// Stable identifier, so the UI can link to a check without matching on
    /// its label text.
    pub id: &'static str,
    pub label: &'static str,
    pub status: DiagnosticStatus,
    /// One line, stating what was found.
    pub summary: String,
    /// What it means and what to do, in plain language. Empty when the
    /// summary already says everything.
    pub detail: String,
}

impl Diagnostic {
    fn new(
        id: &'static str,
        label: &'static str,
        status: DiagnosticStatus,
        summary: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            id,
            label,
            status,
            summary: summary.into(),
            detail: detail.into(),
        }
    }
}

/// One finished run of a server, as recorded in `launch_history`.
#[derive(Debug, Clone)]
pub struct LaunchRecord {
    pub started_at: DateTime<Utc>,
    pub stopped_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i64>,
    /// `running` | `stopped` | `crashed`.
    pub status: String,
}

/// Everything [`build`] needs, gathered by the command layer.
#[derive(Debug, Clone)]
pub struct DiagnosticInputs {
    pub minecraft_version: Option<String>,
    pub loader: String,
    /// The version string of the Java installation this instance is
    /// *explicitly* assigned, or `None` if none is.
    ///
    /// `None` is not a failure: `commands::server::resolve_java_path` picks
    /// a matching detected install automatically, which is the normal state
    /// for a freshly created or imported instance.
    pub java_version: Option<String>,
    /// Version strings of every detected Java installation, so this check
    /// can reproduce the same auto-selection the launcher performs instead
    /// of reporting the unassigned-but-launchable case as broken.
    pub detected_java_versions: Vec<String>,
    pub min_ram_mb: i64,
    pub max_ram_mb: i64,
    /// Total physical RAM, or `None` if it couldn't be read.
    pub system_ram_mb: Option<u64>,
    pub jvm_args: Vec<String>,
    pub disk: Option<DiskUsage>,
    /// `None` for an instance with no mods folder at all.
    pub modpack: Option<ModpackHealth>,
    /// `None` when there is no `latest.log` to read at all - a server that
    /// has never started. Distinct from `Some(vec![])`, which means a log
    /// was read and was clean.
    pub log_issues: Option<Vec<LogIssue>>,
    pub launch_history: Vec<LaunchRecord>,
}

/// The whole report.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticReport {
    /// The worst status among the checks that actually ran.
    pub overall: DiagnosticStatus,
    pub generated_at: DateTime<Utc>,
    pub diagnostics: Vec<Diagnostic>,
    /// Surfaced separately from the log check so the UI can show the actual
    /// error text rather than only a count.
    pub log_issues: Vec<LogIssue>,
}

/// Runs every check.
pub fn build(inputs: DiagnosticInputs) -> DiagnosticReport {
    let diagnostics = vec![
        check_java(&inputs),
        check_ram(&inputs),
        check_jvm_args(&inputs),
        check_disk(&inputs),
        check_modpack(&inputs),
        check_logs(&inputs),
        check_crashes(&inputs),
    ];

    // A check that couldn't run must not be able to set the overall
    // verdict, in either direction - it is neither good news nor bad.
    let overall = diagnostics
        .iter()
        .map(|d| d.status)
        .filter(|s| *s != DiagnosticStatus::NotApplicable)
        .min()
        .unwrap_or(DiagnosticStatus::NotApplicable);

    DiagnosticReport {
        overall,
        generated_at: Utc::now(),
        diagnostics,
        log_issues: inputs.log_issues.unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Checks
// ---------------------------------------------------------------------------

fn check_java(inputs: &DiagnosticInputs) -> Diagnostic {
    const ID: &str = "java";
    const LABEL: &str = "Java version";

    let required = required_java_major(inputs.minecraft_version.as_deref());
    let mc = inputs.minecraft_version.as_deref().unwrap_or("this version");

    // Nothing explicitly assigned is the normal state for a new or imported
    // instance, not a fault: `commands::server::resolve_java_path` picks a
    // matching detected install, and only refuses outright for Forge and
    // NeoForge with no match. This mirrors that decision rather than
    // reporting every fresh instance as unable to start.
    let Some(assigned) = inputs.java_version.as_deref() else {
        let Some(required) = required else {
            return Diagnostic::new(
                ID,
                LABEL,
                DiagnosticStatus::NotApplicable,
                "No Java selected, and no Minecraft version to infer one from",
                "This instance has no recorded Minecraft version, so the launcher will fall                  back to whatever \"java\" is on your PATH. Setting the Minecraft version, or                  picking a Java on the Java page, removes the guesswork.",
            );
        };

        let auto_selected = inputs
            .detected_java_versions
            .iter()
            .any(|v| parse_java_major(v) == Some(required));

        if auto_selected {
            return Diagnostic::new(
                ID,
                LABEL,
                DiagnosticStatus::Ok,
                format!("Java {required} will be selected automatically for Minecraft {mc}"),
                "No Java is pinned to this instance, so the launcher picks the detected                  installation matching the Minecraft version. Pin one on the Java page if you                  want a specific build.",
            );
        }

        let is_forge_family = matches!(inputs.loader.as_str(), "forge" | "neoforge");
        return Diagnostic::new(
            ID,
            LABEL,
            if is_forge_family {
                DiagnosticStatus::Critical
            } else {
                DiagnosticStatus::Warning
            },
            format!("No Java {required} installation was detected"),
            if is_forge_family {
                format!(
                    "Minecraft {mc} needs Java {required}, and none is installed. ModpackPilot                      refuses to start Forge and NeoForge servers on the wrong major version,                      because that hangs partway through mod loading rather than failing                      cleanly. Install Java {required} and rescan on the Java page."
                )
            } else {
                format!(
                    "Minecraft {mc} needs Java {required}, and none is installed. The server                      will be started with whatever \"java\" is on your PATH, which may be the                      wrong version. Install Java {required} and rescan on the Java page."
                )
            },
        );
    };

    let Some(required) = required else {
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::NotApplicable,
            format!("Using Java {assigned}"),
            "This instance has no recorded Minecraft version, so the Java version it needs              can't be determined. The Java in use is shown for reference only.",
        );
    };

    let Some(actual) = parse_java_major(assigned) else {
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::NotApplicable,
            format!("Java version \"{assigned}\" could not be read"),
            format!(
                "Minecraft {mc} needs Java {required}, but the selected installation's version                  string could not be parsed, so no comparison was made."
            ),
        );
    };

    if actual == required {
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::Ok,
            format!("Java {actual} matches Minecraft {mc}"),
            String::new(),
        );
    }

    if actual < required {
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::Critical,
            format!("Java {actual} is too old for Minecraft {mc}"),
            format!(
                "Minecraft {mc} requires Java {required}. The server will fail to start,                  usually with an \"UnsupportedClassVersionError\". Install Java {required}                  and select it for this instance."
            ),
        );
    }

    // Newer than required. Usually fine, and often deliberate - but not
    // always, so this is a warning rather than a pass.
    Diagnostic::new(
        ID,
        LABEL,
        DiagnosticStatus::Warning,
        format!("Java {actual} is newer than the Java {required} Minecraft {mc} targets"),
        format!(
            "This often works, and many packs run fine on a newer Java. Some older mods and              mixins do not, and fail in ways that look unrelated. If this server misbehaves              without an obvious cause, try Java {required}."
        ),
    )
}

fn check_ram(inputs: &DiagnosticInputs) -> Diagnostic {
    const ID: &str = "ram";
    const LABEL: &str = "RAM allocation";

    let max = inputs.max_ram_mb;
    let min = inputs.min_ram_mb;

    if max <= 0 {
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::Critical,
            "No maximum RAM is set",
            "This instance has no memory ceiling configured. Set one in its settings.",
        );
    }

    if min > max {
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::Critical,
            format!("Minimum RAM ({min} MB) is above the maximum ({max} MB)"),
            "The JVM refuses to start when -Xms is greater than -Xmx. Lower the minimum or \
             raise the maximum in this instance's settings.",
        );
    }

    let is_modded = !matches!(inputs.loader.as_str(), "vanilla" | "unknown");
    if is_modded && max < MODDED_MIN_SENSIBLE_RAM_MB {
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::Warning,
            format!("{max} MB is low for a modded server"),
            format!(
                "Modded servers generally need at least {} MB. Below that the server may start \
                 but spend most of its time in garbage collection, which shows up as low TPS \
                 rather than as an error.",
                MODDED_MIN_SENSIBLE_RAM_MB
            ),
        );
    }

    let Some(system_ram_mb) = inputs.system_ram_mb else {
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::NotApplicable,
            format!("{min}-{max} MB allocated"),
            "This machine's total memory could not be read, so the allocation could not be \
             compared against it.",
        );
    };

    let ceiling = (system_ram_mb as f64 * RAM_HEADROOM_FRACTION) as i64;
    if max > ceiling {
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::Warning,
            format!("{max} MB of this machine's {system_ram_mb} MB is allocated to one server"),
            format!(
                "Leaving under {}% of system memory free risks the OS swapping or killing the \
                 server under load. The JVM also uses memory beyond -Xmx for metaspace and GC \
                 structures, so the real footprint is higher than the number set here. \
                 Around {ceiling} MB is a safer ceiling on this machine.",
                ((1.0 - RAM_HEADROOM_FRACTION) * 100.0) as i64
            ),
        );
    }

    Diagnostic::new(
        ID,
        LABEL,
        DiagnosticStatus::Ok,
        format!("{min}-{max} MB of {system_ram_mb} MB"),
        String::new(),
    )
}

/// Catches the specific trap of setting memory in two places at once.
///
/// ModpackPilot writes `-Xms`/`-Xmx` from the instance's RAM settings. A
/// `-Xmx` typed into the custom JVM arguments as well is not additive -
/// one of them silently wins - so the operator ends up with a server whose
/// memory is not what either field says.
fn check_jvm_args(inputs: &DiagnosticInputs) -> Diagnostic {
    const ID: &str = "jvm-args";
    const LABEL: &str = "JVM arguments";

    let memory_flags: Vec<&String> = inputs
        .jvm_args
        .iter()
        .filter(|arg| {
            let lower = arg.to_ascii_lowercase();
            lower.starts_with("-xmx") || lower.starts_with("-xms")
        })
        .collect();

    if !memory_flags.is_empty() {
        let flags: Vec<&str> = memory_flags.iter().map(|s| s.as_str()).collect();
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::Warning,
            format!("Memory is set twice: {}", flags.join(", ")),
            "This instance's Min/Max RAM settings already produce -Xms and -Xmx. Setting them \
             again in the custom JVM arguments means one silently overrides the other, so the \
             server's real memory limit may not be the one shown in its settings. Remove the \
             flag from the custom arguments and use the RAM fields.",
        );
    }

    if inputs.jvm_args.is_empty() {
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::Ok,
            "No custom JVM arguments",
            String::new(),
        );
    }

    Diagnostic::new(
        ID,
        LABEL,
        DiagnosticStatus::Ok,
        format!("{} custom argument(s), no conflicts found", inputs.jvm_args.len()),
        String::new(),
    )
}

fn check_disk(inputs: &DiagnosticInputs) -> Diagnostic {
    const ID: &str = "disk";
    const LABEL: &str = "Disk space";

    let Some(disk) = inputs.disk.as_ref() else {
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::NotApplicable,
            "Disk usage unavailable",
            "The volume this instance lives on could not be read.",
        );
    };

    let free = disk.total_bytes.saturating_sub(disk.used_bytes);
    let free_gb = free as f64 / (1024.0 * 1024.0 * 1024.0);

    if free < DISK_CRITICAL_FREE_BYTES {
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::Critical,
            format!("Only {free_gb:.1} GB free on {}", disk.mount_point),
            "A Minecraft server writes its world continuously. Running out of space mid-save \
             is one of the few ways to genuinely corrupt a world. Free space before starting \
             this server again.",
        );
    }

    if free < DISK_WARNING_FREE_BYTES {
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::Warning,
            format!("{free_gb:.1} GB free on {}", disk.mount_point),
            "Enough to run, but a large modded world plus its backups can consume this \
             quickly. Consider pruning old backups or lowering the backup retention count.",
        );
    }

    Diagnostic::new(
        ID,
        LABEL,
        DiagnosticStatus::Ok,
        format!("{free_gb:.1} GB free on {}", disk.mount_point),
        String::new(),
    )
}

fn check_modpack(inputs: &DiagnosticInputs) -> Diagnostic {
    const ID: &str = "modpack";
    const LABEL: &str = "Modpack";

    let Some(health) = inputs.modpack.as_ref() else {
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::NotApplicable,
            "No mods folder",
            "This instance has no mods folder, so there is nothing to check. That is normal \
             for a vanilla server.",
        );
    };

    let critical = health
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Critical)
        .count();
    let warnings = health
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Warning)
        .count();
    let not_checked = health
        .checks
        .iter()
        .filter(|c| c.status == CheckStatus::NotChecked)
        .count();

    if critical > 0 {
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::Critical,
            format!(
                "{critical} critical problem(s) across {} mods",
                health.enabled_mods
            ),
            "Missing dependencies, duplicate mods, unreadable JARs and loader mismatches all \
             normally stop a server starting. Open the Mods tab for the full report and which \
             files are involved.",
        );
    }

    if warnings > 0 {
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::Warning,
            format!("{warnings} warning(s) across {} mods", health.enabled_mods),
            "Nothing that should stop the server starting. See the Mods tab for details.",
        );
    }

    let detail = if not_checked > 0 {
        format!(
            "{not_checked} check(s) could not run against these JARs - see the Mods tab for \
             which, and why."
        )
    } else {
        String::new()
    };

    Diagnostic::new(
        ID,
        LABEL,
        DiagnosticStatus::Ok,
        format!("{} mods, no problems found", health.enabled_mods),
        detail,
    )
}

fn check_logs(inputs: &DiagnosticInputs) -> Diagnostic {
    const ID: &str = "logs";
    const LABEL: &str = "Recent log errors";

    // No log file at all is not a clean log. A server that has never
    // started has nothing to read, and reporting that as "no errors" is the
    // same fabricated pass this module exists to avoid.
    let Some(log_issues) = inputs.log_issues.as_ref() else {
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::NotApplicable,
            "No server log to read",
            "This instance has no logs/latest.log yet, which means it has not been started              since it was set up. There is nothing to scan rather than nothing wrong.",
        );
    };

    if log_issues.is_empty() {
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::Ok,
            "No errors in the recent log",
            String::new(),
        );
    }

    let fatal = log_issues
        .iter()
        .filter(|i| i.level == LogLevel::Fatal)
        .count();
    let repeated: Vec<&LogIssue> = log_issues
        .iter()
        .filter(|i| i.count >= REPEAT_THRESHOLD)
        .collect();

    if fatal > 0 {
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::Critical,
            format!("{fatal} fatal error(s) in the recent log"),
            "A fatal error is the server reporting that it cannot continue. The messages are \
             listed below.",
        );
    }

    if let Some(worst) = repeated.first() {
        let attribution = match worst.mod_id.as_deref() {
            Some(mod_id) => format!(
                " The message names \"{mod_id}\", which is installed here - that mod is the \
                 most likely source."
            ),
            None => String::new(),
        };
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::Warning,
            format!(
                "One error repeated {} times ({} distinct problem(s) found)",
                worst.count,
                log_issues.len()
            ),
            format!(
                "An error repeating is usually the real fault rather than noise - it fires \
                 every tick, every chunk load, or every time a player does one thing.{attribution}"
            ),
        );
    }

    Diagnostic::new(
        ID,
        LABEL,
        DiagnosticStatus::Warning,
        format!("{} distinct error(s) in the recent log", log_issues.len()),
        "None of them repeating. A modded server logs some errors on a healthy boot, so \
         these may be harmless - they are listed below so you can judge.",
    )
}

fn check_crashes(inputs: &DiagnosticInputs) -> Diagnostic {
    const ID: &str = "crashes";
    const LABEL: &str = "Crash history";

    if inputs.launch_history.is_empty() {
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::NotApplicable,
            "No recorded launches",
            "This instance has not been started yet, so there is no history to check.",
        );
    }

    let cutoff = Utc::now() - CRASH_WINDOW;
    let recent_crashes: Vec<&LaunchRecord> = inputs
        .launch_history
        .iter()
        .filter(|r| r.status == "crashed" && r.started_at >= cutoff)
        .collect();

    if recent_crashes.is_empty() {
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::Ok,
            format!("No crashes in the last {} hours", CRASH_WINDOW.num_hours()),
            String::new(),
        );
    }

    // Exit codes are worth showing: a JVM that exited 1 failed differently
    // from one killed by the OS out-of-memory killer (137 on Linux).
    let codes: Vec<String> = recent_crashes
        .iter()
        .filter_map(|r| r.exit_code)
        .map(|c| c.to_string())
        .collect();
    let code_note = if codes.is_empty() {
        String::new()
    } else {
        format!(" Exit code(s): {}.", codes.join(", "))
    };

    // Runs that ended before the server could have finished starting.
    let startup_crashes = recent_crashes
        .iter()
        .filter(|r| {
            r.stopped_at
                .is_some_and(|stopped| (stopped - r.started_at).num_seconds() < STARTUP_CRASH_SECONDS)
        })
        .count();
    let startup_note = if startup_crashes == recent_crashes.len() {
        " Every one of them died within the first two minutes, so this is failing during           startup rather than under load - look at the modpack report, the Java version and           the first errors in the log, not at performance."
    } else {
        ""
    };

    if recent_crashes.len() >= CRASH_PATTERN_THRESHOLD {
        return Diagnostic::new(
            ID,
            LABEL,
            DiagnosticStatus::Critical,
            format!(
                "{} crashes in the last {} hours",
                recent_crashes.len(),
                CRASH_WINDOW.num_hours()
            ),
            format!(
                "Crashing repeatedly means something a restart cannot fix - check the log \
                 errors above and the modpack report before starting it again.{code_note}{startup_note}"
            ),
        );
    }

    Diagnostic::new(
        ID,
        LABEL,
        DiagnosticStatus::Warning,
        format!(
            "{} crash(es) in the last {} hours",
            recent_crashes.len(),
            CRASH_WINDOW.num_hours()
        ),
        format!("A one-off crash is not necessarily a pattern.{code_note}{startup_note}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> DiagnosticInputs {
        DiagnosticInputs {
            minecraft_version: Some("1.20.1".to_string()),
            loader: "forge".to_string(),
            java_version: Some("17.0.9".to_string()),
            detected_java_versions: vec!["17.0.9".to_string()],
            min_ram_mb: 4096,
            max_ram_mb: 8192,
            system_ram_mb: Some(32768),
            jvm_args: Vec::new(),
            disk: Some(DiskUsage {
                used_percent: 50.0,
                used_bytes: 500 * 1024 * 1024 * 1024,
                total_bytes: 1000 * 1024 * 1024 * 1024,
                mount_point: "C:\\".to_string(),
            }),
            modpack: None,
            log_issues: Some(Vec::new()),
            launch_history: Vec::new(),
        }
    }

    fn find<'a>(report: &'a DiagnosticReport, id: &str) -> &'a Diagnostic {
        report.diagnostics.iter().find(|d| d.id == id).unwrap()
    }

    #[test]
    fn a_healthy_instance_comes_back_ok() {
        let report = build(inputs());
        assert_eq!(report.overall, DiagnosticStatus::Ok);
        assert_eq!(find(&report, "java").status, DiagnosticStatus::Ok);
        assert_eq!(find(&report, "ram").status, DiagnosticStatus::Ok);
        assert_eq!(find(&report, "disk").status, DiagnosticStatus::Ok);
    }

    /// A freshly created or imported instance has no Java pinned, and the
    /// launcher auto-selects one. Reporting that as "cannot start" made
    /// every new instance show CRITICAL.
    #[test]
    fn an_unassigned_java_that_auto_selects_is_not_a_failure() {
        let mut i = inputs();
        i.java_version = None;
        i.detected_java_versions = vec!["17.0.9".to_string(), "21.0.2".to_string()];
        let report = build(i);
        assert_eq!(find(&report, "java").status, DiagnosticStatus::Ok);
        assert_eq!(report.overall, DiagnosticStatus::Ok);
    }

    /// ...but with nothing suitable installed it is a real problem, and a
    /// blocking one specifically for Forge/NeoForge, which refuse to launch
    /// on the wrong major rather than failing cleanly.
    #[test]
    fn an_unassigned_java_with_no_match_is_reported_per_loader() {
        let mut forge = inputs();
        forge.java_version = None;
        forge.detected_java_versions = vec!["8.0.392".to_string()];
        let report = build(forge);
        assert_eq!(find(&report, "java").status, DiagnosticStatus::Critical);

        let mut fabric = inputs();
        fabric.java_version = None;
        fabric.loader = "fabric".to_string();
        fabric.detected_java_versions = vec!["8.0.392".to_string()];
        let report = build(fabric);
        assert_eq!(find(&report, "java").status, DiagnosticStatus::Warning);
    }

    /// A server that has never run has no log, which is not a clean log.
    #[test]
    fn a_missing_log_is_not_checked_rather_than_passed() {
        let mut i = inputs();
        i.log_issues = None;
        let report = build(i);
        assert_eq!(find(&report, "logs").status, DiagnosticStatus::NotApplicable);
    }

    #[test]
    fn catches_java_too_old_for_the_minecraft_version() {
        let mut i = inputs();
        i.java_version = Some("8.0.392".to_string());
        let report = build(i);
        assert_eq!(find(&report, "java").status, DiagnosticStatus::Critical);
        assert_eq!(report.overall, DiagnosticStatus::Critical);
    }

    /// Newer Java usually works, so it must not be reported as a failure.
    #[test]
    fn newer_java_is_a_warning_not_a_failure() {
        let mut i = inputs();
        i.java_version = Some("21.0.2".to_string());
        assert_eq!(find(&build(i), "java").status, DiagnosticStatus::Warning);
    }

    #[test]
    fn an_unknown_minecraft_version_makes_the_java_check_inapplicable() {
        let mut i = inputs();
        i.minecraft_version = None;
        let report = build(i);
        assert_eq!(find(&report, "java").status, DiagnosticStatus::NotApplicable);
        // And a check that couldn't run must not drag the overall verdict.
        assert_eq!(report.overall, DiagnosticStatus::Ok);
    }

    #[test]
    fn catches_an_inverted_ram_range() {
        let mut i = inputs();
        i.min_ram_mb = 8192;
        i.max_ram_mb = 4096;
        assert_eq!(find(&build(i), "ram").status, DiagnosticStatus::Critical);
    }

    #[test]
    fn warns_when_the_allocation_leaves_the_machine_no_headroom() {
        let mut i = inputs();
        i.system_ram_mb = Some(8192);
        i.max_ram_mb = 8192;
        assert_eq!(find(&build(i), "ram").status, DiagnosticStatus::Warning);
    }

    /// Vanilla runs happily in far less memory than a modded pack, so the
    /// low-RAM warning must not fire for it.
    #[test]
    fn low_ram_is_only_a_warning_for_modded_servers() {
        let mut modded = inputs();
        modded.min_ram_mb = 1024;
        modded.max_ram_mb = 2048;
        let report = build(modded);
        assert_eq!(find(&report, "ram").status, DiagnosticStatus::Warning);

        let mut vanilla = inputs();
        vanilla.min_ram_mb = 1024;
        vanilla.max_ram_mb = 2048;
        vanilla.loader = "vanilla".to_string();
        let report = build(vanilla);
        assert_eq!(find(&report, "ram").status, DiagnosticStatus::Ok);
    }

    /// The specific trap: memory set in both the RAM fields and the custom
    /// JVM args, where one silently wins.
    #[test]
    fn catches_memory_flags_duplicated_in_jvm_args() {
        let mut i = inputs();
        i.jvm_args = vec!["-XX:+UseG1GC".to_string(), "-Xmx4G".to_string()];
        let report = build(i);
        let d = find(&report, "jvm-args");
        assert_eq!(d.status, DiagnosticStatus::Warning);
        assert!(d.summary.contains("-Xmx4G"));
    }

    #[test]
    fn ordinary_jvm_args_are_fine() {
        let mut i = inputs();
        i.jvm_args = vec!["-XX:+UseG1GC".to_string(), "-XX:MaxGCPauseMillis=50".to_string()];
        assert_eq!(find(&build(i), "jvm-args").status, DiagnosticStatus::Ok);
    }

    #[test]
    fn escalates_as_free_disk_space_falls() {
        let mut i = inputs();
        i.disk = Some(DiskUsage {
            used_percent: 99.0,
            used_bytes: 999 * 1024 * 1024 * 1024,
            total_bytes: 1000 * 1024 * 1024 * 1024,
            mount_point: "C:\\".to_string(),
        });
        assert_eq!(find(&build(i.clone()), "disk").status, DiagnosticStatus::Critical);

        i.disk = Some(DiskUsage {
            used_percent: 95.0,
            used_bytes: 995 * 1024 * 1024 * 1024,
            total_bytes: 1000 * 1024 * 1024 * 1024,
            mount_point: "C:\\".to_string(),
        });
        assert_eq!(find(&build(i), "disk").status, DiagnosticStatus::Warning);
    }

    #[test]
    fn reports_a_repeating_log_error_and_names_the_mod_when_known() {
        let mut i = inputs();
        i.log_issues = Some(vec![LogIssue {
            level: LogLevel::Error,
            example: "[t] [main/ERROR]: Failure in createaddition".to_string(),
            count: 412,
            mod_id: Some("createaddition".to_string()),
        }]);
        let report = build(i);
        let d = find(&report, "logs");
        assert_eq!(d.status, DiagnosticStatus::Warning);
        assert!(d.summary.contains("412"));
        assert!(d.detail.contains("createaddition"));
    }

    #[test]
    fn a_fatal_log_error_is_critical() {
        let mut i = inputs();
        i.log_issues = Some(vec![LogIssue {
            level: LogLevel::Fatal,
            example: "[t] [main/FATAL]: Cannot continue".to_string(),
            count: 1,
            mod_id: None,
        }]);
        assert_eq!(find(&build(i), "logs").status, DiagnosticStatus::Critical);
    }

    #[test]
    fn repeated_recent_crashes_are_critical_but_one_is_not() {
        let now = Utc::now();
        let crash = |hours_ago: i64| LaunchRecord {
            started_at: now - Duration::hours(hours_ago),
            stopped_at: Some(now - Duration::hours(hours_ago)),
            exit_code: Some(1),
            status: "crashed".to_string(),
        };

        let mut one = inputs();
        one.launch_history = vec![crash(2)];
        assert_eq!(find(&build(one), "crashes").status, DiagnosticStatus::Warning);

        let mut many = inputs();
        many.launch_history = vec![crash(1), crash(2), crash(3)];
        let report = build(many);
        let d = find(&report, "crashes");
        assert_eq!(d.status, DiagnosticStatus::Critical);
        assert!(d.detail.contains("Exit code"));
    }

    /// A crash seconds after launch points somewhere different from one
    /// hours in, so the report has to tell them apart.
    #[test]
    fn distinguishes_startup_crashes_from_crashes_under_load() {
        let now = Utc::now();

        let mut startup = inputs();
        startup.launch_history = (1..=3)
            .map(|h| LaunchRecord {
                started_at: now - Duration::hours(h),
                stopped_at: Some(now - Duration::hours(h) + Duration::seconds(20)),
                exit_code: Some(1),
                status: "crashed".to_string(),
            })
            .collect();
        let report = build(startup);
        assert!(find(&report, "crashes").detail.contains("during"));

        let mut under_load = inputs();
        under_load.launch_history = (1..=3)
            .map(|h| LaunchRecord {
                started_at: now - Duration::hours(h * 5),
                stopped_at: Some(now - Duration::hours(h * 5) + Duration::hours(4)),
                exit_code: Some(1),
                status: "crashed".to_string(),
            })
            .collect();
        let report = build(under_load);
        assert!(!find(&report, "crashes").detail.contains("during"));
    }

    /// A crash from last week is history, not a current problem.
    #[test]
    fn old_crashes_fall_out_of_the_window() {
        let mut i = inputs();
        i.launch_history = vec![LaunchRecord {
            started_at: Utc::now() - Duration::days(7),
            stopped_at: None,
            exit_code: Some(1),
            status: "crashed".to_string(),
        }];
        assert_eq!(find(&build(i), "crashes").status, DiagnosticStatus::Ok);
    }

    /// The overall verdict is the worst real finding, and checks that could
    /// not run are excluded from it entirely.
    #[test]
    fn overall_is_the_worst_status_that_actually_ran() {
        let mut i = inputs();
        i.minecraft_version = None; // makes the Java check NotApplicable
        i.modpack = None; // NotApplicable
        i.jvm_args = vec!["-Xmx4G".to_string()]; // Warning
        let report = build(i);
        assert_eq!(report.overall, DiagnosticStatus::Warning);
    }
}
