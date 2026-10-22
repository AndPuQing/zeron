//! Cross-chat message attribution, shared by the desktop transcript and the
//! mobile transcript layout.
//!
//! `send_message` prefixes an agent-to-agent message with a routing header so
//! the receiving agent knows where a reply goes:
//!
//! ```text
//! [Message from Zeron chat <title> (<short id>). Reply to it with the Zeron
//! `send_message` tool, chat <full chat id>.]\n\n<body>
//! ```
//!
//! The header is instructions for the agent, not something a human wants to
//! read in their own bubble: the full routing id and the reply instruction are
//! replaced by a concise `Message from <name>` line, which is also what the
//! collapsed preview must show. The stored prompt, the delivered text and the
//! reply target never change — this is display projection only.
//!
//! Recognition is strict: the complete known wrapper, or nothing. A malformed
//! prefix, a truncated header, or ordinary user text that merely mentions the
//! tool is returned untouched, so a stray `[` or the words `Reply to it` never
//! lose content.

/// The human-facing projection of one attributed message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttributedMessage<'a> {
    /// The sender as the header labels it: `title (short id)`, the title alone,
    /// or the short id alone. Whatever the header carries is what readers see,
    /// so a short id stays usable for recognition.
    pub name: &'a str,
    /// Everything after the header's blank line, verbatim.
    pub body: &'a str,
}

const PREFIX: &str = "[Message from Zeron chat ";
const REPLY_INSTRUCTION: &str = ". Reply to it with the Zeron `send_message` tool, chat ";

/// Parse the complete `send_message` attribution wrapper.
pub fn attributed_message(text: &str) -> Option<AttributedMessage<'_>> {
    let rest = text.strip_prefix(PREFIX)?;
    let (header, body) = rest.split_once("]\n\n")?;
    let (label, id) = header.rsplit_once(REPLY_INSTRUCTION)?;
    let id = id.strip_suffix('.')?;
    // A routing id is a plain opaque token. Whitespace or brackets mean this
    // was never the header we emit, so leave the text alone.
    if id.is_empty()
        || id
            .chars()
            .any(|c| c.is_whitespace() || c == '[' || c == ']')
    {
        return None;
    }
    if label.is_empty() {
        return None;
    }
    // Older headers carried the full id inside the label; newer ones end with
    // the short id instead. Only the exact full-id duplicate is redundant.
    let name = label
        .strip_suffix(&format!(" ({id})"))
        .unwrap_or(label)
        .trim();
    if name.is_empty() {
        return None;
    }
    Some(AttributedMessage { name, body })
}

/// `Message from <name>\n\n<body>` for an attributed message, `None` otherwise.
pub fn attributed_message_display(text: &str) -> Option<String> {
    let parsed = attributed_message(text)?;
    Some(format!("Message from {}\n\n{}", parsed.name, parsed.body))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = "a1b2c3d4-1111-4222-8333-444455556666";

    fn wrapper(label: &str, body: &str) -> String {
        format!(
            "[Message from Zeron chat {label}. Reply to it with the Zeron `send_message` tool, chat {FULL}.]\n\n{body}"
        )
    }

    #[test]
    fn short_id_label_is_kept_and_routing_id_hidden() {
        let text = wrapper(
            "示例会话标题 · 本机 dsh (a1b2c3d4)",
            "正文第一行\n\n第二段",
        );
        let display = attributed_message_display(&text).expect("known wrapper");
        assert_eq!(
            display,
            "Message from 示例会话标题 · 本机 dsh (a1b2c3d4)\n\n正文第一行\n\n第二段"
        );
        assert!(!display.contains(FULL), "{display}");
        assert!(!display.contains("send_message"), "{display}");
    }

    #[test]
    fn title_only_and_short_id_only_labels_survive() {
        for label in ["仅标题", "a1b2c3d4"] {
            let text = wrapper(label, "body");
            let display = attributed_message_display(&text).expect("known wrapper");
            assert_eq!(display, format!("Message from {label}\n\nbody"));
        }
    }

    #[test]
    fn full_id_label_is_collapsed_to_the_name() {
        let text = wrapper(&format!("Named chat ({FULL})"), "body");
        let display = attributed_message_display(&text).expect("known wrapper");
        assert_eq!(display, "Message from Named chat\n\nbody");
    }

    #[test]
    fn names_with_punctuation_and_brackets_are_preserved() {
        for label in [
            "Fix [draft] (v2)",
            "feat: a, b; c",
            "问号？句号。括号（全角）",
            "a (b) (c)",
        ] {
            let text = wrapper(label, "body");
            let display = attributed_message_display(&text).expect("known wrapper");
            assert_eq!(display, format!("Message from {label}\n\nbody"));
        }
    }

    #[test]
    fn ordinary_and_malformed_text_falls_back_untouched() {
        for text in [
            "just a normal message",
            "see [Message from Zeron chat x] for context",
            "Reply to it with the Zeron `send_message` tool",
            "[Message from Zeron chat 标题", // no header terminator
            "[Message from Zeron chat 标题]\n\nbody", // no reply instruction
            "[Message from Zeron chat . Reply to it with the Zeron `send_message` tool, chat id.]\n\nb",
            "[Message from Zeron chat name. Reply to it with the Zeron `send_message` tool, chat .]\n\nb",
            "[Message from Zeron chat name. Reply to it with the Zeron `send_message` tool, chat has space.]\n\nb",
            "[Message from Zeron chat name. Reply to it with the Zeron `send_message` tool, chat id]", // no trailing dot
            "",
        ] {
            assert_eq!(
                attributed_message_display(text),
                None,
                "should not match: {text:?}"
            );
            assert_eq!(attributed_message(text), None, "should not match: {text:?}");
        }
    }

    #[test]
    fn body_keeps_attachment_trailers_and_mentions_verbatim() {
        let body = "Look at [mod.rs](zeron-file:crates/mobile/src/layout/mod.rs)\n\n\
                    Attached images (local files — open them to view):\n- /tmp/uploads/a/shot.png";
        let text = wrapper("来源 (abc12345)", body);
        let parsed = attributed_message(&text).expect("known wrapper");
        assert_eq!(parsed.name, "来源 (abc12345)");
        assert_eq!(
            parsed.body, body,
            "body must reach the attachment parser unchanged"
        );
    }
}
