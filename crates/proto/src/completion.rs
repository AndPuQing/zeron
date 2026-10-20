//! Shared desktop/mobile completion boundaries in Markdown prose. Offsets are UTF-8 bytes.
use pulldown_cmark::{Event, Options, Parser, Tag};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionToken {
    pub range: Range<usize>,
    pub query: String,
}

/// Refine a locally identified token using Markdown source ranges. This is
/// deliberately called only after a trigger has passed cheap boundary checks.
pub fn completion_markdown_end(
    text: &str,
    start: usize,
    cursor: usize,
    mut end: usize,
) -> Option<usize> {
    use pulldown_cmark::{Event, Options, Parser, Tag};
    let needs_quote_boundary = text[..start].ends_with('>');
    let line_start = text[..start].rfind('\n').map_or(0, |at| at + 1);
    let mut paragraph_start = line_start;
    let mut token_is_prose = false;
    let mut quote_boundary = false;
    let parser = Parser::new_ext(text, Options::ENABLE_STRIKETHROUGH);
    // Definitions have no body events, but their destinations are still link
    // syntax and must never be replaced by a nested canonical link.
    if parser
        .reference_definitions()
        .iter()
        .any(|(_, definition)| definition.span.contains(&start))
    {
        return None;
    }
    for (event, range) in parser.into_offset_iter() {
        if !range.contains(&start) {
            continue;
        }
        match event {
            Event::Start(Tag::Link { .. } | Tag::Image { .. }) => return None,
            Event::Start(Tag::Paragraph | Tag::Heading { .. } | Tag::Item) => {
                paragraph_start = range.start;
            }
            Event::Text(_) => token_is_prose = true,
            Event::Start(Tag::BlockQuote(_)) if needs_quote_boundary => {
                let prefix = &text[range.start.max(line_start)..start];
                quote_boundary |= prefix.chars().all(|ch| matches!(ch, '>' | ' ' | '\t'));
            }
            Event::Start(tag @ (Tag::Strong | Tag::Emphasis | Tag::Strikethrough)) => {
                let delimiter = match tag {
                    Tag::Strong => 2,
                    Tag::Emphasis => 1,
                    _ => text[range.clone()]
                        .bytes()
                        .take_while(|byte| *byte == b'~')
                        .count(),
                };
                let closing = range.end.saturating_sub(delimiter);
                if cursor > closing {
                    return None;
                }
                end = end.min(closing);
            }
            _ => {}
        }
    }
    if needs_quote_boundary && !quote_boundary {
        return None;
    }
    // Duplicate reference definitions are consumed without body events and
    // only the first definition is retained by reference_definitions(). Requiring
    // text also protects those later definitions and raw HTML without excluding
    // tight-list items, which omit paragraph events.
    if !token_is_prose {
        return None;
    }
    // The parser leaves an unfinished link as prose. Once its destination is
    // being authored, inserting a canonical link would create nested syntax.
    // An unfinished destination can continue across a soft line break. Stay
    // within its parsed paragraph so a malformed link in an earlier block does
    // not disable completion in later prose.
    let before = &text[paragraph_start..cursor];
    for (destination, _) in before.rmatch_indices("](") {
        if let Some(label) = before[..destination].rfind('[') {
            let escaped = before[..label]
                .bytes()
                .rev()
                .take_while(|byte| *byte == b'\\')
                .count()
                % 2
                == 1;
            let closing_escaped = before[..destination]
                .bytes()
                .rev()
                .take_while(|byte| *byte == b'\\')
                .count()
                % 2
                == 1;
            if !escaped && !closing_escaped && paragraph_start + destination + 2 <= start {
                let mut depth = 1usize;
                let mut chars = before[destination + 2..].chars();
                while let Some(ch) = chars.next() {
                    match ch {
                        '\\' => {
                            chars.next();
                        }
                        '(' => depth += 1,
                        ')' => {
                            depth = depth.saturating_sub(1);
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                if depth > 0 {
                    return None;
                }
            }
        }
    }
    (!in_code(text, cursor)).then_some(end)
}

pub fn invocation_token(text: &str, cursor: usize, prefix: char) -> Option<MentionToken> {
    if cursor > text.len() || !text.is_char_boundary(cursor) {
        return None;
    }
    let start = text[..cursor]
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace() || matches!(c, '(' | '[' | '{' | '>'))
        .map_or(0, |(i, c)| i + c.len_utf8());
    if !text[start..cursor].starts_with(prefix) {
        return None;
    }
    let query = &text[start + 1..cursor];
    let name_grapheme = |grapheme: &str| {
        grapheme.chars().next().is_some_and(char::is_alphanumeric)
            || matches!(grapheme, "-" | "_" | ":" | ".")
    };
    if !query.graphemes(true).all(name_grapheme)
        || (prefix == '$' && query.starts_with(char::is_numeric))
    {
        return None;
    }
    // Scan from the token's start, including any combining marks after a caret
    // positioned between code points of the same grapheme.
    let end = text[start + 1..]
        .grapheme_indices(true)
        .find_map(|(offset, grapheme)| (!name_grapheme(grapheme)).then_some(start + 1 + offset))
        .unwrap_or(text.len());
    if text.get(end..end + 1) == Some("/") {
        return None;
    }
    let end = completion_markdown_end(text, start, cursor, end)?;
    Some(MentionToken {
        range: start..end,
        query: query.to_string(),
    })
}

pub fn in_code(text: &str, cursor: usize) -> bool {
    if cursor > text.len() || !text.is_char_boundary(cursor) {
        return false;
    }
    let mut parsed_code = Vec::new();
    let mut inline_start = None;
    let mut quote_depth = 0;
    for (event, range) in Parser::new_ext(
        text,
        Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH,
    )
    .into_offset_iter()
    {
        match event {
            Event::Start(Tag::BlockQuote(_)) => quote_depth += 1,
            Event::End(pulldown_cmark::TagEnd::BlockQuote(_)) => quote_depth -= 1,
            Event::Start(Tag::Paragraph | Tag::Heading { .. } | Tag::Item)
                if range.start <= cursor && cursor <= range.end =>
            {
                inline_start = Some(range.start);
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                if range.start <= cursor && cursor < range.end {
                    return true;
                }
                if range.end == cursor {
                    match kind {
                        pulldown_cmark::CodeBlockKind::Indented => return true,
                        pulldown_cmark::CodeBlockKind::Fenced(_) => {
                            let source = &text[range.clone()];
                            let mut lines = source.lines();
                            let opening = lines.next().unwrap_or_default().trim_start();
                            let delimiter = opening.chars().next().unwrap_or('`');
                            let count = opening.chars().take_while(|c| *c == delimiter).count();
                            let closed = lines.last().is_some_and(|line| {
                                // Offset ranges retain container prefixes on
                                // subsequent lines. Remove only quote markers
                                // belonging to the parsed enclosing containers.
                                let mut line = line.trim();
                                for _ in 0..quote_depth {
                                    if let Some(rest) = line.strip_prefix('>') {
                                        line = rest.trim_start();
                                    }
                                }
                                line.len() >= count && line.chars().all(|c| c == delimiter)
                            });
                            if !closed {
                                return true;
                            }
                        }
                    }
                }
                parsed_code.push(range);
            }
            Event::Text(_) if range.end == cursor => {
                // A fence-looking final line can still be literal code (for
                // example with four spaces of extra indentation). The parser's
                // body event is authoritative over the closing-line heuristic.
                if parsed_code
                    .last()
                    .is_some_and(|code: &Range<usize>| code.contains(&range.start))
                {
                    return true;
                }
            }
            Event::Code(_) => {
                if range.start <= cursor && cursor < range.end {
                    return true;
                }
                parsed_code.push(range);
            }
            _ => {}
        }
    }
    // Pulldown intentionally leaves unfinished inline spans as prose. Match
    // delimiter RUNS, skipping valid parsed spans (which can contain backticks
    // of another length), rather than counting individual backticks.
    let before = &text[..cursor];
    // Inline code cannot cross a paragraph/heading boundary. Whitespace-only
    // blank lines also terminate unfinished spans, including in CRLF drafts.
    let mut at = inline_start.unwrap_or_else(|| {
        let mut offset = 0;
        let mut start = 0;
        for line in before.split_inclusive('\n') {
            offset += line.len();
            if line.trim_matches([' ', '\t', '\r', '\n']).is_empty() {
                start = offset;
            }
        }
        start
    });
    let bytes = before.as_bytes();
    let mut delimiter = None;
    let mut code_index = 0;
    while at < bytes.len() {
        while parsed_code
            .get(code_index)
            .is_some_and(|range| range.end <= at)
        {
            code_index += 1;
        }
        if let Some(range) = parsed_code
            .get(code_index)
            .filter(|range| range.contains(&at))
        {
            at = range.end;
        } else if bytes[at] == b'\\' && delimiter.is_none() {
            at += 2;
        } else if bytes[at] == b'`' {
            let count = bytes[at..].iter().take_while(|b| **b == b'`').count();
            if delimiter == Some(count) {
                delimiter = None;
            } else if delimiter.is_none() {
                delimiter = Some(count);
            }
            at += count;
        } else {
            at += 1;
        }
    }
    delimiter.is_some()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_and_skills_complete_in_prose_and_preserve_suffixes() {
        for prefix in ['/', '$'] {
            for before in ["", "please ", "👋 请使用 ", "(", ">", "- "] {
                let text = format!("{before}{prefix}review) then continue");
                let cursor = before.len() + 3;
                let token = invocation_token(&text, cursor, prefix).unwrap();
                assert_eq!(token.query, "re");
                assert_eq!(&text[token.range], format!("{prefix}review"));
            }
            assert_eq!(
                invocation_token(&prefix.to_string(), 1, prefix)
                    .unwrap()
                    .query,
                ""
            );
        }
    }

    #[test]
    fn code_links_paths_escapes_and_arguments_are_not_completion_queries() {
        for prefix in ['/', '$'] {
            for text in [
                format!("    {prefix}review"),
                format!("\t{prefix}review"),
                format!("```\n{prefix}review"),
                format!("`{prefix}review`"),
                format!("`unfinished {prefix}review"),
                format!("\\{prefix}review"),
                format!("word{prefix}review"),
                format!("{prefix}repo/review"),
                format!("[label]({prefix}review"),
                format!("[label](\n{prefix}review"),
                format!("[{prefix}review](url)"),
                format!("<!-- {prefix}review -->"),
                format!("[id]: {prefix}review"),
            ] {
                let cursor = text.find("review").unwrap() + 6;
                assert!(invocation_token(&text, cursor, prefix).is_none(), "{text}");
            }
            for text in [
                format!("{prefix}review "),
                format!("{prefix}review argument"),
                format!("{prefix}review\t"),
            ] {
                assert!(
                    invocation_token(&text, text.len(), prefix).is_none(),
                    "{text}"
                );
            }
        }
        assert!(invocation_token("$100", 4, '$').is_none());
        assert!(invocation_token("https://host/path", 17, '/').is_none());
    }

    #[test]
    fn unicode_and_combining_marks_keep_complete_token_ranges() {
        for name in ["réview", "re\u{301}view", "确认", "レビュー"] {
            let text = format!("use ${name}");
            let token = invocation_token(&text, text.len(), '$').unwrap();
            assert_eq!(token.query, name);
            assert_eq!(&text[token.range], format!("${name}"));
        }
        let text = "$re\u{301}view";
        assert_eq!(invocation_token(text, 3, '$').unwrap().range, 0..text.len());
        assert!(invocation_token("$确认", 2, '$').is_none());
    }

    #[test]
    fn markdown_closing_delimiters_and_finished_blocks_are_preserved() {
        for delimiter in ["_", "**", "***", "~~"] {
            let text = format!("{delimiter}use $review{delimiter}");
            let closing = text.len() - delimiter.len();
            assert_eq!(
                invocation_token(&text, closing, '$').unwrap().range.end,
                closing
            );
            assert!(invocation_token(&text, text.len(), '$').is_none());
        }
        for text in [
            "`code` $review",
            "```\ncode\n```\n$review",
            "[label](url) $review",
            "[label](unfinished\n\n$review",
        ] {
            assert!(invocation_token(text, text.len(), '$').is_some(), "{text}");
        }
    }
}
