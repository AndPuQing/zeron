//! Bounded, inline raster payload extraction shared by harness adapters.
use serde_json::Value;

/// Images kept from one tool result. Matches the chip preview cap.
const MAX_TOOL_RESULT_IMAGES: usize = 4;

fn is_base64_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=')
}

/// Copy `data` when it looks like base64 within the image size cap.
/// Whitespace is removed. Anything else is dropped before it is stored.
pub(crate) fn compact_base64(data: &str) -> Option<String> {
    let limit = zeron_proto::IMAGE_MAX_BYTES.div_ceil(3) * 4;
    let mut count = 0usize;
    for byte in data.bytes() {
        if byte.is_ascii_whitespace() {
            continue;
        }
        if !is_base64_byte(byte) {
            return None;
        }
        count += 1;
        if count > limit {
            return None;
        }
    }
    if count == 0 {
        return None;
    }
    if count == data.len() {
        return Some(data.to_owned());
    }
    Some(
        data.bytes()
            .filter(|byte| !byte.is_ascii_whitespace())
            .map(char::from)
            .collect(),
    )
}

fn push_tool_image(out: &mut Vec<zeron_proto::ToolResultImage>, name: &str, data: &str) {
    if out.len() >= MAX_TOOL_RESULT_IMAGES {
        return;
    }
    let Some(data_base64) = compact_base64(data) else {
        return;
    };
    out.push(zeron_proto::ToolResultImage {
        name: name.to_owned(),
        data_base64,
    });
}

/// Image blocks inside a tool result's `content` array.
pub(crate) fn images_in_content(
    content: Option<&Value>,
    name: &str,
) -> Vec<zeron_proto::ToolResultImage> {
    let mut out = Vec::new();
    let Some(blocks) = content.and_then(Value::as_array) else {
        return out;
    };
    for block in blocks {
        // ACP wraps a ContentBlock in {type: "content", content: ...}.
        let block = if block["type"] == "content" {
            &block["content"]
        } else {
            block
        };
        if out.len() >= MAX_TOOL_RESULT_IMAGES {
            break;
        }
        if block.get("type").and_then(Value::as_str) != Some("image") {
            continue;
        }
        if let Some(kind) = block.pointer("/source/type").and_then(Value::as_str)
            && kind != "base64"
        {
            continue;
        }
        let Some(data) = block
            .pointer("/source/data")
            .and_then(Value::as_str)
            .or_else(|| block.get("data").and_then(Value::as_str))
        else {
            continue;
        };
        push_tool_image(&mut out, name, data);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn image_payloads_cover_mcp_acp_and_native_blocks_without_fetching_urls() {
        let content = json!([
            {"type":"image", "data":"YWJj", "mimeType":"image/png"},
            {"type":"content", "content":{"type":"image", "data":"YWJj", "mimeType":"image/png"}},
            {"type":"image", "source":{"type":"base64", "data":"YW Jj\n"}},
            {"type":"image", "source":{"type":"url", "url":"https://example.com/a.png"}},
            {"type":"text", "text":"YWJj"}
        ]);
        let images = images_in_content(Some(&content), "shot.png");
        assert_eq!(images.len(), 3);
        assert!(images.iter().all(|i| i.data_base64 == "YWJj"));
        let many = json!(
            (0..8)
                .map(|_| json!({"type":"image", "data":"YWJj"}))
                .collect::<Vec<_>>()
        );
        assert_eq!(images_in_content(Some(&many), "shot.png").len(), 4);
        assert!(compact_base64("!").is_none());
        assert!(
            compact_base64(&"A".repeat(zeron_proto::IMAGE_MAX_BYTES.div_ceil(3) * 4 + 1)).is_none()
        );
    }
}
