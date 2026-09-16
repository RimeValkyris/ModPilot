//! Version comparison for the mod metadata loaders declare about themselves.
//!
//! Deliberately narrow. Every function here returns `Option`, and `None`
//! means "this could not be decided", never "no". A modpack health check
//! that guesses is worse than one that stays quiet: telling somebody a mod
//! is incompatible when it is fine sends them deleting working mods, and
//! the whole value of the feature rests on its warnings being trustworthy.

/// A dotted version, reduced to the numeric components that can actually be
/// ordered.
///
/// Anything non-numeric (`-pre1`, `+build`, `rc2`) is dropped rather than
/// ranked, because there is no ordering that is right for every mod author's
/// scheme. Two versions that differ only in a suffix therefore compare
/// equal, which keeps a snapshot from being reported as out of range for a
/// constraint written against its release.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(Vec<u64>);

impl Version {
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }

        let mut parts = Vec::new();
        for segment in raw.split('.') {
            // Stop at the first segment that doesn't start with a digit:
            // "1.20.1-pre2" is (1, 20, 1), and "1.20.x" is (1, 20).
            let digits: String = segment.chars().take_while(char::is_ascii_digit).collect();
            if digits.is_empty() {
                break;
            }
            parts.push(digits.parse().ok()?);
        }

        (!parts.is_empty()).then_some(Version(parts))
    }

    /// Compares against `other`, treating a missing trailing component as
    /// zero so that "1.20" and "1.20.0" are the same version.
    fn cmp_padded(&self, other: &Version) -> std::cmp::Ordering {
        let len = self.0.len().max(other.0.len());
        for i in 0..len {
            let a = self.0.get(i).copied().unwrap_or(0);
            let b = other.0.get(i).copied().unwrap_or(0);
            match a.cmp(&b) {
                std::cmp::Ordering::Equal => continue,
                other => return other,
            }
        }
        std::cmp::Ordering::Equal
    }
}

/// Whether `version` satisfies a Maven version range, as Forge and NeoForge
/// write them in `mods.toml`.
///
/// Handles the forms that actually appear there:
///
/// - `[1.20.1,1.21)` - inclusive lower, exclusive upper
/// - `[1.20,)` / `(,1.21]` - open ended
/// - `[1.20.1]` - pinned to exactly one version
/// - `[1.18,1.19),[1.20,)` - a union; satisfying any member satisfies it
///
/// A bare version (`1.20.1`, no brackets) is Maven's *soft* requirement -
/// a preference, not a constraint - so it is reported as `None` rather than
/// as a violation.
///
/// Returns `None` for anything unparseable, which the caller reports as
/// "not checked" instead of as a problem.
pub fn maven_range_contains(range: &str, version: &Version) -> Option<bool> {
    let range = range.trim();
    if range.is_empty() {
        return None;
    }

    // A soft requirement carries no brackets at all.
    if !range.starts_with('[') && !range.starts_with('(') {
        return None;
    }

    let mut any_parsed = false;
    for clause in split_clauses(range)? {
        match clause_contains(&clause, version) {
            Some(true) => return Some(true),
            Some(false) => any_parsed = true,
            None => {}
        }
    }

    any_parsed.then_some(false)
}

/// Splits `[1.18,1.19),[1.20,)` into its bracketed clauses.
///
/// Splitting on commas naively would tear `[1.18,1.19)` in half, so this
/// only breaks at a comma that sits outside any bracket pair.
fn split_clauses(range: &str) -> Option<Vec<String>> {
    let mut clauses = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;

    for c in range.chars() {
        match c {
            '[' | '(' => {
                depth += 1;
                current.push(c);
            }
            ']' | ')' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
                current.push(c);
            }
            ',' if depth == 0 => {
                if !current.trim().is_empty() {
                    clauses.push(current.trim().to_string());
                }
                current.clear();
            }
            _ => current.push(c),
        }
    }
    if depth != 0 {
        return None;
    }
    if !current.trim().is_empty() {
        clauses.push(current.trim().to_string());
    }

    (!clauses.is_empty()).then_some(clauses)
}

