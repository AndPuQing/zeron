//! Self-contained agent file citations. Rendering never resolves opaque source
//! IDs or reads the filesystem; ordinary link navigation owns that decision.
use std::ops::Range;
use std::sync::Arc;

use pulldown_cmark::{Event, Options, Parser, Tag};

use crate::parser::{Block, BlockTree, InlineRun};

const DIRECTIVE: &str = ":codex-file-citation{";
const MAX_CITATION: usize = 16 * 1024;

struct Edit {
    original: Range<usize>,
    rewritten: Range<usize>,
}

pub(crate) struct Rewrite {
    pub text: String,
    edits: Vec<Edit>,
}

impl Rewrite {
    fn original(&self, at: usize) -> usize {
        let count = self
            .edits
            .partition_point(|edit| edit.rewritten.start <= at);
        let Some(edit) = count.checked_sub(1).map(|i| &self.edits[i]) else {
            return at;
        };
        if at < edit.rewritten.end {
            edit.original.start
        } else {
            edit.original.end + (at - edit.rewritten.end)
        }
    }

    /// Preserve original source ranges for incremental parsing and task edits.
    pub fn restore_ranges(&self, tree: &mut BlockTree, offset: usize) {
        fn runs(runs: &mut [InlineRun], rewrite: &Rewrite, offset: usize) {
            for run in runs {
                if let Some(task) = &mut run.style.task {
                    task.range = offset + rewrite.original(task.range.start)
                        ..offset + rewrite.original(task.range.end);
                }
            }
        }
        fn visit(value: &mut Block, rewrite: &Rewrite, offset: usize) {
            match value {
                Block::Paragraph { runs: r } | Block::Heading { runs: r, .. } => {
                    runs(r, rewrite, offset)
                }
                Block::BlockQuote { children } => {
                    for child in children {
                        visit(child, rewrite, offset);
                    }
                }
                Block::List { items, .. } => {
                    for child in items.iter_mut().flatten() {
                        visit(child, rewrite, offset);
                    }
                }
                Block::Table { header, rows, .. } => {
                    for cell in header.iter_mut().chain(rows.iter_mut().flatten()) {
                        runs(cell, rewrite, offset);
                    }
                }
                _ => {}
            }
        }
        for top in &mut tree.blocks {
            let top = Arc::make_mut(top);
            top.range =
                offset + self.original(top.range.start)..offset + self.original(top.range.end);
            visit(&mut top.block, self, offset);
        }
    }
}

