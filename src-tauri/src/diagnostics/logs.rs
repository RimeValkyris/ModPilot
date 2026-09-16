//! Finds the signal in a Minecraft server log.
//!
//! A modded server's `latest.log` is tens of thousands of lines, and the
//! handful that explain why it is broken are buried among them. The useful
//! reduction is not "show me the errors" - a big pack logs errors on every
//! healthy boot - but "show me *which* error is happening over and over",
//! because a line repeating hundreds of times is almost always the actual
//! fault.
//!
//! So this groups errors by their shape rather than their text: two lines
//! that differ only in a coordinate, a tick number or an entity UUID are
//! the same problem happening twice.

use std::collections::HashMap;

use serde::Serialize;

/// How much of the log tail to read.
///
/// The end of the file is where a crash lands, and reading a multi-hundred
/// megabyte log of a server that has been up for weeks would stall the
/// diagnostic for no benefit.
pub const MAX_SCAN_BYTES: u64 = 2 * 1024 * 1024;

/// How many distinct problems to report. Past this the list stops being
/// something a person reads and starts being another log to search.
const MAX_GROUPS: usize = 8;

/// How long a grouping key may get. Long stack-trace lines would otherwise
/// each become their own "group" keyed on text nobody compares.
const MAX_KEY_LEN: usize = 180;

/// How many times a problem must appear before it is called out as
/// repeating rather than merely present.
pub const REPEAT_THRESHOLD: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Fatal,
    Error,
    Warn,
}

/// One distinct problem found in the log, with how often it occurred.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogIssue {
    pub level: LogLevel,
    /// The first line that matched, shown verbatim so the operator sees the
    /// real message rather than the normalized key it was grouped by.
    pub example: String,
    pub count: usize,
    /// A mod id named in the message, when one of the instance's installed
    /// mods could be matched against it. Never guessed from arbitrary text.
    pub mod_id: Option<String>,
}

/// Scans log text for errors, grouping repeats.
///
/// `known_mod_ids` is used only to attribute an error to a mod when the
/// message actually names it - passing an empty slice simply means no
/// attribution is made.
pub fn scan(log: &str, known_mod_ids: &[String]) -> Vec<LogIssue> {
    struct Group {
        level: LogLevel,
        example: String,
        count: usize,
    }

    let mut groups: HashMap<String, Group> = HashMap::new();

    for line in log.lines() {
        let Some(level) = classify(line) else { continue };
        // Warnings are counted only when they repeat; a single warning on a
        // healthy boot is normal and is not worth a diagnostic finding.
        let key = normalize(line);
        if key.is_empty() {
            continue;
        }

        let entry = groups.entry(key).or_insert_with(|| Group {
            level,
            example: line.trim().to_string(),
            count: 0,
        });
        entry.count += 1;
        // The most serious level seen for a shape wins: a warning that
        // later escalates to an error is reported as the error.
        if level < entry.level {
            entry.level = level;
        }
    }

    let mut issues: Vec<LogIssue> = groups
        .into_values()
        .filter(|g| g.level != LogLevel::Warn || g.count >= REPEAT_THRESHOLD)
        .map(|g| LogIssue {
            level: g.level,
            mod_id: attribute(&g.example, known_mod_ids),
            example: g.example,
            count: g.count,
        })
        .collect();

    // Most serious first, then most frequent - the thing happening 400
    // times is what someone needs to see, not the one that happened once.
    issues.sort_by(|a, b| a.level.cmp(&b.level).then(b.count.cmp(&a.count)));
    issues.truncate(MAX_GROUPS);
    issues
}

/// Reads a line's severity out of the standard Minecraft log prefix,
/// `[HH:MM:SS] [Thread/LEVEL] [source]: message`.
///
/// Matching on the bracketed level rather than searching for the word
/// anywhere keeps a player typing "error" in chat, or a mod named
/// "FatalError", out of the report.
fn classify(line: &str) -> Option<LogLevel> {
    // The level sits inside the second bracket group, after a '/'.
    let level_section = line.split(']').find(|s| s.contains('/'))?;
    let level = level_section.rsplit('/').next()?.trim();

    match level.to_ascii_uppercase().as_str() {
        "FATAL" => Some(LogLevel::Fatal),
        "ERROR" | "SEVERE" => Some(LogLevel::Error),
        "WARN" | "WARNING" => Some(LogLevel::Warn),
        _ => None,
    }
}

/// Reduces a line to the shape two occurrences of the same problem share.
///
/// Drops the timestamp/thread prefix, then flattens anything that varies
/// between occurrences - numbers, hex ids, and path separators - so
/// "Entity 4821 at (123, 64, -900)" and "Entity 9134 at (7, 70, 12)" group
/// together instead of becoming two findings.
fn normalize(line: &str) -> String {
    // Everything up to the last "]: " is prefix.
    let body = line.rsplit_once("]: ").map(|(_, rest)| rest).unwrap_or(line).trim();

    let mut out = String::with_capacity(body.len().min(MAX_KEY_LEN));
    let mut last_was_placeholder = false;
    for c in body.chars() {
        if out.len() >= MAX_KEY_LEN {
            break;
        }
        if c.is_ascii_digit() {
            // Collapse whole runs of digits to one marker.
            if !last_was_placeholder {
                // A sign belongs to the number, not to the surrounding
                // text. Without this, "at (123, 64, -900)" and
                // "at (0, 0, 0)" normalize differently and the same fault
                // is reported twice - which is exactly the noise this
                // grouping exists to remove.
                if out.ends_with('-') {
                    out.pop();
                }
                out.push('#');
                last_was_placeholder = true;
            }
            continue;
        }
        last_was_placeholder = false;
        out.push(c.to_ascii_lowercase());
    }

    out.trim().to_string()
}

