use std::collections::HashMap;
use std::path::Path;

use tauri::State;

use super::instance::fetch_instance;
use crate::AppState;

/// The only keys the Configuration tab's form edits. Restricting to a known
/// list (rather than accepting arbitrary key/value pairs from the frontend)
/// means a bug or bad input can't rewrite something else in the file.
const EDITABLE_KEYS: &[&str] = &[
    "motd",
    "max-players",
    "difficulty",
    "gamemode",
    "pvp",
    "online-mode",
    "white-list",
    "hardcore",
    "view-distance",
    "spawn-protection",
];

fn is_editable(key: &str) -> bool {
    EDITABLE_KEYS.contains(&key)
}

/// Minecraft accepts `difficulty` and `gamemode` as either a word or its
/// numeric alias (server.properties itself, and some server versions/mods,
/// can write either form) - normalized to the word form on read so the
/// Configuration tab's dropdowns always have a value they actually know,
/// instead of falling back to showing the raw number when the file has
/// e.g. `difficulty=3` instead of `difficulty=hard`. Passed through
/// unchanged if it's already a word, or some other value entirely.
fn normalize_editable_value(key: &str, value: &str) -> String {
    let alias = match (key, value) {
        ("difficulty", "0") => Some("peaceful"),
        ("difficulty", "1") => Some("easy"),
        ("difficulty", "2") => Some("normal"),
        ("difficulty", "3") => Some("hard"),
        ("gamemode", "0") => Some("survival"),
        ("gamemode", "1") => Some("creative"),
        ("gamemode", "2") => Some("adventure"),
        ("gamemode", "3") => Some("spectator"),
        _ => None,
    };
    alias.map(str::to_string).unwrap_or_else(|| value.to_string())
}

/// Rejects a value that would break out of its own `key=value` line.
///
/// `server.properties` is line-oriented, so an embedded newline in any
/// field (a pasted multi-line MOTD is the realistic way this happens)
/// would silently append whatever follows it as a *separate property* - a
/// MOTD containing a line break followed by `online-mode=false` would
/// quietly disable authentication. Trailing line breaks are trimmed rather
/// than rejected, since a stray newline on the end of a paste is harmless
/// and annoying to error on; only an interior break is a real problem.
///
/// Minecraft has no way to express a literal newline here anyway (it uses
/// a two-character backslash-n escape), so nothing legitimate is lost.
///
/// A trailing backslash is the same attack without a line break: Java's
/// properties format treats an odd run of `\` at the end of a line as a
/// continuation, so `motd=hi\` would swallow the *next* line of the file
/// into the MOTD - silently dropping a `white-list=true` that followed it.
fn validate_property_value(key: &str, value: &str) -> Result<String, String> {
    let trimmed = value.trim_end_matches(['\r', '\n']);
    if trimmed.contains('\n') || trimmed.contains('\r') {
        return Err(format!(
            "\"{key}\" can't contain a line break - server.properties stores one setting per line."
        ));
    }
    let trailing_backslashes = trimmed.chars().rev().take_while(|&c| c == '\\').count();
    if trailing_backslashes % 2 == 1 {
        return Err(format!(
            "\"{key}\" can't end with a single backslash - server.properties would join it to the next line."
        ));
    }
    Ok(trimmed.to_string())
}

/// How a `server.properties` file was encoded on disk, so a rewrite can
/// keep it that way.
///
/// Older Minecraft versions write the file as ISO-8859-1 (Java's
/// `Properties.store` default), so a MOTD with a `§` color code is not valid
/// UTF-8. Reading such a file as UTF-8 fails outright - which used to make
/// the Configuration tab error, the world folder fall back to `world`, and a
/// save replace the entire file with just the edited keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PropertiesEncoding {
    Utf8,
    Latin1,
}

/// Decodes `server.properties` bytes as UTF-8, falling back to ISO-8859-1 -
/// which maps every byte to a char, so this never fails.
pub(crate) fn decode_properties(bytes: &[u8]) -> (String, PropertiesEncoding) {
    match std::str::from_utf8(bytes) {
        Ok(text) => (text.to_string(), PropertiesEncoding::Utf8),
        Err(_) => (
            bytes.iter().map(|&b| char::from(b)).collect(),
            PropertiesEncoding::Latin1,
        ),
    }
}

