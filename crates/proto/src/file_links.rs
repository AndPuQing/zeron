//! The file-link grammar for agent-authored Markdown, shared by the desktop
//! transcript and the phone document previews so both open the same path for
//! the same link text.
use std::borrow::Cow;

use crate::file_mentions::{FILE_MENTION_SCHEME, percent_decode_path, percent_encode_path};

/// The shape of a classified file link; each kind has its own inside/outside
/// rule once a root is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileLinkKind {
    /// `zeron-file:` mention — resolution stays inside-or-unresolved.
    Mention,
    /// Root-relative path.
    Relative,
    /// POSIX-absolute path — inside a root when one owns it, a host file
    /// read-only otherwise.
    Absolute,
}

/// A decoded path plus its line reference, tagged by shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedFileLink {
    pub kind: FileLinkKind,
    pub path: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

/// What a `#`/`:` line suffix or fragment contributed: a valid first-line
/// anchor, nothing usable (plain `#anchor` fragments are dropped), or an
/// anchor that kills the whole target (line 0, reversed range).
enum LineAnchor {
    None,
    Line { line: u32, column: Option<u32> },
    Invalid,
}

/// Classify a link destination under the file-link grammar — without a root
/// to resolve against yet. Line references split on the raw target so an
/// escaped `#` or `:` stays inside the path; the path itself then decodes
/// exactly once and every safety check runs on the decoded string.
pub fn classify_file_link(target: &str) -> Option<ClassifiedFileLink> {
    let target = target.trim();
    if target.is_empty() {
        return None;
    }

    // `zeron-file:` mentions keep their strict canonical spelling: the whole
    // path decodes once and must re-encode to the identical string.
    if let Some(encoded) = target.strip_prefix(FILE_MENTION_SCHEME) {
        let decoded = percent_decode_path(encoded)?;
        if percent_encode_path(&decoded) != encoded
            || decoded.ends_with('/')
            || decoded.contains(':')
            || !clean_path(&decoded)
        {
            return None;
        }
        return Some(ClassifiedFileLink {
            kind: FileLinkKind::Mention,
            path: decoded,
            line: None,
            column: None,
        });
    }

    let file_url = target.starts_with("file://");
    let raw = if file_url {
        let rest = &target["file://".len()..];
        // Only an empty host keeps this a file path — `file://localhost/…`
        // and friends are ordinary URLs — and a `?query` is never a file.
        let host_end = rest.find('/').unwrap_or(rest.len());
        if !rest[..host_end].is_empty() || rest.contains('?') {
            return None;
        }
        &rest[host_end..]
    } else {
        if target.contains("://") || target.starts_with("mailto:") {
            return None;
        }
        target
    };

    let (raw, fragment) = split_line_fragment(raw)?;
    let (raw, suffix) = split_line_suffix(raw)?;
    let decoded: Cow<str> = if raw.contains('%') {
        match percent_decode_path(raw) {
            Some(decoded) => Cow::Owned(decoded),
            // Broken escapes keep the raw spelling — a literal `%` in a file
            // name still opens.
            None => Cow::Borrowed(raw),
        }
    } else {
        Cow::Borrowed(raw)
    };
    if decoded.is_empty() || !clean_path(&decoded) {
        return None;
    }
    let kind = if decoded.starts_with('/') {
        // Absolute POSIX path: one leading slash, not the root itself, and
        // no trailing slash. A plain absolute path also wants a `.` in its
        // file name (`/usr/bin/ls` stays plain text); `file://` is exempt.
        if decoded.len() == 1
            || decoded.starts_with("//")
            || decoded.ends_with('/')
            || (!file_url && !file_name(&decoded).contains('.'))
        {
            return None;
        }
        FileLinkKind::Absolute
    } else {
        if file_url || decoded.starts_with('~') || has_url_scheme(&decoded) {
            return None;
        }
        if !file_name(&decoded).contains('.') {
            return None;
        }
        FileLinkKind::Relative
    };
    Some(ClassifiedFileLink {
        kind,
        path: decoded.into_owned(),
        line: fragment.line.or(suffix.line),
        column: fragment.column.or(suffix.column),
    })
}

/// `target` without its `#L12` fragment or `:12` line suffix: the path a
/// location link points into. A target that is not a location stays whole.
pub fn without_location(target: &str) -> &str {
    split_line_fragment(target)
        .and_then(|(path, _)| split_line_suffix(path))
        .map_or(target, |(path, _)| path)
}

/// A line reference merged across `#` fragment and `:` suffix: the fragment
/// wins each field it provides.
#[derive(Debug, Default)]
pub struct ParsedAnchor {
    pub line: Option<u32>,
    pub column: Option<u32>,
}

