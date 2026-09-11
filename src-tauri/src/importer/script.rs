//! Reads a server pack's own start script (`run.bat`, `start.sh`, ...) to
//! work out what it actually launches.
//!
//! Plenty of modpack server packs ship *only* a start script - no
//! recognizable server jar at the root, and no argfile under any path
//! `detect::find_loader_argfile` knows to look in (older Forge layouts,
//! relocated `libraries/` folders, Fabric packs whose launch jar sits
//! under a subfolder). Without this, those packs import as "no server JAR
//! detected" and refuse to start, even though the answer was written down
//! in plain text right next to the mods folder.
//!
//! This is deliberately a *parser*, not an interpreter: it extracts the
//! `-jar <path>` or `@<argfile>` the script would hand to Java, so the
//! server can still run as a direct child process with piped I/O (see
//! `server::process::spawn_server_process`). Running the script itself is
//! the last-resort fallback, not the goal.

use std::collections::HashMap;

/// What a start script turned out to launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScriptLaunch {
    /// Path (relative to the server root, forward slashes) of either the
    /// jar or the `@`-argfile the script passes to Java.
    pub target: String,
    /// `true` when `target` is a Forge/NeoForge `@`-argfile.
    pub is_argfile: bool,
}

/// Pulls the Java invocation out of a start script's text.
///
/// Returns `None` whenever the script does something this can't resolve
/// with certainty - unexpanded variables, a nested script call, a
/// downloader/installer wrapper. Guessing wrong here is worse than not
/// guessing: the caller falls back to launching the script itself, which
/// at least does whatever the pack author intended.
pub(crate) fn parse_start_script(contents: &str) -> Option<ScriptLaunch> {
    let mut vars: HashMap<String, String> = HashMap::new();

    for raw_line in contents.lines() {
        let line = strip_comment(raw_line.trim());
        if line.is_empty() {
            continue;
        }

        if let Some((name, value)) = parse_assignment(line) {
            vars.insert(name, value);
            continue;
        }

        let tokens = tokenize(line);
        if !invokes_java(&tokens) {
            continue;
        }
        if let Some(launch) = extract_target(&tokens, &vars) {
            return Some(launch);
        }
    }

    None
}

/// Strips `rem`/`::`/`#` comments. Only whole-line comments are handled: a
/// `#` mid-line is far more likely to be part of a path or a Java property
/// than the start of a comment.
fn strip_comment(line: &str) -> &str {
    let unprefixed = line.strip_prefix('@').unwrap_or(line);
    let lower = unprefixed.to_lowercase();
    if lower.starts_with("rem ") || lower == "rem" || lower.starts_with("::") || lower.starts_with('#') {
        return "";
    }
    unprefixed
}

/// Recognizes `set NAME=value` (batch) and `NAME=value` (sh), the two ways
/// a start script names its jar before using it. Values are stored with
/// surrounding quotes removed so they can be substituted verbatim.
fn parse_assignment(line: &str) -> Option<(String, String)> {
    let body = match line.strip_prefix("set ").or_else(|| line.strip_prefix("SET ")) {
        Some(rest) => rest.trim(),
        None => line.strip_prefix("export ").map(str::trim).unwrap_or(line),
    };
    let (name, value) = body.split_once('=')?;
    let name = name.trim().trim_matches('"');
    // Guards against treating a command that merely contains an `=` (a
    // `-Dsome.prop=value` JVM flag, say) as an assignment.
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    Some((name.to_string(), unquote(value.trim()).to_string()))
}

/// Splits a command line on whitespace, keeping double-quoted runs
/// together. Quotes are dropped - every consumer here wants the path, not
/// the shell's spelling of it.
fn tokenize(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut has_content = false;

    for ch in line.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                has_content = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if has_content {
                    tokens.push(std::mem::take(&mut current));
                    has_content = false;
                }
            }
            c => {
                current.push(c);
                has_content = true;
            }
        }
    }
    if has_content {
        tokens.push(current);
    }
    tokens
}

/// True when the line runs Java rather than, say, echoing a message or
/// pausing. Covers the usual spellings: a bare `java`, an absolute JDK
/// path, and `%JAVA%` / `$JAVA_HOME/bin/java` style indirection.
fn invokes_java(tokens: &[String]) -> bool {
    let Some(first) = tokens.first() else {
        return false;
    };
    // `call java ...`, `start java ...`, `exec java ...` - skip the
    // wrapper and judge the command it wraps.
    let first = match first.to_lowercase().as_str() {
        "call" | "start" | "exec" => match tokens.get(1) {
            Some(next) => next,
            None => return false,
        },
        _ => first,
    };

    let lower = first.to_lowercase();
    let basename = lower.rsplit(['/', '\\']).next().unwrap_or(&lower);
    let stem = basename.strip_suffix(".exe").unwrap_or(basename);
    stem == "java" || stem == "javaw" || (lower.contains("java") && (lower.contains('%') || lower.contains('$')))
}