/// The inverse of [`decode_properties`]. A character ISO-8859-1 can't hold
/// is written as a `\uXXXX` escape, which Java's properties reader decodes.
fn encode_properties(text: &str, encoding: PropertiesEncoding) -> Vec<u8> {
    match encoding {
        PropertiesEncoding::Utf8 => text.as_bytes().to_vec(),
        PropertiesEncoding::Latin1 => {
            let mut out = Vec::with_capacity(text.len());
            for c in text.chars() {
                match u8::try_from(u32::from(c)) {
                    Ok(byte) => out.push(byte),
                    Err(_) => {
                        for unit in c.encode_utf16(&mut [0; 2]) {
                            out.extend_from_slice(format!("\\u{unit:04X}").as_bytes());
                        }
                    }
                }
            }
            out
        }
    }
}

/// Reads and decodes a `server.properties` file. `Ok(None)` when it doesn't
/// exist yet.
pub(crate) async fn read_properties_file(
    path: &Path,
) -> std::io::Result<Option<(String, PropertiesEncoding)>> {
    match tokio::fs::read(path).await {
        Ok(bytes) => Ok(Some(decode_properties(&bytes))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Reads just the editable keys' current values out of `server.properties`.
/// A key that isn't present in the file yet (e.g. before the server has
/// ever started once to generate its defaults) is simply absent from the map.
#[tauri::command]
pub async fn read_server_properties(
    state: State<'_, AppState>,
    id: String,
) -> Result<HashMap<String, String>, String> {
    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let path = Path::new(&instance.server_directory)
        .join("server")
        .join("server.properties");

    let contents = match read_properties_file(&path).await {
        Ok(Some((text, _))) => text,
        Ok(None) => return Ok(HashMap::new()),
        Err(e) => return Err(format!("Failed to read server.properties: {e}")),
    };

    let mut values = HashMap::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            if is_editable(key) {
                values.insert(key.to_string(), normalize_editable_value(key, value.trim()));
            }
        }
    }

    Ok(values)
}

/// Updates only the given keys in `server.properties`, leaving every other
/// line (comments, ordering, keys this form doesn't know about) untouched.
/// A key that doesn't already exist in the file is appended; the file is
/// created if it doesn't exist yet.
///
/// Like most server.properties edits, most of these only take effect on
/// the next start - this doesn't attempt to push a live `/reload` or
/// restart anything.
#[tauri::command]
pub async fn write_server_properties(
    state: State<'_, AppState>,
    id: String,
    updates: HashMap<String, String>,
) -> Result<(), String> {
    let mut updates = updates;
    for key in updates.keys() {
        if !is_editable(key) {
            return Err(format!("\"{key}\" is not an editable setting"));
        }
    }
    // Validate every value before touching the file, so a bad one fails the
    // whole write rather than leaving server.properties half-updated.
    for (key, value) in updates.iter_mut() {
        *value = validate_property_value(key, value)?;
    }

    let instance = fetch_instance(&state, &id)
        .await?
        .ok_or_else(|| "Instance not found".to_string())?;

    let server_dir = Path::new(&instance.server_directory).join("server");
    merge_properties(&server_dir, updates).await
}

/// Merges `updates` into `<server_dir>/server.properties`, leaving every
/// other line (comments, ordering, keys this app doesn't know about)
/// exactly as it was. Creates the file if it doesn't exist yet.
///
/// Split out from the command above so the importer can apply defaults to a
/// freshly-imported pack without routing through the Tauri command layer.
pub(crate) async fn merge_properties(
    server_dir: &Path,
    updates: HashMap<String, String>,
) -> Result<(), String> {
    let path = server_dir.join("server.properties");

    // Only a missing file starts from empty. Any other read failure must
    // stop the write: carrying on would replace every setting the file
    // holds (port, seed, world name...) with just the handful being edited.
    let (existing, encoding) = read_properties_file(&path)
        .await
        .map_err(|e| format!("Failed to read server.properties: {e}"))?
        .unwrap_or((String::new(), PropertiesEncoding::Utf8));
    let mut remaining = updates;
    let mut lines: Vec<String> = Vec::new();

    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            lines.push(line.to_string());
            continue;
        }
        let matched = trimmed
            .split_once('=')
            .and_then(|(key, _)| remaining.remove(key.trim()).map(|value| (key.trim().to_string(), value)));
        match matched {
            Some((key, value)) => lines.push(format!("{key}={value}")),
            None => lines.push(line.to_string()),
        }
    }

    // Anything not already present in the file gets appended.
    for (key, value) in remaining {
        lines.push(format!("{key}={value}"));
    }

    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    tokio::fs::write(&path, encode_properties(&(lines.join("\n") + "\n"), encoding))
        .await
        .map_err(|e| format!("Failed to write server.properties: {e}"))
}

