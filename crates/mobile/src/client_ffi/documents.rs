//! Read-only host documents shared by both phone clients.
use zeron_client::documents::{DocumentRead as HostRead, HostDocument};
use zeron_proto::file_links::{classify_file_link, document_link_target};
use zeron_proto::file_mentions::{FILE_MENTION_SCHEME, percent_decode_path};

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

/// A document read: the preview, or a file the viewer must first agree to
/// open because it lies beyond the chat's workspace (any file, in a chat in
/// the host's home folder). Read it again with `allow_outside` after they do.
#[derive(Clone, uniffi::Enum)]
pub enum DocumentRead {
    Ready { document: DocumentPreview },
    NeedsConfirmation { device_id: String, device_name: String, path: String },
}

/// Resolve a local file link without consulting the phone's filesystem.
/// `base_path` is the document containing a relative link, not a local directory.
// Relative results name files below the chat's folder.
#[uniffi::export]
pub fn document_link_path(url: String, base_path: Option<String>) -> Option<String> {
    resolve_link(&url, base_path.as_deref())
}

/// Transcript links follow the desktop transcript's file-link grammar; links
/// inside a document follow the desktop Markdown preview's: relative to the
/// document, never above its root and never absolute, so a document can't
/// reach past the folder it was opened from.
pub(crate) fn resolve_link(url: &str, base: Option<&str>) -> Option<String> {
    let url = url.trim();
    let Some(base) = base else {
        return classify_file_link(url).map(|link| link.path);
    };
    let path_part = url.split('#').next().unwrap_or(url);
    // A folder link stays an ordinary link rather than a file read, and a
    // home-relative one never names a file below this document.
    if path_part.ends_with('/') || path_part.starts_with('~') {
        return None;
    }
    // An absolute document keeps its links below the host's root.
    let (root, document) = match base.strip_prefix('/') {
        Some(rest) => ("/", rest),
        None => ("", base),
    };
    let (path, _anchor) = document_link_target(document, url).or_else(|| {
        // A literal `%` that is no escape (`100%.md`) keeps its raw spelling,
        // as in transcript links, however the platform encoded the URL.
        (path_part.contains('%') && percent_decode_path(path_part).is_none())
            .then(|| {
                let raw = format!("{}{}", path_part.replace('%', "%25"), &url[path_part.len()..]);
                document_link_target(document, &raw)
            })
            .flatten()
    })?;
    if path.chars().any(char::is_control) {
        return None;
    }
    // Mentions already name a path below the chat's folder.
    Some(if url.starts_with(FILE_MENTION_SCHEME) { path } else { format!("{root}{path}") })
}

fn preview(path: String, host: HostDocument) -> Result<DocumentPreview, zeron_client::ClientError> {
    use zeron_proto::WorkspaceReadOnlyReason as R;
    let Some(mut text) = host.text else {
        let message = match host.read_only_reason {
            Some(R::Binary) => "This file is binary; only text documents can be previewed.",
            Some(R::TooLarge) => "This file is too large to preview.",
            Some(R::UnsupportedEncoding) => "This document's text encoding is not supported.",
            Some(R::Symlink) => "Symbolic links cannot be previewed.",
            Some(R::PermissionDenied) => "The host denied access to this document.",
            Some(R::NotRegularFile) => "This link points to a folder or special file, not a document.",
            _ => "This document cannot be previewed.",
        };
        return Err(zeron_client::ClientError::HostError(message.into()));
    };
    let truncated = host.truncated || text.len() > PREVIEW_LIMIT;
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
        device_id: host.device_id,
        cwd: host.cwd,
    })
}

/// The phone's outcome for a host read; `device_name` labels the prompt.
fn document_read(
    path: String,
    read: HostRead,
    device_name: impl FnOnce(&str) -> Option<String>,
) -> Result<DocumentRead, zeron_client::ClientError> {
    Ok(match read {
        HostRead::Ready(host) => DocumentRead::Ready { document: preview(path, host)? },
        HostRead::Outside { device_id, path } => DocumentRead::NeedsConfirmation {
            device_name: device_name(&device_id).unwrap_or_else(|| device_id.clone()),
            device_id,
            path,
        },
    })
}