/// Walks the arguments looking for whatever identifies the server: an
/// `@`-argfile, or the jar after `-jar`.
fn extract_target(tokens: &[String], vars: &HashMap<String, String>) -> Option<ScriptLaunch> {
    let mut iter = tokens.iter().skip(1);
    while let Some(token) = iter.next() {
        let token = expand(token, vars)?;

        if let Some(argfile) = token.strip_prefix('@') {
            // `@user_jvm_args.txt` holds the RAM flags, not the server -
            // ModpackPilot supplies its own and passes that file itself
            // (see `spawn_server_process`'s "argfile" mode).
            if argfile.eq_ignore_ascii_case("user_jvm_args.txt") {
                continue;
            }
            let path = normalize(argfile);
            if path.is_empty() {
                continue;
            }
            return Some(ScriptLaunch { target: path, is_argfile: true });
        }

        if token.eq_ignore_ascii_case("-jar") {
            let jar = expand(iter.next()?, vars)?;
            let path = normalize(&jar);
            if !path.to_lowercase().ends_with(".jar") {
                return None;
            }
            return Some(ScriptLaunch { target: path, is_argfile: false });
        }
    }
    None
}

/// Substitutes `%NAME%` / `$NAME` / `${NAME}` from assignments seen
/// earlier in the script. Returns `None` if anything is left unresolved -
/// a half-expanded path is a path that won't exist on disk.
fn expand(token: &str, vars: &HashMap<String, String>) -> Option<String> {
    // `%*` (batch) and `"$@"` (sh) forward the script's own arguments;
    // there are none, so they're simply not part of the launch.
    if token == "%*" || token == "$@" || token == "$*" {
        return Some(String::new());
    }

    let mut out = token.to_string();
    for (name, value) in vars {
        out = out
            .replace(&format!("%{name}%"), value)
            .replace(&format!("${{{name}}}"), value)
            .replace(&format!("${name}"), value);
    }
    if out.contains('%') || out.contains('$') {
        return None;
    }
    Some(out)
}

/// Normalizes a path the way the rest of detection spells them: forward
/// slashes, no `./` prefix.
fn normalize(path: &str) -> String {
    let path = unquote(path).replace('\\', "/");
    path.strip_prefix("./").unwrap_or(&path).to_string()
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argfile(target: &str) -> Option<ScriptLaunch> {
        Some(ScriptLaunch { target: target.to_string(), is_argfile: true })
    }

    fn jar(target: &str) -> Option<ScriptLaunch> {
        Some(ScriptLaunch { target: target.to_string(), is_argfile: false })
    }

    #[test]
    fn parses_modern_neoforge_run_bat() {
        let script = "@echo off\r\n\
             java @user_jvm_args.txt @libraries/net/neoforged/neoforge/21.1.72/win_args.txt %*\r\n\
             pause\r\n";
        assert_eq!(
            parse_start_script(script),
            argfile("libraries/net/neoforged/neoforge/21.1.72/win_args.txt")
        );
    }

    #[test]
    fn parses_forge_run_sh() {
        let script = "#!/usr/bin/env sh\n\
             # Forge requires a configured set of both JVM and program arguments.\n\
             java @user_jvm_args.txt @libraries/net/minecraftforge/forge/1.20.1-47.3.0/unix_args.txt \"$@\"\n";
        assert_eq!(
            parse_start_script(script),
            argfile("libraries/net/minecraftforge/forge/1.20.1-47.3.0/unix_args.txt")
        );
    }

    #[test]
    fn parses_plain_jar_launch() {
        let script = "@echo off\r\njava -Xmx4G -Xms4G -jar fabric-server-launch.jar nogui\r\npause";
        assert_eq!(parse_start_script(script), jar("fabric-server-launch.jar"));
    }

    #[test]
    fn resolves_a_jar_named_by_a_batch_variable() {
        let script = "@echo off\r\n\
             set SERVER_JAR=\"forge-1.12.2-14.23.5.2860-universal.jar\"\r\n\
             java -Xmx6G -jar %SERVER_JAR% nogui\r\n";
        assert_eq!(parse_start_script(script), jar("forge-1.12.2-14.23.5.2860-universal.jar"));
    }

    #[test]
    fn resolves_a_shell_variable_and_quoted_java_path() {
        let script = "#!/bin/bash\nJAR=server.jar\n\"java\" -Xms2G -jar \"$JAR\" nogui\n";
        assert_eq!(parse_start_script(script), jar("server.jar"));
    }

    #[test]
    fn handles_windows_separators_and_leading_dot_slash() {
        let script = "java -jar .\\libraries\\minecraft_server.jar nogui";
        assert_eq!(parse_start_script(script), jar("libraries/minecraft_server.jar"));
    }

    #[test]
    fn ignores_comments_echoes_and_pauses() {
        let script = "@echo off\r\n\
             REM java -jar wrong-one.jar\r\n\
             :: java -jar also-wrong.jar\r\n\
             echo Starting server...\r\n\
             title My Server\r\n\
             java -jar right-one.jar nogui\r\n\
             pause\r\n";
        assert_eq!(parse_start_script(script), jar("right-one.jar"));
    }

    #[test]
    fn gives_up_on_an_unresolvable_variable() {
        let script = "java %JAVA_ARGS% -jar %UNSET_JAR% nogui";
        assert_eq!(parse_start_script(script), None);
    }

    #[test]
    fn gives_up_when_the_script_only_calls_another_script() {
        let script = "@echo off\r\ncall ServerStart.bat\r\npause";
        assert_eq!(parse_start_script(script), None);
    }

    #[test]
    fn ignores_user_jvm_args_as_a_launch_target() {
        assert_eq!(parse_start_script("java @user_jvm_args.txt nogui"), None);
    }
}