/// Settings ModpackPilot forces on a freshly-imported pack.
///
/// A lot of distributed server packs ship with the whitelist already on,
/// which silently locks everyone out on first boot and is confusing to
/// diagnose. These are applied once, at import - never on every start, so
/// an operator who deliberately turns the whitelist back on keeps it.
pub(crate) async fn apply_import_defaults(server_dir: &Path) -> Result<(), String> {
    let defaults = HashMap::from([
        ("white-list".to_string(), "false".to_string()),
        ("difficulty".to_string(), "normal".to_string()),
    ]);
    merge_properties(server_dir, defaults).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_numeric_aliases() {
        assert_eq!(normalize_editable_value("difficulty", "3"), "hard");
        assert_eq!(normalize_editable_value("gamemode", "0"), "survival");
        // Already a word, or an unrelated key - passed through untouched.
        assert_eq!(normalize_editable_value("difficulty", "peaceful"), "peaceful");
        assert_eq!(normalize_editable_value("max-players", "3"), "3");
    }

    #[test]
    fn rejects_line_breaks_that_would_inject_a_property() {
        assert!(validate_property_value("motd", "Hi\nonline-mode=false").is_err());
        assert!(validate_property_value("motd", "Hi\ronline-mode=false").is_err());
        // A trailing newline from a paste is trimmed, not rejected.
        assert_eq!(validate_property_value("motd", "Hello\n").unwrap(), "Hello");
        assert_eq!(validate_property_value("motd", "Hello").unwrap(), "Hello");
    }

    #[test]
    fn rejects_a_trailing_backslash_that_would_swallow_the_next_line() {
        assert!(validate_property_value("motd", "Hi\\").is_err());
        assert!(validate_property_value("motd", "Hi\\\\\\").is_err());
        // An escaped backslash is a literal one, not a continuation.
        assert!(validate_property_value("motd", "Hi\\\\").is_ok());
        assert!(validate_property_value("motd", "\\u00A7aGreen").is_ok());
    }

    #[test]
    fn latin1_files_round_trip_byte_for_byte() {
        // `§` as the single ISO-8859-1 byte 0xA7, as older servers write it.
        let original = b"motd=\xA7aHello\nlevel-name=Welt\n";
        let (text, encoding) = decode_properties(original);
        assert_eq!(encoding, PropertiesEncoding::Latin1);
        assert!(text.contains("level-name=Welt"));
        assert_eq!(encode_properties(&text, encoding), original);
        // A char outside Latin-1 becomes an escape Java reads back.
        assert_eq!(encode_properties("a\u{2603}", encoding), b"a\\u2603");
    }

    /// The data-loss bug: a non-UTF-8 file used to read as empty, so saving
    /// one setting wiped every other line.
    #[tokio::test]
    async fn merging_into_a_latin1_file_keeps_every_other_setting() {
        let dir = std::env::temp_dir().join(format!("mpp-props-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("server.properties"),
            b"motd=\xA7aHello\nserver-port=25570\nlevel-name=survival\n",
        )
        .unwrap();

        merge_properties(&dir, HashMap::from([("pvp".to_string(), "false".to_string())]))
            .await
            .unwrap();

        let written = std::fs::read(dir.join("server.properties")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            written,
            b"motd=\xA7aHello\nserver-port=25570\nlevel-name=survival\npvp=false\n"
        );
    }
}
