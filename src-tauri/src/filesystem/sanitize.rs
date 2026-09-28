/// Windows reserved device names - not valid as a file/directory name
/// regardless of extension.
const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "CONIN$",
    "CONOUT$",
];

/// Whether a single path segment names a Windows device rather than a file.
///
/// Windows ignores everything from the first dot and any trailing spaces
/// when matching these, so `nul.txt` and `CON .log` are the device too - an
/// exact-match check alone misses them.
pub fn is_reserved_windows_name(segment: &str) -> bool {
    let stem = segment.split('.').next().unwrap_or("").trim_end();
    RESERVED_NAMES.contains(&stem.to_ascii_uppercase().as_str())
}

/// Turns an arbitrary user-supplied instance name into a safe, single path
/// segment usable as a directory name on both Windows and Linux.
///
/// Strips characters invalid on either platform, collapses whitespace,
/// trims trailing dots/spaces (illegal on Windows), and falls back to
/// "instance" if nothing usable is left.
pub fn sanitize_dir_name(name: &str) -> String {
    let mut cleaned: String = name
        .trim()
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();

    while cleaned.ends_with('.') || cleaned.ends_with(' ') {
        cleaned.pop();
    }

    if cleaned.is_empty() || is_reserved_windows_name(&cleaned) {
        cleaned = "instance".to_string();
    }

    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_names_match_with_an_extension_or_trailing_space() {
        for name in ["CON", "nul", "nul.txt", "Com1.jar", "CON .log", "conout$"] {
            assert!(is_reserved_windows_name(name), "should be reserved: {name:?}");
        }
        for name in ["console", "null.txt", "config", "com10"] {
            assert!(!is_reserved_windows_name(name), "should be allowed: {name:?}");
        }
    }

    #[test]
    fn instance_names_never_become_a_device() {
        assert_eq!(sanitize_dir_name("nul.pack"), "instance");
        assert_eq!(sanitize_dir_name("My Pack."), "My Pack");
        assert_eq!(sanitize_dir_name("a/b"), "a_b");
    }
}
