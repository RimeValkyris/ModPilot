/// Minecraft's official Java version per release, per Mojang's own launcher
/// requirements. Ordered newest-first; the first threshold the given
/// version meets or exceeds wins.
///
/// Mirrors `src/lib/javaRequirement.ts` on the frontend - kept in sync
/// deliberately rather than shared, since the frontend needs it to render
/// recommendations before a launch and the backend needs it to actually
/// pick the JVM.
const THRESHOLDS: &[((u32, u32, u32), u32)] = &[
    ((1, 20, 5), 21),
    ((1, 18, 0), 17),
    ((1, 17, 0), 16),
    ((0, 0, 0), 8),
];

fn parse_version_tuple(v: &str) -> Option<(u32, u32, u32)> {
    let mut parts = v.trim().split('.');
    let major: u32 = parts.next()?.trim().parse().ok()?;
    let minor: u32 = parts
        .next()
        .and_then(|p| p.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    let patch: u32 = parts
        .next()
        .and_then(|p| p.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    Some((major, minor, patch))
}

/// The Java major version a given Minecraft release targets, or `None` if
/// the version string can't be parsed (snapshots, unknown) - never guessed
/// with false confidence.
pub fn required_java_major(minecraft_version: Option<&str>) -> Option<u32> {
    let tuple = parse_version_tuple(minecraft_version?)?;
    THRESHOLDS
        .iter()
        .find(|(min, _)| tuple >= *min)
        .map(|(_, java)| *java)
}

/// Parses the major version out of a `JavaInstallation`'s normalized
/// version string ("21.0.2" -> 21, "8.0.392" -> 8).
pub fn parse_java_major(java_version: &str) -> Option<u32> {
    java_version.split('.').next()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_minecraft_versions_to_java() {
        assert_eq!(required_java_major(Some("1.20.1")), Some(17));
        assert_eq!(required_java_major(Some("1.20.6")), Some(21));
        assert_eq!(required_java_major(Some("1.21")), Some(21));
        assert_eq!(required_java_major(Some("1.17.1")), Some(16));
        assert_eq!(required_java_major(Some("1.12.2")), Some(8));
        assert_eq!(required_java_major(None), None);
        assert_eq!(required_java_major(Some("not-a-version")), None);
    }

    #[test]
    fn parses_java_majors() {
        assert_eq!(parse_java_major("21.0.2"), Some(21));
        assert_eq!(parse_java_major("8.0.392"), Some(8));
        assert_eq!(parse_java_major("17"), Some(17));
    }
}