/// Rewrite only citation openers in prose. A path's punctuation is data, not
/// Markdown: it is escaped before the ordinary parser sees the replacement.
pub(crate) fn rewrite(source: &str, options: Options) -> Option<Rewrite> {
    if !source.contains(DIRECTIVE) && !source.contains('【') {
        return None;
    }
    let mut excluded = Vec::new();
    let mut text = Vec::new();
    for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
        match event {
            Event::Start(
                Tag::CodeBlock(_) | Tag::Link { .. } | Tag::Image { .. } | Tag::HtmlBlock,
            )
            | Event::Code(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::Html(_)
            | Event::InlineHtml(_) => excluded.push(range),
            Event::Text(_) => text.push(range),
            _ => {}
        }
    }
    excluded.sort_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in excluded {
        if let Some(last) = merged.last_mut().filter(|last| range.start <= last.end) {
            last.end = last.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    let contains = |ranges: &[Range<usize>], at: usize| {
        let n = ranges.partition_point(|range| range.start <= at);
        n > 0 && at < ranges[n - 1].end
    };
    text.sort_by_key(|range| range.start);
    let mut candidates: Vec<_> = source
        .match_indices(DIRECTIVE)
        .map(|(at, _)| at)
        .chain(source.match_indices('【').map(|(at, _)| at))
        .collect();
    candidates.sort_unstable();
    let mut out = String::new();
    let mut edits = Vec::new();
    let mut copied = 0;
    for at in candidates {
        if at < copied
            || contains(&merged, at)
            || !contains(&text, at)
            || source[..at]
                .bytes()
                .rev()
                .take_while(|b| *b == b'\\')
                .count()
                % 2
                == 1
        {
            continue;
        }
        let tail = &source[at..];
        let citation = if tail.starts_with(DIRECTIVE) {
            directive(tail)
        } else {
            bracket(tail)
        };
        let Some((len, label, target)) = citation else {
            continue;
        };
        out.push_str(&source[copied..at]);
        let start = out.len();
        out.push_str(&markdown_link(&label, &target));
        copied = at + len;
        edits.push(Edit {
            original: at..copied,
            rewritten: start..out.len(),
        });
    }
    if edits.is_empty() {
        return None;
    }
    out.push_str(&source[copied..]);
    Some(Rewrite { text: out, edits })
}

fn directive(source: &str) -> Option<(usize, String, String)> {
    let limit = source.len().min(MAX_CITATION);
    let bytes = source.as_bytes();
    let mut at = DIRECTIVE.len();
    let mut path = None;
    let (mut start, mut end) = (None, None);
    while at < limit {
        while at < limit && matches!(bytes[at], b' ' | b'\t') {
            at += 1;
        }
        if bytes.get(at) == Some(&b'}') {
            let (label, target) = file_reference(path.as_deref()?, start, end)?;
            return Some((at + 1, label, target));
        }
        let begin = at;
        while at < limit && (bytes[at].is_ascii_alphanumeric() || matches!(bytes[at], b'_' | b'-'))
        {
            at += 1;
        }
        if at == begin {
            return None;
        }
        let key = &source[begin..at];
        while at < limit && matches!(bytes[at], b' ' | b'\t') {
            at += 1;
        }
        if bytes.get(at)? != &b'=' {
            return None;
        }
        at += 1;
        while at < limit && matches!(bytes[at], b' ' | b'\t') {
            at += 1;
        }
        let quote = *bytes.get(at)?;
        let value;
        if matches!(quote, b'"' | b'\'') {
            at += 1;
            let begin = at;
            while at < limit && bytes[at] != quote {
                if bytes[at] == b'\n' || bytes[at] == b'\r' {
                    return None;
                }
                if bytes[at] == b'\\' && matches!(bytes.get(at + 1), Some(b'\\' | b'"' | b'\'')) {
                    at += 1;
                }
                at += 1;
            }
            if at >= limit {
                return None;
            }
            value = unquote(&source[begin..at]);
            at += 1;
        } else {
            let begin = at;
            while at < limit && bytes[at].is_ascii_digit() {
                at += 1;
            }
            if at == begin {
                return None;
            }
            value = source[begin..at].to_owned();
        }
        match key {
            "path" if path.is_none() => path = Some(value),
            "path" => return None,
            "line" | "start_line" | "line_start" if start.is_none() => {
                start = Some(positive(&value)?)
            }
            "end_line" | "line_end" if end.is_none() => end = Some(positive(&value)?),
            "line" | "start_line" | "line_start" | "end_line" | "line_end" => return None,
            _ => {} // purpose/title and future metadata do not change navigation.
        }
        if bytes
            .get(at)
            .is_some_and(|b| !b.is_ascii_whitespace() && *b != b'}')
        {
            return None;
        }
    }
    None
}

fn unquote(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.peek().is_some_and(|c| matches!(c, '\\' | '"' | '\'')) {
            out.push(chars.next().unwrap());
        } else {
            out.push(ch);
        }
    }
    out
}

/// Path-bearing citations used by coding agents, e.g. 【F:src/a.rs†L2-L5】.
/// Opaque web/upload IDs are deliberately not interpreted as file paths.
fn bracket(source: &str) -> Option<(usize, String, String)> {
    let (end, close) = source['【'.len_utf8()..]
        .char_indices()
        .map(|(at, ch)| (at + '【'.len_utf8(), ch))
        .take_while(|(at, _)| *at < MAX_CITATION)
        .find(|(_, ch)| matches!(ch, '【' | '】'))?;
    if close != '】' {
        return None;
    }
    if end > MAX_CITATION {
        return None;
    }
    let body = &source['【'.len_utf8()..end];
    let (path, lines) = body.split_once('†')?;
    let path = path.strip_prefix("F:").unwrap_or(path);
    if !path.rsplit('/').next()?.contains('.') {
        return None;
    }
    let (first, last) = match lines.strip_prefix('L')?.split_once('-') {
        Some((first, last)) => (
            positive(first)?,
            Some(positive(last.strip_prefix('L').unwrap_or(last))?),
        ),
        None => (positive(lines.strip_prefix('L')?)?, None),
    };
    let (label, target) = file_reference(path, Some(first), last)?;
    Some((end + '】'.len_utf8(), label, target))
}

/// Cursor's code-reference fence info: start:end:path (the snippet stays code).
pub(crate) fn code_reference(info: &str) -> Option<(String, String)> {
    let mut parts = info.trim().splitn(3, ':');
    let start = positive(parts.next()?)?;
    let end = positive(parts.next()?)?;
    let path = parts.next()?;
    if !path.rsplit('/').next()?.contains('.') {
        return None;
    }
    file_reference(path, Some(start), Some(end))
}

/// Explicit path:line[:column] code spans used by Claude Code and other
/// terminal agents. Ordinary identifiers and commands remain code, unlinked.
pub fn code_location(text: &str) -> Option<String> {
    let (before, last) = text.rsplit_once(':')?;
    let (path, start, end, column) = if let Some((first, end)) = last.split_once('-') {
        (before, positive(first)?, Some(positive(end)?), None)
    } else if let Some((path, line)) = before.rsplit_once(':') {
        (path, positive(line)?, None, Some(positive(last)?))
    } else {
        (before, positive(last)?, None, None)
    };
    if path.contains('=') {
        return None;
    }
    if path
        .find(char::is_whitespace)
        .is_some_and(|space| path.find('/').is_none_or(|slash| slash > space))
    {
        return None;
    }
    let extension = path.rsplit('/').next()?.rsplit_once('.')?.1;
    if !extension.chars().any(char::is_alphabetic) {
        return None;
    }
    // A bare host:port or dotted program expression is not a source location.
    // With no path separator, require a familiar code/document extension;
    // explicit Markdown links and citation directives have no such heuristic.
    if !path.contains('/')
        && !matches!(
            extension.to_ascii_lowercase().as_str(),
            "rs" | "py"
                | "js"
                | "jsx"
                | "ts"
                | "tsx"
                | "go"
                | "c"
                | "h"
                | "cpp"
                | "hpp"
                | "cc"
                | "java"
                | "kt"
                | "kts"
                | "swift"
                | "m"
                | "mm"
                | "cs"
                | "rb"
                | "php"
                | "sh"
                | "fish"
                | "zsh"
                | "md"
                | "mdx"
                | "txt"
                | "rst"
                | "json"
                | "jsonc"
                | "toml"
                | "yaml"
                | "yml"
                | "xml"
                | "html"
                | "css"
                | "scss"
                | "sql"
                | "vue"
                | "svelte"
                | "dart"
                | "lua"
                | "luau"
                | "ex"
                | "exs"
                | "erl"
                | "scala"
                | "pl"
                | "r"
                | "jl"
                | "hs"
                | "ml"
                | "ps1"
                | "proto"
                | "gradle"
                | "cmake"
                | "lock"
                | "csv"
                | "tsv"
                | "log"
                | "pdf"
                | "tex"
                | "conf"
                | "cfg"
                | "ini"
        )
    {
        return None;
    }
    let (_, mut target) = file_reference(path, Some(start), end)?;
    if let Some(column) = column {
        target.push_str(&format!("C{column}"));
    }
    Some(target)
}

/// Plain path:line references outside code, formulas and existing links.
/// Whitespace-containing paths must use inline code or an explicit citation.
pub(crate) fn link_locations(runs: Vec<InlineRun>) -> Vec<InlineRun> {
    let mut out = Vec::with_capacity(runs.len());
    for run in runs {
        if run.style.link.is_some()
            || run.style.code
            || run.style.math
            || run.style.image.is_some()
            || !run.text.contains(':')
        {
            out.push(run);
            continue;
        }
        let mut copied = 0;
        let mut offset = 0;
        for token in run.text.split_inclusive(char::is_whitespace) {
            let trimmed = token
                .trim()
                .trim_start_matches(['(', '[', '{', '\'', '"'])
                .trim_end_matches([')', ']', '}', ',', ';', '!', '.', '\'', '"']);
            let start = offset + token.find(trimmed).unwrap_or(0);
            offset += token.len();
            let Some(target) = code_location(trimmed) else {
                continue;
            };
            if start > copied {
                out.push(InlineRun {
                    text: run.text[copied..start].into(),
                    style: run.style.clone(),
                });
            }
            let mut style = run.style.clone();
            style.link = Some(target);
            out.push(InlineRun {
                text: trimmed.into(),
                style,
            });
            copied = start + trimmed.len();
        }
        if copied == 0 {
            out.push(run);
        } else if copied < run.text.len() {
            out.push(InlineRun {
                text: run.text[copied..].into(),
                style: run.style,
            });
        }
    }
    out
}

fn positive(text: &str) -> Option<u32> {
    (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
        .filter(|n| *n > 0)
}

fn file_reference(path: &str, start: Option<u32>, end: Option<u32>) -> Option<(String, String)> {
    if path.is_empty()
        || path.starts_with(['~', '\\'])
        || path.contains(['\\', ':', '?'])
        || path.chars().any(char::is_control)
        || path.ends_with('/')
        || path
            .split('/')
            .enumerate()
            .any(|(i, p)| matches!(p, "." | "..") || (p.is_empty() && i != 0))
        || end.is_some_and(|end| start.is_none_or(|start| end < start))
    {
        return None;
    }
    let name = path.rsplit('/').next()?;
    let mut target = if path.starts_with('/') {
        "file://".to_owned()
    } else {
        String::new()
    };
    // Relative extensionless files use the existing explicit mention scheme.
    if !path.starts_with('/') && !name.contains('.') {
        if start.is_some() {
            return None;
        }
        target.push_str("zeron-file:");
    }
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            target.push(byte as char);
        } else {
            target.push_str(&format!("%{byte:02X}"));
        }
    }
    let mut label = name.to_owned();
    if let Some(start) = start {
        target.push_str(&format!("#L{start}"));
        label.push_str(&format!(":{start}"));
        if let Some(end) = end.filter(|end| *end != start) {
            target.push_str(&format!("-L{end}"));
            label.push_str(&format!("–{end}"));
        }
    }
    Some((label, target))
}