/// A `#L12`-style fragment (GitHub line anchors, ranges included, all
/// opening at the first line). A non-empty fragment without `/` that is not
/// one of these drops away, leaving the file path; `0` and reversed ranges
/// make the whole target not a file link.
pub fn split_line_fragment(target: &str) -> Option<(&str, ParsedAnchor)> {
    let Some((path, fragment)) = target.rsplit_once('#') else {
        return Some((target, ParsedAnchor::default()));
    };
    if fragment.is_empty() {
        return Some((path, ParsedAnchor::default()));
    }
    if fragment.contains('/') {
        // The `#` is inside the path, not an anchor.
        return Some((target, ParsedAnchor::default()));
    }
    match parse_anchor(fragment) {
        LineAnchor::Invalid => None,
        LineAnchor::None => Some((path, ParsedAnchor::default())),
        LineAnchor::Line { line, column } => Some((
            path,
            ParsedAnchor {
                line: Some(line),
                column,
            },
        )),
    }
}

/// `path:12`, `path:12:5` and `path:12-20` line suffixes. As with fragments,
/// a zero line or a backwards range makes the whole target not a file link.
pub fn split_line_suffix(target: &str) -> Option<(&str, ParsedAnchor)> {
    let Some(colon) = target.rfind(':') else {
        return Some((target, ParsedAnchor::default()));
    };
    let last = &target[colon + 1..];
    if let Some((start, end)) = last.split_once('-') {
        let (Ok(start), Ok(end)) = (start.parse::<u32>(), end.parse::<u32>()) else {
            return Some((target, ParsedAnchor::default()));
        };
        return if start == 0 || end < start {
            None
        } else {
            Some((
                &target[..colon],
                ParsedAnchor {
                    line: Some(start),
                    column: None,
                },
            ))
        };
    }
    if last.parse::<u32>().is_ok_and(|number| number == 0) {
        return None;
    }
    let Some(last_number) = positive_number(last) else {
        return Some((target, ParsedAnchor::default()));
    };
    let before = &target[..colon];
    let anchor = match before.rfind(':') {
        Some(colon2) => match positive_number(&before[colon2 + 1..]) {
            Some(line) => (
                &before[..colon2],
                ParsedAnchor {
                    line: Some(line),
                    column: Some(last_number),
                },
            ),
            // `a:0:5` rejects like `a:0`; `a:x:5` keeps `a:x` as the path —
            // the `:` it carries rejects it as a scheme-shaped target anyway.
            None if before[colon2 + 1..].parse::<u32>() == Ok(0) => return None,
            None => (
                before,
                ParsedAnchor {
                    line: Some(last_number),
                    column: None,
                },
            ),
        },
        None => (
            before,
            ParsedAnchor {
                line: Some(last_number),
                column: None,
            },
        ),
    };
    Some(anchor)
}

/// `#L12`, `#L12C5`, `#L12-L20`, `#L12-20` or `#L12C1-L20C3` — the parsed
/// first line plus its column when present.
fn parse_anchor(fragment: &str) -> LineAnchor {
    let Some(rest) = fragment.strip_prefix('L') else {
        return LineAnchor::None;
    };
    let (start_part, end_part) = match rest.split_once('-') {
        Some((start, end)) => (start, Some(end.strip_prefix('L').unwrap_or(end))),
        None => (rest, None),
    };
    let Some((line, column)) = parse_line_column(start_part) else {
        return LineAnchor::None;
    };
    if line == 0 || column == Some(0) {
        return LineAnchor::Invalid;
    }
    if let Some(end_part) = end_part {
        let Some((end, end_column)) = parse_line_column(end_part) else {
            return LineAnchor::None;
        };
        if end < line || end_column == Some(0) {
            return LineAnchor::Invalid;
        }
    }
    LineAnchor::Line { line, column }
}

/// `12` or `12C5` — the line number plus an optional column.
fn parse_line_column(value: &str) -> Option<(u32, Option<u32>)> {
    let (line, column) = match value.split_once('C') {
        Some((line, column)) => (line, Some(column)),
        None => (value, None),
    };
    let line = line.parse::<u32>().ok()?;
    let column = match column {
        Some(column) => Some(column.parse::<u32>().ok()?),
        None => None,
    };
    Some((line, column))
}

