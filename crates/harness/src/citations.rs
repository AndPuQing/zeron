//! Links carried as structured content, normalized to the same Markdown that
//! every frontend already renders. A source ID is never treated as a URL.
use serde_json::Value;

fn escape_label(label: &str) -> String {
    let mut escaped = String::new();
    for ch in label.chars() {
        if ch.is_control() {
            escaped.push(' ');
            continue;
        }
        if ch.is_ascii_punctuation() {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

pub(crate) fn resource_link(uri: &str, label: &str) -> Option<String> {
    if uri.is_empty() || uri.len() > 16 * 1024 || uri.chars().any(char::is_control) {
        return None;
    }
    let valid = if uri.starts_with("https://") || uri.starts_with("http://") {
        reqwest::Url::parse(uri).is_ok_and(|url| {
            url.host_str().is_some() && url.username().is_empty() && url.password().is_none()
        })
    } else {
        zeron_proto::file_links::classify_file_link(uri).is_some()
    };
    if !valid {
        return None;
    }
    let mut target = String::new();
    for ch in uri.chars() {
        if matches!(ch, '<' | '>' | '\\' | ' ' | '"' | '\'') {
            for byte in ch.to_string().bytes() {
                target.push_str(&format!("%{byte:02X}"));
            }
        } else if ch == '&' {
            // CommonMark decodes entities in link destinations. Protect a
            // literal &copy; (or any query separator) without changing its URI.
            target.push_str("&amp;");
        } else {
            target.push(ch);
        }
    }
    let label = if label.trim().is_empty() { uri } else { label };
    Some(format!("[{}](<{target}>)", escape_label(label)))
}

pub(crate) fn claude_citation(citation: &Value) -> Option<String> {
    let url = citation.get("url")?.as_str()?;
    let label = citation
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Source");
    resource_link(url, label).map(|link| format!(" {link}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn structured_citation_destinations_and_labels_are_not_markdown_instructions() {
        assert_eq!(
            resource_link("file:///docs/a file.pdf#L2", "**a [file]**"),
            Some(r"[\*\*a \[file\]\*\*](<file:///docs/a%20file.pdf#L2>)".into())
        );
        assert_eq!(
            resource_link("https://example.com?q=%20&x=(1)", "a\nlabel"),
            Some("[a label](<https://example.com?q=%20&amp;x=(1)>)".into())
        );
        for target in [
            "javascript:alert(1)",
            "data:text/html,hi",
            "file://server/a.md",
            "../secret.md",
            "https://user:pass@example.com",
            "https://example.com\n",
        ] {
            assert!(resource_link(target, "name").is_none(), "{target}");
        }
        assert!(claude_citation(&serde_json::json!({"type":"page_location","document_index":0,"document_title":"report.pdf"})).is_none());
    }
}