/// Attributes an error to a mod, but only when the message names it.
///
/// Matching is on a word-ish boundary so a mod called "core" does not claim
/// every line containing the word. Returns the longest match, so
/// "createaddition" is preferred over "create" when both appear.
fn attribute(line: &str, known_mod_ids: &[String]) -> Option<String> {
    let haystack = line.to_ascii_lowercase();

    known_mod_ids
        .iter()
        .filter(|id| id.len() >= 3)
        .filter(|id| {
            let id = id.to_ascii_lowercase();
            haystack.match_indices(&id).any(|(at, _)| {
                let before = haystack[..at].chars().next_back();
                let after = haystack[at + id.len()..].chars().next();
                let boundary = |c: Option<char>| {
                    c.is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
                };
                boundary(before) && boundary(after)
            })
        })
        .max_by_key(|id| id.len())
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_levels_out_of_the_log_prefix() {
        assert_eq!(
            classify("[21:03:11] [Server thread/ERROR]: Something broke"),
            Some(LogLevel::Error)
        );
        assert_eq!(
            classify("[21:03:11] [main/FATAL] [net.minecraft]: Fatal problem"),
            Some(LogLevel::Fatal)
        );
        assert_eq!(
            classify("[21:03:11] [Server thread/WARN]: Careful"),
            Some(LogLevel::Warn)
        );
        assert_eq!(classify("[21:03:11] [Server thread/INFO]: Done (12.3s)!"), None);
    }

    /// A player typing about an error must never become a diagnostic
    /// finding.
    #[test]
    fn chat_cannot_fake_a_log_level() {
        assert_eq!(
            classify("[21:03:11] [Server thread/INFO]: <Bob> ERROR something is broken"),
            None
        );
        assert_eq!(classify("just some text"), None);
        assert_eq!(classify(""), None);
    }

    /// The core of the feature: the same fault with different numbers in it
    /// is one problem, not two hundred.
    #[test]
    fn groups_the_same_error_with_varying_numbers() {
        let log = "\
[21:03:11] [Server thread/ERROR]: Ticking entity 4821 at (123, 64, -900)
[21:03:12] [Server thread/ERROR]: Ticking entity 9134 at (7, 70, 12)
[21:03:13] [Server thread/ERROR]: Ticking entity 1 at (0, 0, 0)
[21:03:14] [Server thread/INFO]: All good here";

        let issues = scan(log, &[]);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].count, 3);
        assert_eq!(issues[0].level, LogLevel::Error);
        // The example is the real line, not the normalized key.
        assert!(issues[0].example.contains("4821"));
    }

    /// A negative coordinate must not split a group. Minecraft logs are
    /// full of them, so getting this wrong would break grouping for most
    /// real world-related errors.
    #[test]
    fn a_sign_belongs_to_the_number_it_precedes() {
        assert_eq!(normalize("[t] [main/ERROR]: at (-900, 64, 12)"), normalize("[t] [main/ERROR]: at (5, 0, 0)"));
        // But a lone hyphen in prose is still meaningful text.
        assert_ne!(normalize("[t] [main/ERROR]: fast-path failed"), normalize("[t] [main/ERROR]: fastpath failed"));
    }

    #[test]
    fn orders_by_severity_then_frequency() {
        let log = "\
[t] [main/ERROR]: Common failure 1
[t] [main/ERROR]: Common failure 2
[t] [main/ERROR]: Common failure 3
[t] [main/FATAL]: The server is going down";

        let issues = scan(log, &[]);
        assert_eq!(issues[0].level, LogLevel::Fatal);
        assert_eq!(issues[1].level, LogLevel::Error);
        assert_eq!(issues[1].count, 3);
    }

    /// A lone warning on a healthy boot is normal; a warning repeating is
    /// not.
    #[test]
    fn a_single_warning_is_not_a_finding_but_a_repeating_one_is() {
        let once = scan("[t] [main/WARN]: Something minor", &[]);
        assert!(once.is_empty());

        let repeated = "\
[t] [main/WARN]: Chunk save took too long
[t] [main/WARN]: Chunk save took too long
[t] [main/WARN]: Chunk save took too long";
        let issues = scan(repeated, &[]);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].count, 3);
    }

    #[test]
    fn attributes_an_error_to_a_mod_it_names() {
        let mods = vec!["create".to_string(), "createaddition".to_string(), "jei".to_string()];
        let log = "[t] [main/ERROR]: Exception in com.simibubi.createaddition.Startup";
        let issues = scan(log, &mods);
        // The longer match wins, so the addon isn't misattributed to Create.
        assert_eq!(issues[0].mod_id.as_deref(), Some("createaddition"));
    }

    /// Attribution has to be evidence-based, or it sends people deleting
    /// innocent mods.
    #[test]
    fn does_not_attribute_on_a_partial_word_match() {
        let mods = vec!["ore".to_string(), "core".to_string()];
        let log = "[t] [main/ERROR]: Something went wrong in the scoreboard";
        assert_eq!(scan(log, &mods)[0].mod_id, None);
    }

    #[test]
    fn caps_how_many_problems_are_reported() {
        let log: String = (0..40)
            .map(|i| format!("[t] [main/ERROR]: Distinct failure kind {}\n", char::from(b'a' + (i % 26) as u8)))
            .collect();
        assert!(scan(&log, &[]).len() <= MAX_GROUPS);
    }

    #[test]
    fn an_empty_or_clean_log_produces_nothing() {
        assert!(scan("", &[]).is_empty());
        assert!(scan("[t] [main/INFO]: Done (1.2s)!\n[t] [main/INFO]: Ready", &[]).is_empty());
    }
}