fn markdown_link(label: &str, target: &str) -> String {
    let mut escaped = String::new();
    for ch in label.chars() {
        if ch.is_ascii_punctuation() {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    format!("[{escaped}](<{target}>)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IncrementalParser, parse_full};

    fn links(block: &Block, found: &mut Vec<(String, String)>) {
        match block {
            Block::Paragraph { runs } | Block::Heading { runs, .. } => {
                found.extend(runs.iter().filter_map(|r| {
                    r.style
                        .link
                        .as_ref()
                        .map(|target| (r.text.clone(), target.clone()))
                }));
            }
            Block::List { items, .. } => {
                for b in items.iter().flatten() {
                    links(b, found);
                }
            }
            Block::BlockQuote { children } => {
                for b in children {
                    links(b, found);
                }
            }
            Block::Table { header, rows, .. } => {
                for c in header.iter().chain(rows.iter().flatten()) {
                    found.extend(c.iter().filter_map(|r| {
                        r.style
                            .link
                            .as_ref()
                            .map(|target| (r.text.clone(), target.clone()))
                    }));
                }
            }
            _ => {}
        }
    }
    fn references(source: &str) -> Vec<(String, String)> {
        let mut found = Vec::new();
        for block in parse_full(source).blocks {
            links(&block.block, &mut found);
        }
        found
    }

    #[test]
    fn codex_file_directive_preserves_native_path_and_hides_transport_metadata() {
        let source = r#":codex-file-citation{path="/home/user/Cloud Drive/project/spec_origin.pdf" purpose="source"}"#;
        assert_eq!(
            references(source),
            [(
                "spec_origin.pdf".into(),
                "file:///home/user/Cloud%20Drive/project/spec_origin.pdf".into()
            )]
        );
        let tree = parse_full(source);
        assert_eq!(tree.blocks[0].range, 0..source.len());
        assert!(
            matches!(&tree.blocks[0].block, Block::Paragraph {runs} if runs.len()==1 && runs[0].text=="spec_origin.pdf")
        );
    }

    #[test]
    fn punctuation_unicode_and_percent_are_file_name_data() {
        let source = r#"before :codex-file-citation{purpose="source" path="/docs/中文/a_[b]#50%`$x$.pdf"} after"#;
        assert_eq!(
            references(source),
            [(
                "a_[b]#50%`$x$.pdf".into(),
                "file:///docs/%E4%B8%AD%E6%96%87/a_%5Bb%5D%2350%25%60%24x%24.pdf".into()
            )]
        );
        assert_eq!(
            references(r#":codex-file-citation{path='/repo/a}.md' line_start=2 line_end="5"}"#),
            [("a}.md:2–5".into(), "file:///repo/a%7D.md#L2-L5".into())]
        );
    }

    #[test]
    fn code_examples_existing_links_and_opaque_ids_stay_literal() {
        let directive = r#":codex-file-citation{path="/repo/a.pdf"}"#;
        for text in [
            format!("`{directive}`"),
            format!("```text\n{directive}\n```"),
            format!("    {directive}\n"),
            format!("\\{directive}"),
            format!("<div>\n{directive}\n</div>"),
            format!("${directive}$"),
            "fileciteturn0file0".into(),
            "【turn0file0†L2-L4】".into(),
        ] {
            assert!(references(&text).is_empty(), "{text}");
        }
        assert_eq!(
            references(&format!("[{directive}](https://example.com)")),
            [(directive.into(), "https://example.com".into())]
        );
        for tail in ["", "path=", "path=\"/repo/a.pdf", "path=\"/repo/a.pdf\""] {
            assert!(references(&format!("{DIRECTIVE}{tail}")).is_empty());
        }
        for path in [
            "javascript:alert(1)",
            "../secret.pdf",
            "/a/../b.pdf",
            "//server/a.pdf",
            r"C:\repo\a.pdf",
        ] {
            assert!(references(&format!(":codex-file-citation{{path=\"{path}\"}}")).is_empty());
        }
        assert!(
            references(r#":codex-file-citation{path="a.rs" start_line=9 end_line=2}"#).is_empty()
        );
    }

    #[test]
    fn agent_markdown_and_path_bearing_bracket_references_share_link_navigation() {
        for source in [
            "[a.rs](src/a.rs#L2-L5)",
            "【F:src/a.rs†L2-L5】",
            "【src/a.rs†L2-L5】",
        ] {
            assert_eq!(references(source)[0].1, "src/a.rs#L2-L5");
        }
        let source = "```2:5:src/a file.rs\nlet x = 1;\n```";
        let tree = parse_full(source);
        assert_eq!(
            references(source),
            [("a file.rs:2–5".into(), "src/a%20file.rs#L2-L5".into())]
        );
        assert!(
            matches!(&tree.blocks[1].block, Block::CodeBlock {code, language:None} if code=="let x = 1;")
        );
        assert_eq!(tree.blocks[0].range, tree.blocks[1].range);
        assert!(references("```8:2:src/a.rs\nx\n```").is_empty());
    }

    #[test]
    fn claude_and_terminal_agent_locations_are_clickable_without_changing_code_examples() {
        for source in [
            "See src/main.rs:42.",
            "See `src/main.rs:42`.",
            "(src/main.rs:42)",
        ] {
            assert_eq!(
                references(source),
                [("src/main.rs:42".into(), "src/main.rs#L42".into())]
            );
        }
        assert_eq!(
            references("`src/main.rs:42:7`"),
            [("src/main.rs:42:7".into(), "src/main.rs#L42C7".into())]
        );
        assert_eq!(
            references("`/home/user/My Project/app.rs:2-4`"),
            [(
                "/home/user/My Project/app.rs:2-4".into(),
                "file:///home/user/My%20Project/app.rs#L2-L4".into()
            )]
        );
        for source in [
            "../private.rs:2",
            "./src/a.rs:4",
            "127.0.0.1:8080",
            "example.com:443",
            "api.example.dev:8080",
            "src/main.rs:0",
            "v1.2:3",
            "```text\nsrc/main.rs:42\n```",
            "`echo hello`",
            "`echo src/main.rs:42`",
            "`VAR=path.rs:2`",
        ] {
            assert!(references(source).is_empty(), "{source}");
        }
        assert_eq!(
            references("[src/main.rs:42](https://example.com)"),
            [("src/main.rs:42".into(), "https://example.com".into())]
        );
    }

    #[test]
    fn citation_rewrites_preserve_surrounding_block_source_ranges() {
        let citation = r#":codex-file-citation{path="/docs/中文/a long file.pdf"}"#;
        let source = format!("prefix\n\n{citation}\n\n- [ ] task\n\nlast");
        let tree = parse_full(&source);
        let original_ranges: Vec<_> = tree
            .blocks
            .iter()
            .map(|b| &source[b.range.clone()])
            .collect();
        assert_eq!(
            original_ranges,
            ["prefix\n", &format!("{citation}\n"), "- [ ] task\n\n", "last"]
        );
        let long_path = format!("/docs/{}.pdf", "界".repeat(100));
        let source = format!(":codex-file-citation{{path=\"{long_path}\"}}\n\n- [x] after");
        let tree = parse_full(&source);
        let Block::List { items, .. } = &tree.blocks[1].block else {
            panic!()
        };
        let Block::Paragraph { runs } = &items[0][0] else {
            panic!()
        };
        let task = runs.iter().find_map(|r| r.style.task.as_ref()).unwrap();
        assert_eq!(&source[task.range.clone()], "[x]");
    }

    #[test]
    fn repeated_unclosed_citations_do_not_scan_the_remaining_document_each_time() {
        let text = "【".repeat(50_000);
        assert!(rewrite(&text, Options::empty()).is_none());
        let text = format!("{}L1】", "【F:a.rs†".repeat(10_000));
        assert_eq!(references(&text).len(), 1);
    }

    #[test]
    fn streaming_ranges_tasks_and_nested_citations_match_full_parse() {
        let source = concat!(
            "Intro\n\n- [x] **See :codex-file-citation{path=\"/docs/中文/a_[x].pdf\"}**\n",
            "- [ ] Next 【F:src/a.rs†L2-L4】\n\n",
            "| Source |\n|---|\n| :codex-file-citation{path=\"/docs/b.pdf\"} |\n\n",
            "```2:3:src/a.rs\ncode\n```\n\nLast\n",
        );
        let mut stream = IncrementalParser::new();
        let mut consumed = String::new();
        for ch in source.chars() {
            consumed.push(ch);
            stream.append(&ch.to_string());
            assert_eq!(stream.tree(), &parse_full(&consumed), "{consumed}");
            let mut settled = IncrementalParser::new();
            settled.set_text(&consumed);
            assert_eq!(stream.display_tree(), settled.display_tree(), "{consumed}");
        }
        let tree = stream.tree();
        let Block::List { items, .. } = &tree.blocks[1].block else {
            panic!("{tree:?}")
        };
        for (item, spelling) in items.iter().zip(["[x]", "[ ]"]) {
            let Block::Paragraph { runs } = &item[0] else {
                panic!()
            };
            let range = &runs
                .iter()
                .find_map(|r| r.style.task.as_ref())
                .unwrap()
                .range;
            assert_eq!(&source[range.clone()], spelling);
        }
        assert!(
            matches!(&items[0][0], Block::Paragraph {runs} if runs.iter().any(|r| r.style.link.is_some() && r.style.bold))
        );
    }
}
