//! Local reply-image references shared by desktop and mobile. These only
//! select candidates; the owning engine enforces the chat-folder jail.

use serde::{Deserialize, Serialize};

/// Bytes per `ReadChatImageChunk` reply. A multiple of 3 so independent
/// base64 chunks concatenate losslessly; readers derive their chunk budget
/// from it and [`crate::IMAGE_MAX_BYTES`].
pub const CHAT_IMAGE_CHUNK_BYTES: usize = 45_000;
const _: () = assert!(CHAT_IMAGE_CHUNK_BYTES % 3 == 0);

/// An image a reply in `chat_id` names: a host path that only
/// `ReadChatImageChunk`, under that chat's folder jail, may read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatImage {
    pub chat_id: String,
    pub path: String,
}

/// UNC (`\\host\share`, `//host/share`), device and verbatim (`\\.\`,
/// `\\?\`, `\??\`) and drive-relative (`C:x.png`) forms. Agent output names
/// them freely, and merely opening one can reach another host (on Windows
/// with the user's NTLM credentials), so they are never previewed. The
/// engine's jail refuses them as well.
pub fn is_remote_or_device_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    let separator = |b: Option<&u8>| matches!(b, Some(b'/' | b'\\'));
    (separator(bytes.first()) && separator(bytes.get(1)))
        || path.starts_with(r"\??\")
        || (bytes.len() >= 2
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && !separator(bytes.get(2)))
}

/// Whether `path` is absolute on its host, whichever OS that is (`/a`, `\a`,
/// `C:\a`, `C:/a`).
pub fn is_host_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with(['/', '\\'])
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
}

/// Absolute path on the owning host. Relative paths join the chat's cwd in
/// that host's separator style (the UI may run on another OS).
pub fn resolve_path(path: &str, cwd: Option<&str>) -> String {
    let absolute = is_host_absolute(path);
    match cwd.filter(|_| !absolute) {
        Some(cwd) if cwd.contains('\\') => format!(
            "{}\\{}",
            cwd.trim_end_matches(['/', '\\']),
            path.trim_start_matches("./").replace('/', "\\")
        ),
        Some(cwd) => format!(
            "{}/{}",
            cwd.trim_end_matches('/'),
            path.trim_start_matches("./")
        ),
        None => path.to_owned(),
    }
}

/// `http:`, `file:`, `data:`, `javascript:`…; a drive letter (`C:\`) is not one.
fn has_scheme(source: &str) -> bool {
    source.split_once(':').is_some_and(|(scheme, _)| {
        scheme.len() > 1
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

fn raster_mime(path: &str) -> Option<&'static str> {
    let (_, ext) = path.rsplit_once('.')?;
    match ext.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

/// The host path and type of an image a reply names, when it may be drawn.
/// `cwd` is the chat's folder on its host.
pub fn markdown_image(source: &str, cwd: Option<&str>) -> Option<(String, &'static str)> {
    let source = source.trim();
    if source.is_empty()
        || source.starts_with('~')
        || has_scheme(source)
        || is_remote_or_device_path(source)
    {
        return None;
    }
    let mime = raster_mime(source)?;
    // Keep `..` intact: removing it before the host's lexical jail could
    // turn an outside path into a different, permitted file.
    let path = resolve_path(source, cwd);
    // A relative path needs a folder to resolve against; a home-relative
    // folder (a projectless chat's) can't be expanded here.
    (is_host_absolute(&path) && !is_remote_or_device_path(&path)).then_some((path, mime))
}

/// Whether `path` (as [`markdown_image`] resolved it) lies strictly inside
/// `cwd` once `.` and `..` are resolved lexically: the only images
/// `ReadChatImageChunk` can serve. A `~/…` folder (a projectless chat's
/// scratch folder) is matched by its whole path below some home, since this
/// client doesn't know the host's. Fails closed for no folder, the host's
/// home itself, and network folders. The host still enforces its own jail
/// (links, canonical spellings).
pub fn in_chat_folder(path: &str, cwd: Option<&str>) -> bool {
    let Some(cwd) = cwd
        .map(str::trim)
        .filter(|cwd| !crate::projectless::is_home_cwd(cwd))
    else {
        return false;
    };
    if !is_host_absolute(path) || is_remote_or_device_path(cwd) || is_remote_or_device_path(path) {
        return false;
    }
    // Windows hosts compare case-insensitively.
    let windows = cwd.contains('\\') || cwd.as_bytes().get(1) == Some(&b':');
    let components = |path: &str| {
        let mut out = Vec::new();
        for part in path.split(['/', '\\']) {
            match part {
                "" | "." => {}
                ".." => {
                    out.pop()?;
                }
                part if windows => out.push(part.to_ascii_lowercase()),
                part => out.push(part.to_owned()),
            }
        }
        Some(out)
    };
    let Some(file) = components(path) else {
        return false;
    };
    if let Some(below_home) = cwd.strip_prefix('~') {
        let Some(folder) = components(below_home).filter(|folder| !folder.is_empty()) else {
            return false;
        };
        // Some home before the folder, and the file inside it.
        return (1..file.len().saturating_sub(folder.len())).any(|at| file[at..].starts_with(&folder));
    }
    if !is_host_absolute(cwd) {
        return false;
    }
    components(cwd).is_some_and(|folder| file.len() > folder.len() && file.starts_with(&folder))
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
        // `./` marks a relative path: what follows never becomes absolute.
        assert_eq!(
            markdown_image(".//shots/a.png", Some("/repo")),
            Some(("/repo//shots/a.png".into(), "image/png"))
        );
        assert_eq!(
            markdown_image("./C:/a.png", Some("/repo")),
            Some(("/repo/C:/a.png".into(), "image/png"))
        );
    }

    #[test]
    fn only_paths_inside_the_chat_folder_can_be_served() {
        for path in ["/repo/a.png", "/repo/shots/../a.png", "/repo//shots/./a.png"] {
            assert!(in_chat_folder(path, Some("/repo/")), "{path}");
        }
        for path in ["/tmp/a.png", "/repo/../a.png", "/repo", "/repository/a.png", "//host/repo/a.png"] {
            assert!(!in_chat_folder(path, Some("/repo")), "{path}");
        }
        assert!(in_chat_folder(r"c:\Repo\Shots\a.png", Some(r"C:\repo")));
        assert!(!in_chat_folder(r"D:\repo\a.png", Some(r"C:\repo")));
        assert!(!in_chat_folder("/Repo/a.png", Some("/repo")));
        for cwd in [None, Some("~"), Some(""), Some("~/repo"), Some("~/.zerun/scratch/x"), Some("//host/share")] {
            assert!(!in_chat_folder("/repo/a.png", cwd), "{cwd:?}");
        }
        // A projectless chat's scratch folder sits below a home this client doesn't know.
        let scratch = Some("~/.zerun/scratch/2026-10-25-a1b2c3d4");
        assert!(in_chat_folder("/Users/u/.zerun/scratch/2026-10-25-a1b2c3d4/shot.png", scratch));
        assert!(in_chat_folder(r"C:\Users\u\.zerun\scratch\2026-10-25-a1b2c3d4\a.png", scratch));
        for path in [
            "/Users/u/.zerun/scratch/2026-10-25-ffffffff/shot.png",
            "/Users/u/.zerun/scratch/2026-10-25-a1b2c3d4",
            "/.zerun/scratch/2026-10-25-a1b2c3d4/shot.png",
            "/Users/u/.zerun/scratch/2026-10-25-a1b2c3d4/../shot.png",
        ] {
            assert!(!in_chat_folder(path, scratch), "{path}");
        }
    }
}