/// The path-safety checks every classified link passes, on the decoded path:
/// no backslashes, no `?`, no control characters, no `.`/`..`/empty segments.
/// A single leading `/` segment is allowed — it marks an absolute path.
pub fn clean_path(path: &str) -> bool {
    !path.contains(['\\', '?'])
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .enumerate()
            .all(|(index, part)| !(part.is_empty() && index != 0) && !matches!(part, "." | ".."))
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `scheme:` at the start of a relative path means it is a URL, not a file.
pub fn has_url_scheme(path: &str) -> bool {
    let mut chars = path.chars();
    if !chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
    {
        return false;
    }
    for character in chars {
        match character {
            ':' => return true,
            character
                if character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.') => {}
            _ => return false,
        }
    }
    false
}

fn positive_number(value: &str) -> Option<u32> {
    value.parse::<u32>().ok().filter(|number| *number > 0)
}

/// A link inside a Markdown document, resolved beside `document` (a
/// root-relative path): the target path plus its `#fragment`. URL path
/// resolution is independent of the viewer's filesystem; absolute paths,
/// schemes and `..` above the root are not document links.
pub fn document_link_target(document: &str, target: &str) -> Option<(String, Option<String>)> {
    if let Some(target) = target.strip_prefix(FILE_MENTION_SCHEME) {
        return document_link_target("", target);
    }
    if target.starts_with('/') || target.contains(':') || target.contains('\\') {
        return None;
    }
    let (path, anchor) = target
        .split_once('#')
        .map_or((target, None), |(p, a)| (p, Some(a.to_string())));
    let decode = |s: &str| -> Option<String> {
        let mut bytes = Vec::new();
        let mut chars = s.as_bytes().iter().copied();
        while let Some(c) = chars.next() {
            if c == b'%' {
                let a = (chars.next()? as char).to_digit(16)?;
                let b = (chars.next()? as char).to_digit(16)?;
                bytes.push((a * 16 + b) as u8);
            } else {
                bytes.push(c);
            }
        }
        String::from_utf8(bytes).ok()
    };
    let path = decode(path)?;
    if path.contains(['\\', ':', '\0']) || path.starts_with('/') {
        return None;
    }
    if path.is_empty() {
        return Some((document.into(), anchor.and_then(|a| decode(&a))));
    }
    let mut parts: Vec<&str> = document.split('/').collect();
    parts.pop();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some((parts.join("/"), anchor.and_then(|a| decode(&a))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classified(target: &str) -> Option<(FileLinkKind, String, Option<u32>, Option<u32>)> {
        classify_file_link(target).map(|link| (link.kind, link.path, link.line, link.column))
    }

    #[test]
    fn transcript_links_follow_the_desktop_grammar() {
        use FileLinkKind::*;
        assert_eq!(
            classified("file:///home/a%20b.md:12:3"),
            Some((Absolute, "/home/a b.md".into(), Some(12), Some(3)))
        );
        assert_eq!(
            classified("/repo/readme.md#L10"),
            Some((Absolute, "/repo/readme.md".into(), Some(10), None))
        );
        assert_eq!(
            classified("zeron-file:docs/a.md"),
            Some((Mention, "docs/a.md".into(), None, None))
        );
        // A broken escape keeps its raw spelling; a valid one decodes once.
        assert_eq!(classified("100%.md"), Some((Relative, "100%.md".into(), None, None)));
        assert_eq!(classified("100%25.md"), Some((Relative, "100%.md".into(), None, None)));
        assert_eq!(classified("a%23b.md"), Some((Relative, "a#b.md".into(), None, None)));
        for target in [
            "file://localhost/home/a.md",
            "file://server/a.md",
            "https://example.com/a.md",
            "mailto:a@b.com",
            "../secret.md",
            "%2e%2e/secret.md",
            "docs/",
            "LICENSE",
            "/usr/bin/ls",
            "~/notes.md",
            r"C:\repo\a.md",
            "zeron-file:docs/a%20b.md#x",
        ] {
            assert_eq!(classified(target), None, "{target}");
        }
        assert_eq!(without_location("src/a.rs:12"), "src/a.rs");
    }

    #[test]
    fn document_links_resolve_beside_their_document_inside_the_root() {
        for (document, target, expected) in [
            ("docs/readme.md", "../a%20b.md#hello", Some(("a b.md", Some("hello")))),
            ("docs/readme.md", "#hello", Some(("docs/readme.md", Some("hello")))),
            ("docs/readme.md", "LICENSE", Some(("docs/LICENSE", None))),
            ("docs/readme.md", "zeron-file:src/lib.rs", Some(("src/lib.rs", None))),
            ("readme.md", "../secret", None),
            ("docs/readme.md", "%2Fetc/passwd", None),
            ("docs/readme.md", "/etc/passwd", None),
            ("docs/readme.md", "https://example.com", None),
        ] {
            assert_eq!(
                document_link_target(document, target),
                expected.map(|(path, anchor)| (path.to_owned(), anchor.map(str::to_owned))),
                "{target}"
            );
        }
    }
}
