/// Windows reserved device names - not valid as a file/directory name
/// regardless of extension.
const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

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

    if cleaned.is_empty() || RESERVED_NAMES.contains(&cleaned.to_uppercase().as_str()) {
        cleaned = "instance".to_string();
    }

    cleaned
}