fn clause_contains(clause: &str, version: &Version) -> Option<bool> {
    let lower_inclusive = clause.starts_with('[');
    let upper_inclusive = clause.ends_with(']');
    if !lower_inclusive && !clause.starts_with('(') {
        return None;
    }
    if !upper_inclusive && !clause.ends_with(')') {
        return None;
    }

    let inner = &clause[1..clause.len() - 1];

    // No comma means a pinned single version: `[1.20.1]`.
    let Some((low_raw, high_raw)) = inner.split_once(',') else {
        let pinned = Version::parse(inner)?;
        return Some(version.cmp_padded(&pinned) == std::cmp::Ordering::Equal);
    };

    if let Some(low) = Version::parse(low_raw) {
        let ord = version.cmp_padded(&low);
        let ok = if lower_inclusive {
            ord != std::cmp::Ordering::Less
        } else {
            ord == std::cmp::Ordering::Greater
        };
        if !ok {
            return Some(false);
        }
    } else if !low_raw.trim().is_empty() {
        return None; // present but unparseable - decline rather than guess
    }

    if let Some(high) = Version::parse(high_raw) {
        let ord = version.cmp_padded(&high);
        let ok = if upper_inclusive {
            ord != std::cmp::Ordering::Greater
        } else {
            ord == std::cmp::Ordering::Less
        };
        if !ok {
            return Some(false);
        }
    } else if !high_raw.trim().is_empty() {
        return None;
    }

    Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn parses_and_orders_dotted_versions() {
        assert_eq!(v("1.20.1"), Version(vec![1, 20, 1]));
        // A suffix is dropped, not ranked.
        assert_eq!(v("1.20.4-pre1"), Version(vec![1, 20, 4]));
        assert_eq!(v("1.21"), Version(vec![1, 21]));
        assert_eq!(Version::parse("not-a-version"), None);
        assert_eq!(Version::parse(""), None);
    }

    #[test]
    fn treats_missing_components_as_zero() {
        assert_eq!(v("1.20").cmp_padded(&v("1.20.0")), std::cmp::Ordering::Equal);
        assert_eq!(v("1.20.1").cmp_padded(&v("1.20")), std::cmp::Ordering::Greater);
    }

    #[test]
    fn evaluates_the_common_forge_ranges() {
        assert_eq!(maven_range_contains("[1.20.1,1.21)", &v("1.20.1")), Some(true));
        assert_eq!(maven_range_contains("[1.20.1,1.21)", &v("1.20.4")), Some(true));
        assert_eq!(maven_range_contains("[1.20.1,1.21)", &v("1.21")), Some(false));
        assert_eq!(maven_range_contains("[1.20.1,1.21)", &v("1.19.2")), Some(false));

        // Open ended.
        assert_eq!(maven_range_contains("[1.20,)", &v("1.21.4")), Some(true));
        assert_eq!(maven_range_contains("[1.20,)", &v("1.19")), Some(false));
        assert_eq!(maven_range_contains("(,1.21]", &v("1.21")), Some(true));
        assert_eq!(maven_range_contains("(,1.21]", &v("1.21.1")), Some(false));

        // Pinned.
        assert_eq!(maven_range_contains("[1.20.1]", &v("1.20.1")), Some(true));
        assert_eq!(maven_range_contains("[1.20.1]", &v("1.20.2")), Some(false));
    }

    #[test]
    fn a_union_is_satisfied_by_any_member() {
        let range = "[1.18,1.19),[1.20,1.21)";
        assert_eq!(maven_range_contains(range, &v("1.18.2")), Some(true));
        assert_eq!(maven_range_contains(range, &v("1.20.1")), Some(true));
        // Falls in the gap between the two clauses.
        assert_eq!(maven_range_contains(range, &v("1.19.2")), Some(false));
    }

    /// Everything undecidable must come back as `None`, so the health check
    /// reports "not checked" rather than inventing an incompatibility.
    #[test]
    fn declines_rather_than_guesses() {
        // Maven's soft requirement: a preference, not a constraint.
        assert_eq!(maven_range_contains("1.20.1", &v("1.19.2")), None);
        assert_eq!(maven_range_contains("", &v("1.20.1")), None);
        assert_eq!(maven_range_contains("[1.20.1", &v("1.20.1")), None);
        assert_eq!(maven_range_contains("[nonsense,)", &v("1.20.1")), None);
        // Fabric writes semver ranges, which are not Maven ranges.
        assert_eq!(maven_range_contains(">=1.20.1", &v("1.20.1")), None);
    }
}
