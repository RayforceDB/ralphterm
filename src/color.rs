//! Tiny ANSI color helpers for ralphterm's own output.
//!
//! No new dependency: just raw escape codes wrapped in a guard that
//! checks `NO_COLOR` (the de-facto standard) and stdout-is-a-tty.
//! Callers don't need to know whether colors are enabled — the helpers
//! return either the wrapped string or the bare string.

use std::io::IsTerminal;
use std::sync::OnceLock;

const RESET: &str = "\x1b[0m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const CYAN: &str = "\x1b[36m";
const MAGENTA: &str = "\x1b[35m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";

fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        if std::env::var_os("NO_COLOR").is_some() {
            return false;
        }
        if std::env::var_os("RALPHTERM_NO_COLOR").is_some() {
            return false;
        }
        std::io::stdout().is_terminal()
    })
}

fn wrap(prefix: &'static str, text: &str) -> String {
    if enabled() {
        format!("{prefix}{text}{RESET}")
    } else {
        text.to_string()
    }
}

pub fn dim(text: &str) -> String {
    wrap(DIM, text)
}
pub fn bold(text: &str) -> String {
    wrap(BOLD, text)
}
pub fn cyan(text: &str) -> String {
    wrap(CYAN, text)
}
pub fn magenta(text: &str) -> String {
    wrap(MAGENTA, text)
}
pub fn green(text: &str) -> String {
    wrap(GREEN, text)
}
pub fn yellow(text: &str) -> String {
    wrap(YELLOW, text)
}

/// True if the line looks like a section header from an agent's
/// formatted response — short, no leading bullet marker, no embedded
/// colon (those tend to be `key: value` lines). E.g. "What I did",
/// "Validation", "Commit", "Next iteration".
pub fn is_section_header(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    if t.starts_with("- ") || t.starts_with("* ") || t.starts_with('#') {
        return false;
    }
    if t.contains(':') {
        return false;
    }
    if t.len() > 60 {
        return false;
    }
    t.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_header_detection() {
        assert!(is_section_header("What I did"));
        assert!(is_section_header("Validation"));
        assert!(is_section_header("Commit"));
        assert!(is_section_header("Next iteration"));

        assert!(!is_section_header("- Created index.html"));
        assert!(!is_section_header("  - nested bullet"));
        assert!(!is_section_header("commit: abc1234"));
        assert!(!is_section_header(""));
        assert!(!is_section_header(
            "this is a long sentence that runs on and on past the header threshold and shouldn't be flagged as one"
        ));
    }
}
