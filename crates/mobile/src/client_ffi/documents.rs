//! Read-only host documents shared by both phone clients.
use super::{CoreClient, CoreResult, on_runtime};

const PREVIEW_LIMIT: usize = 2 * 1024 * 1024;

#[derive(Clone, uniffi::Record)]
pub struct DocumentPreview {
    /// Requested path, retained even when the host returns a workspace-relative path.
    pub path: String,
    pub text: String,
    pub markdown: bool,
    pub truncated: bool,
    pub device_id: String,
    pub cwd: Option<String>,
}

/// Resolve a local file link without consulting the phone's filesystem.
/// `base_path` is the document containing a relative link, not a local directory.
#[uniffi::export]
pub fn document_link_path(url: String, base_path: Option<String>) -> Option<String> {
    resolve_link(&url, base_path.as_deref())
}

pub(crate) fn resolve_link(url: &str, base: Option<&str>) -> Option<String> {
    let url = url.trim();
    let (raw, explicit) = if let Some(path) = url.strip_prefix("zeron-file:") {
        (path, true)
    } else if let Some(path) = url.strip_prefix("file://") {
        let path = path
            .strip_prefix("localhost/")
            .map(|p| format!("/{p}"))
            .unwrap_or_else(|| path.to_owned());
        if !path.starts_with('/') || path.starts_with("//") {
            return None;
        }
        return resolve_path(&path, None, true);
    } else {
        (url, false)
    };
    resolve_path(raw, base, explicit)
}

fn resolve_path(raw: &str, base: Option<&str>, explicit: bool) -> Option<String> {
    // Split location syntax before decoding: %23 belongs to the filename.
    let mut raw = raw.split('#').next()?;
    for _ in 0..2 {
        if let Some((path, suffix)) = raw.rsplit_once(':') {
            if !suffix.is_empty()
                && suffix
                    .split('-')
                    .all(|s| s.parse::<u32>().is_ok_and(|n| n > 0))
            {
                raw = path;
            }
        }
    }
    if raw.is_empty() {
        return None;
    }
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    let path = String::from_utf8(decoded).ok()?;
    if path.chars().any(char::is_control)
        || path.contains(['\\', ':', '?'])
        || path.starts_with("//")
        || path.starts_with('~')
        || path.ends_with('/')
    {
        return None;
    }
    if !explicit && !path.rsplit('/').next()?.contains('.') {
        return None;
    }
    let joined = if !path.starts_with('/') {
        match base.and_then(|b| b.rsplit_once('/')) {
            Some((parent, _)) => format!("{parent}/{path}"),
            None => path,
        }
    } else {
        path
    };
    let absolute = joined.starts_with('/');
    let mut parts = Vec::new();
    for part in joined.split('/') {
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
    Some(format!(
        "{}{}",
        if absolute { "/" } else { "" },
        parts.join("/")
    ))
}

fn preview(
    path: String,
    device_id: String,
    cwd: Option<String>,
    file: zeron_proto::WorkspaceFileText,
) -> Result<DocumentPreview, zeron_client::ClientError> {
    use zeron_proto::WorkspaceReadOnlyReason as R;
    let Some(mut text) = file.text else {
        let message = match file.read_only_reason {
            Some(R::Binary) => "This file is binary; only text documents can be previewed.",
            Some(R::TooLarge) => "This file is too large to preview.",
            Some(R::UnsupportedEncoding) => "This document's text encoding is not supported.",
            Some(R::Symlink) => "Symbolic links cannot be previewed.",
            Some(R::PermissionDenied) => "The host denied access to this document.",
            Some(R::NotRegularFile) => "This path is not a regular file.",
            _ => "This document cannot be previewed.",
        };
        return Err(zeron_client::ClientError::HostError(message.into()));
    };
    let truncated = file.truncated || text.len() > PREVIEW_LIMIT;
    if text.len() > PREVIEW_LIMIT {
        let mut end = PREVIEW_LIMIT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    let markdown = path
        .rsplit_once('.')
        .is_some_and(|(_, ext)| matches!(ext.to_ascii_lowercase().as_str(), "md" | "markdown"));
    Ok(DocumentPreview {
        path,
        text,
        markdown,
        truncated,
        device_id,
        cwd,
    })
}

#[uniffi::export]
impl CoreClient {
    pub async fn read_document(
        &self,
        chat_id: String,
        path: String,
    ) -> CoreResult<DocumentPreview> {
        let client = self.client.clone();
        on_runtime(async move {
            let host = client.read_chat_file(&chat_id, &path).await?;
            preview(path, host.device_id, host.cwd, host.file)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_keep_host_paths_and_resolve_document_neighbors() {
        for (link, base, expected) in [
            (
                "/home/user/docs/readme.md#L10",
                None,
                "/home/user/docs/readme.md",
            ),
            ("file:///home/user/a%20b.md:12:3", None, "/home/user/a b.md"),
            ("file://localhost/home/user/a.md", None, "/home/user/a.md"),
            ("zeron-file:docs/a.md", None, "docs/a.md"),
            ("../README.md", Some("docs/guide.md"), "README.md"),
            (
                "./more.md",
                Some("/repo/docs/guide.md"),
                "/repo/docs/more.md",
            ),
            ("a%23b.md", None, "a#b.md"),
        ] {
            assert_eq!(
                resolve_link(link, base).as_deref(),
                Some(expected),
                "{link}"
            );
        }
    }

    #[test]
    fn non_file_and_unsafe_links_are_not_host_reads() {
        for link in [
            "https://example.com/a.md",
            "mailto:a@b.com",
            "javascript:alert(1)",
            "file://server/a.md",
            "//server/a.md",
            "../secret.md",
            "%2e%2e/secret.md",
            "a%00.md",
            "a%xx.md",
            "#heading",
            "docs/",
            "a\\b.md",
        ] {
            assert_eq!(resolve_link(link, None), None, "{link}");
        }
    }

    #[test]
    fn text_limits_and_binary_errors_are_visible() {
        let file = |text| zeron_proto::WorkspaceFileText {
            checkout_id: String::new(),
            path: "README.md".into(),
            text,
            content_hash: None,
            size: 0,
            modified_at: None,
            encoding: zeron_proto::WorkspaceTextEncoding::Utf8,
            line_ending: None,
            read_only_reason: Some(zeron_proto::WorkspaceReadOnlyReason::Binary),
            truncated: false,
        };
        let p = preview(
            "/repo/README.MD".into(),
            "ubuntu".into(),
            None,
            file(Some("中".repeat(PREVIEW_LIMIT))),
        )
        .unwrap();
        assert!(p.truncated && p.markdown && p.text.len() <= PREVIEW_LIMIT);
        assert_eq!(p.path, "/repo/README.MD");
        assert!(
            preview("a.pdf".into(), "ubuntu".into(), None, file(None))
                .err()
                .unwrap()
                .to_string()
                .contains("binary")
        );
        assert!(
            preview(
                "empty.md".into(),
                "ubuntu".into(),
                None,
                file(Some(String::new()))
            )
            .unwrap()
            .text
            .is_empty()
        );
    }
}