#[uniffi::export]
impl CoreClient {
    pub async fn read_document(
        &self,
        chat_id: String,
        path: String,
        allow_outside: bool,
    ) -> CoreResult<DocumentRead> {
        let client = self.client.clone();
        on_runtime(async move {
            let read = client.read_chat_file(&chat_id, &path, PREVIEW_LIMIT, allow_outside).await?;
            let workspace = client.workspace();
            document_read(path, read, |id| workspace.device(id).map(|device| device.name.clone()))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_links_use_the_desktop_grammar() {
        for (link, expected) in [
            ("/home/user/docs/readme.md#L10", "/home/user/docs/readme.md"),
            ("file:///home/user/a%20b.md:12:3", "/home/user/a b.md"),
            ("zeron-file:docs/a.md", "docs/a.md"),
            ("a%23b.md", "a#b.md"),
            ("%E4%B8%AD%E6%96%87%20guide.md", "中文 guide.md"),
            // Android hands over the raw text, iOS the URL-encoded one.
            ("100%.md", "100%.md"),
            ("100%25.md", "100%.md"),
            ("a%xx.md", "a%xx.md"),
        ] {
            assert_eq!(resolve_link(link, None).as_deref(), Some(expected), "{link}");
        }
        for link in [
            "https://example.com/a.md",
            "mailto:a@b.com",
            "javascript:alert(1)",
            "file://localhost/home/user/a.md",
            "file://server/a.md",
            "//server/a.md",
            "../secret.md",
            "%2e%2e/secret.md",
            "a%00.md",
            "#heading",
            "docs/",
            "a\\b.md",
            "LICENSE",
            "~/.ssh/id_ed25519",
        ] {
            assert_eq!(resolve_link(link, None), None, "{link}");
        }
    }

    #[test]
    fn document_links_resolve_beside_the_document_and_stay_below_its_root() {
        for (link, base, expected) in [
            ("../README.md", "docs/guide.md", "README.md"),
            ("./more.md", "/repo/docs/guide.md", "/repo/docs/more.md"),
            ("../LICENSE", "docs/guide.md", "LICENSE"),
            ("next", "/repo/docs/guide.md", "/repo/docs/next"),
            ("a%20b.md#part", "docs/guide.md", "docs/a b.md"),
            ("zeron-file:src/lib.rs", "/repo/docs/guide.md", "src/lib.rs"),
            // A self-link names the open document, so the viewer stays put.
            ("guide.md#usage", "docs/guide.md", "docs/guide.md"),
            ("#usage", "/repo/docs/guide.md", "/repo/docs/guide.md"),
            ("100%.md", "docs/guide.md", "docs/100%.md"),
            ("100%25.md", "docs/guide.md", "docs/100%.md"),
        ] {
            assert_eq!(resolve_link(link, Some(base)).as_deref(), Some(expected), "{link}");
        }
        for (link, base) in [
            ("../../.ssh/id_ed25519", "docs/guide.md"),
            ("../secret.md", "guide.md"),
            ("/home/user/.ssh/id_ed25519", "docs/guide.md"),
            ("file:///home/user/.ssh/id_ed25519", "docs/guide.md"),
            ("~/.ssh/id_ed25519", "docs/guide.md"),
            ("%2Fetc/passwd", "docs/guide.md"),
            ("https://example.com/a.md", "docs/guide.md"),
            ("mailto:a@b.com", "docs/guide.md"),
            ("docs/", "guide.md"),
            ("a\\b.md", "guide.md"),
            ("a%0Ab.md", "guide.md"),
        ] {
            assert_eq!(resolve_link(link, Some(base)), None, "{link}");
        }
    }

    fn host(text: Option<String>) -> HostDocument {
        HostDocument {
            device_id: "ubuntu".into(),
            cwd: None,
            text,
            read_only_reason: Some(zeron_proto::WorkspaceReadOnlyReason::Binary),
            truncated: false,
        }
    }

    /// Outside files, and every file of a chat in the host's home folder,
    /// come back from the host as `Outside`: the phone asks, never refuses.
    #[test]
    fn outside_reads_become_a_confirmation_naming_device_and_path() {
        let read = HostRead::Outside { device_id: "ubuntu".into(), path: "/home/me/notes/plan.md".into() };
        match document_read("~/notes/plan.md".into(), read, |id| (id == "ubuntu").then(|| "Build box".into())).unwrap() {
            DocumentRead::NeedsConfirmation { device_id, device_name, path } => {
                assert_eq!((device_id.as_str(), device_name.as_str()), ("ubuntu", "Build box"));
                assert_eq!(path, "/home/me/notes/plan.md");
            }
            DocumentRead::Ready { .. } => panic!("opened without consent"),
        }
        // An unknown device is still named, by its id.
        let read = HostRead::Outside { device_id: "ubuntu".into(), path: "/etc/hosts".into() };
        assert!(matches!(
            document_read("/etc/hosts".into(), read, |_| None).unwrap(),
            DocumentRead::NeedsConfirmation { device_name, .. } if device_name == "ubuntu"
        ));
        let ready = document_read("a.md".into(), HostRead::Ready(host(Some("# A".into()))), |_| None).unwrap();
        assert!(matches!(ready, DocumentRead::Ready { document } if document.markdown && document.text == "# A"));
    }

    #[test]
    fn text_limits_and_binary_errors_are_visible() {
        let p = preview("/repo/README.MD".into(), host(Some("中".repeat(PREVIEW_LIMIT)))).unwrap();
        assert!(p.truncated && p.markdown && p.text.len() <= PREVIEW_LIMIT);
        assert_eq!(p.path, "/repo/README.MD");
        assert!(preview("a.pdf".into(), host(None)).err().unwrap().to_string().contains("binary"));
        assert!(preview("empty.md".into(), host(Some(String::new()))).unwrap().text.is_empty());
        let more = HostDocument { truncated: true, ..host(Some("head".into())) };
        assert!(preview("log.txt".into(), more).unwrap().truncated);
    }
}
