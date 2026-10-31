//! Local reply-image references shared by desktop and mobile. These only
//! select candidates; the owning engine enforces the chat-folder jail.

use serde::{Deserialize, Serialize};

const PREFIX: &str = "zeron-chat-image:";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatImage {
    pub chat_id: String,
    pub path: String,
}

impl ChatImage {
    pub fn reference(&self) -> String {
        format!(
            "{PREFIX}{}",
            serde_json::to_string(self).expect("image reference")
        )
    }

    pub fn is_reference(value: &str) -> bool {
        value.starts_with(PREFIX)
    }

    pub fn parse(value: &str) -> Option<Self> {
        let image: Self = serde_json::from_str(value.strip_prefix(PREFIX)?).ok()?;
        (!image.chat_id.is_empty() && markdown_image(&image.path, None).is_some()).then_some(image)
    }
}

pub fn markdown_image(source: &str, cwd: Option<&str>) -> Option<(String, &'static str)> {
    let source = source.trim();
    let remote = |s: &str| {
        let b = s.as_bytes();
        let sep = |b: Option<&u8>| matches!(b, Some(b'/' | b'\\'));
        (sep(b.first()) && sep(b.get(1)))
            || s.starts_with(r"\??\")
            || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && !sep(b.get(2)))
    };
    let scheme = source.split_once(':').is_some_and(|(s, _)| {
        s.len() > 1
            && s.starts_with(|c: char| c.is_ascii_alphabetic())
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    });
    if source.is_empty() || source.starts_with('~') || scheme || remote(source) {
        return None;
    }
    let mime = match source.rsplit_once('.')?.1.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return None,
    };
    let absolute = |s: &str| {
        let b = s.as_bytes();
        s.starts_with(['/', '\\'])
            || (b.len() >= 3
                && b[0].is_ascii_alphabetic()
                && b[1] == b':'
                && matches!(b[2], b'/' | b'\\'))
    };
    let mut source = source;
    while let Some(rest) = source.strip_prefix("./") {
        source = rest;
    }
    let path = match cwd.filter(|_| !absolute(source)) {
        Some(cwd) if cwd.contains('\\') => format!(
            "{}\\{}",
            cwd.trim_end_matches(['/', '\\']),
            source.replace('/', "\\")
        ),
        Some(cwd) => format!("{}/{source}", cwd.trim_end_matches('/')),
        None => source.to_owned(),
    };
    // Keep `..` intact: removing it before the host's lexical jail could
    // turn an outside path into a different, permitted file.
    (absolute(&path) && !remote(&path)).then_some((path, mime))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_images_keep_host_paths_and_refuse_urls() {
        assert_eq!(
            markdown_image("../shot.png", Some("/repo")),
            Some(("/repo/../shot.png".into(), "image/png"))
        );
        assert_eq!(
            markdown_image("shots/a.gif", Some(r"C:\repo")),
            Some((r"C:\repo\shots\a.gif".into(), "image/gif"))
        );
        for path in [
            "https://x/a.png",
            "file:///a.png",
            "data:image/png,x",
            "~/a.png",
            "//host/a.png",
            r"\\host\a.png",
            "C:a.png",
            "a.svg",
        ] {
            assert!(markdown_image(path, Some("/repo")).is_none(), "{path}");
        }
        assert!(markdown_image("a.png", Some("~/repo")).is_none());
        assert!(markdown_image("a.png", None).is_none());
        assert!(markdown_image("a.png", Some("//host/share")).is_none());
    }

    #[test]
    fn references_round_trip_without_confusing_paths_and_urls() {
        let image = ChatImage {
            chat_id: "chat#1".into(),
            path: "/repo/截图 #1.png".into(),
        };
        assert_eq!(ChatImage::parse(&image.reference()), Some(image));
        assert!(ChatImage::parse("zeron-chat-image:bad").is_none());
    }
}
