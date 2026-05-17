//! Broken-link checker scoped to the `docs/` directory.
//!
//! For each `.md` file under `docs/`, extract local relative link targets from
//! `[text](target)` markdown and `<a href="target">` HTML, resolve them
//! relative to the source file, and assert the target exists.
//!
//! Skipped target classes:
//!   * `http://` and `https://` (no network in tests)
//!   * `mailto:`, `tel:`, `ftp:`, etc. (any scheme)
//!   * Anchors only (`#…`)
//!   * Absolute internal targets that start with `/` (those are for the
//!     website root, not the in-repo `docs/` tree)

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use regex::Regex;

fn repo_root() -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest)
}

fn docs_root() -> PathBuf {
    repo_root().join("docs")
}

fn collect_markdown_files(root: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(&path, out);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

/// Strip fenced code blocks (```...```) and inline code spans (`...`) so that
/// example URLs inside snippets are not treated as links.
fn strip_code(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            out.push('\n');
            continue;
        }
        if in_fence {
            out.push('\n');
            continue;
        }
        // Strip inline code spans on this line.
        let mut chars = line.chars().peekable();
        let mut in_code = false;
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                if let Some(next) = chars.next() {
                    if !in_code {
                        out.push(ch);
                        out.push(next);
                    }
                }
                continue;
            }
            if ch == '`' {
                in_code = !in_code;
                continue;
            }
            if !in_code {
                out.push(ch);
            }
        }
        out.push('\n');
    }
    out
}

fn extract_link_targets(content: &str) -> BTreeSet<String> {
    let mut targets = BTreeSet::new();
    let stripped = strip_code(content);

    // Markdown links: [text](target). Title attributes are uncommon in this
    // codebase but we strip them just in case (target may contain a space and
    // a quoted title).
    let md_link = Regex::new(r#"\[(?:[^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)"#).unwrap();
    for caps in md_link.captures_iter(&stripped) {
        if let Some(target) = caps.get(1) {
            targets.insert(target.as_str().to_string());
        }
    }

    // HTML anchors: <a href="target"> or <a href='target'>.
    let html_double = Regex::new(r#"<a\b[^>]*\shref="([^"]+)""#).unwrap();
    for caps in html_double.captures_iter(&stripped) {
        if let Some(target) = caps.get(1) {
            targets.insert(target.as_str().to_string());
        }
    }
    let html_single = Regex::new(r#"<a\b[^>]*\shref='([^']+)'"#).unwrap();
    for caps in html_single.captures_iter(&stripped) {
        if let Some(target) = caps.get(1) {
            targets.insert(target.as_str().to_string());
        }
    }

    targets
}

fn is_external_scheme(target: &str) -> bool {
    if let Some((scheme, _)) = target.split_once(':') {
        if scheme.is_empty() {
            return false;
        }
        // Any ASCII-alpha scheme is treated as external (http, https, mailto,
        // tel, ftp, irc, file, ws, wss, …).
        return scheme.chars().all(|c| c.is_ascii_alphabetic());
    }
    false
}

fn is_skippable(target: &str) -> bool {
    if target.starts_with('#') {
        return true;
    }
    if target.starts_with('/') {
        return true;
    }
    if is_external_scheme(target) {
        return true;
    }
    false
}

fn resolve_target(source: &Path, target: &str) -> PathBuf {
    let cleaned = match target.split_once('#') {
        Some((path, _anchor)) => path,
        None => target,
    };
    let cleaned = match cleaned.split_once('?') {
        Some((path, _query)) => path,
        None => cleaned,
    };
    let parent = source.parent().unwrap_or_else(|| Path::new("."));
    parent.join(cleaned)
}

#[test]
fn docs_local_links_resolve() {
    let docs = docs_root();
    assert!(
        docs.exists(),
        "docs directory missing at {}",
        docs.display()
    );

    let mut files = Vec::new();
    collect_markdown_files(&docs, &mut files);
    assert!(
        !files.is_empty(),
        "no markdown files found under {}",
        docs.display()
    );

    let mut broken: Vec<String> = Vec::new();

    for file in &files {
        let content = match fs::read_to_string(file) {
            Ok(text) => text,
            Err(err) => {
                broken.push(format!("could not read {}: {err}", file.display()));
                continue;
            }
        };
        for target in extract_link_targets(&content) {
            if is_skippable(&target) {
                continue;
            }
            let resolved = resolve_target(file, &target);
            if !resolved.exists() {
                broken.push(format!(
                    "broken link in {}: target {:?} does not resolve to a file (looked at {})",
                    file.display(),
                    target,
                    resolved.display()
                ));
            }
        }
    }

    assert!(
        broken.is_empty(),
        "broken links found:\n  - {}",
        broken.join("\n  - ")
    );
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn extract_link_targets_handles_markdown_and_html() {
        let text = r#"
See [docs](./other.md) and <a href="../sibling.md">sibling</a>.
Anchor only: [top](#top).
External: [home](https://example.com).
"#;
        let targets = extract_link_targets(text);
        assert!(targets.contains("./other.md"));
        assert!(targets.contains("../sibling.md"));
        assert!(targets.contains("#top"));
        assert!(targets.contains("https://example.com"));
    }

    #[test]
    fn extract_link_targets_skips_code_blocks() {
        let text = r#"
Outside [a](./real.md) link.

```bash
[fake](./missing-on-purpose.md)
```

Also `[inline](./also-fake.md)` inline.
"#;
        let targets = extract_link_targets(text);
        assert!(targets.contains("./real.md"));
        assert!(!targets.contains("./missing-on-purpose.md"));
        assert!(!targets.contains("./also-fake.md"));
    }

    #[test]
    fn is_skippable_classifies_targets() {
        assert!(is_skippable("https://example.com"));
        assert!(is_skippable("http://example.com"));
        assert!(is_skippable("mailto:x@example.com"));
        assert!(is_skippable("#anchor"));
        assert!(is_skippable("/absolute"));
        assert!(!is_skippable("./relative.md"));
        assert!(!is_skippable("sibling.md"));
        assert!(!is_skippable("../parent.md"));
    }
}
