//! Claude's server-side tools use assistant content blocks, not user tool results.
//! Emit the existing tool events so desktop, mobile and older clients share the
//! same running/completed/error presentation without a new wire variant.
use std::collections::HashMap;

use serde_json::Value;
use zeron_proto::{AgentEvent, ToolCall};

use super::wire::ContentBlock;

pub(super) fn call(name: &str, input: &Value) -> ToolCall {
    let label = match name {
        "advisor" => "Advising",
        "web_search" => "Web search",
        "web_fetch" => "Fetch webpage",
        "code_execution" => "Run code",
        "bash_code_execution" => "Run shell",
        "text_editor_code_execution" => "Edit file",
        "tool_search" | "tool_search_tool_regex" | "tool_search_tool_bm25" => "Search tools",
        _ => {
            return ToolCall::Unknown {
                // A server tool named Agent must not become a client-side subagent.
                name: format!("Server tool: {name}"),
                input: (!input.is_null()).then(|| input.clone()),
            };
        }
    };
    ToolCall::Unknown {
        name: label.into(),
        input: (!input.is_null() && input != &serde_json::json!({})).then(|| input.clone()),
    }
}

pub(super) fn is_result(kind: &str) -> bool {
    kind.ends_with("_tool_result")
}

/// Keep only public text/metadata. In particular, advisor and encrypted code
/// execution blobs must never enter tool events, logs or synchronized docs.
/// Tool output follows the existing journal-only policy; no schema change.
pub(super) fn result(content: &Value) -> (bool, Option<String>) {
    fn append(out: &mut String, text: &str) {
        const LIMIT: usize = 16 * 1024;
        if out.len() >= LIMIT || text.is_empty() {
            return;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        let mut end = text.len().min(LIMIT.saturating_sub(out.len()));
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        out.push_str(&text[..end]);
    }
    fn visit(value: &Value, out: &mut String, error: &mut bool, depth: usize) {
        if depth > 8 {
            return;
        }
        match value {
            Value::String(text) => append(out, text),
            Value::Array(rows) => {
                for row in rows.iter().take(100) {
                    visit(row, out, error, depth + 1);
                }
            }
            Value::Object(fields) => {
                let kind = fields.get("type").and_then(Value::as_str).unwrap_or("");
                *error |=
                    kind.ends_with("_error") || fields.get("is_error") == Some(&Value::Bool(true));
                if let Some(code) = fields.get("return_code").and_then(Value::as_i64) {
                    *error |= code != 0;
                    append(out, &format!("Exit code: {code}"));
                }
                if kind.contains("redacted") || kind.starts_with("encrypted_") {
                    append(out, "The provider keeps this tool response private.");
                    return;
                }
                for key in [
                    "title",
                    "url",
                    "text",
                    "stdout",
                    "stderr",
                    "error_code",
                    "error_message",
                    "tool_name",
                ] {
                    if let Some(text) = fields.get(key).and_then(Value::as_str) {
                        append(out, text);
                    }
                }
                for key in ["content", "tool_references"] {
                    if let Some(child) = fields.get(key) {
                        visit(child, out, error, depth + 1);
                    }
                }
            }
            _ => {}
        }
    }
    let (mut output, mut is_error) = (String::new(), false);
    visit(content, &mut output, &mut is_error, 0);
    (is_error, (!output.is_empty()).then_some(output))
}

#[derive(Default)]
struct Seen {
    call: Option<ToolCall>,
    completed: bool,
}

/// Streaming starts and aggregate messages repeat the same blocks. Deduplicate
/// by scope AND tool id, keeping independent subagents independent.
#[derive(Default)]
pub(super) struct ServerTools {
    seen: HashMap<(String, String), Seen>,
}

impl ServerTools {
    /// A turn may stop or fail before its server result arrives. Resolve only
    /// this scope; background agents can outlive their parent's turn.
    pub fn finish(&mut self, parent: Option<&str>) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        for ((scope, id), seen) in &mut self.seen {
            if scope != parent.unwrap_or("") || seen.completed || seen.call.is_none() {
                continue;
            }
            seen.completed = true;
            let event = AgentEvent::ToolResult {
                id: id.clone(),
                is_error: true,
                output: Some("The turn ended before this tool returned a result.".into()),
                diff: None,
                preview_paths: Vec::new(),
                image_payloads: Vec::new(),
            };
            events.push(match parent {
                Some(parent) => AgentEvent::Subagent {
                    parent_tool_use_id: parent.into(),
                    event: Box::new(event),
                },
                None => event,
            });
        }
        events
    }

    pub fn block(&mut self, parent: Option<&str>, block: &ContentBlock) -> Vec<AgentEvent> {
        let is_call = block.kind == "server_tool_use";
        if !is_call && !is_result(&block.kind) {
            return Vec::new();
        }
        let id = if is_call {
            &block.id
        } else {
            &block.tool_use_id
        };
        if id.is_empty() || (is_call && block.name.is_empty()) {
            return Vec::new();
        }
        let seen = self
            .seen
            .entry((parent.unwrap_or("").into(), id.clone()))
            .or_default();
        let mut events = Vec::new();
        if is_call {
            let call = call(&block.name, &block.input);
            // The complete input may arrive only in the aggregate message,
            // after a streamed result. Updating a call preserves its resolved
            // state in the document fold; an empty replay never erases details.
            let has_input = !block.input.is_null() && block.input != serde_json::json!({});
            if seen.call.as_ref() != Some(&call) && (seen.call.is_none() || has_input) {
                seen.call = Some(call.clone());
                events.push(AgentEvent::ToolCall {
                    id: id.clone(),
                    call,
                });
            }
        } else if !seen.completed && !block.content.is_null() {
            // Some wrappers forward only the aggregate result. Keep it visible
            // without inventing a second id or an unresolvable orphan result.
            if seen.call.is_none() {
                let name = block.kind.trim_end_matches("_tool_result");
                let call = call(name, &Value::Null);
                seen.call = Some(call.clone());
                events.push(AgentEvent::ToolCall {
                    id: id.clone(),
                    call,
                });
            }
            let (is_error, output) = result(&block.content);
            seen.completed = true;
            events.push(AgentEvent::ToolResult {
                id: id.clone(),
                is_error: is_error || block.is_error == Some(true),
                output,
                diff: None,
                preview_paths: Vec::new(),
                image_payloads: Vec::new(),
            });
        }
        events
            .into_iter()
            .map(|event| match parent {
                Some(parent) => AgentEvent::Subagent {
                    parent_tool_use_id: parent.into(),
                    event: Box::new(event),
                },
                None => event,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn block(value: Value) -> ContentBlock {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn streaming_and_aggregate_blocks_share_one_card_and_keep_complete_inputs() {
        let mut tools = ServerTools::default();
        let start =
            block(json!({"type":"server_tool_use","id":"search","name":"web_search","input":{}}));
        assert!(matches!(
            tools.block(None, &start).as_slice(),
            [AgentEvent::ToolCall { .. }]
        ));
        let result = block(
            json!({"type":"web_search_tool_result","tool_use_id":"search","content":[{"type":"web_search_result","title":"Documentation","url":"https://example.com","encrypted_content":"do not publish"}]}),
        );
        assert!(matches!(
            tools.block(None, &result).as_slice(),
            [AgentEvent::ToolResult {
                is_error: false,
                ..
            }]
        ));
        let complete = block(
            json!({"type":"server_tool_use","id":"search","name":"web_search","input":{"query":"Rust"}}),
        );
        assert!(
            matches!(tools.block(None, &complete).as_slice(), [AgentEvent::ToolCall { call: ToolCall::Unknown { input: Some(input), .. }, .. }] if input["query"] == "Rust")
        );
        assert!(tools.block(None, &start).is_empty());
        assert!(tools.block(None, &complete).is_empty());
        assert!(tools.block(None, &result).is_empty());
    }

    #[test]
    fn advisor_redaction_and_errors_are_not_plaintext_or_success() {
        let (error, output) = result(
            &json!({"type":"advisor_redacted_result","encrypted_content":"private blob","text":"also private"}),
        );
        assert!(!error);
        assert_eq!(
            output.as_deref(),
            Some("The provider keeps this tool response private.")
        );
        let (error, output) =
            result(&json!({"type":"advisor_tool_result_error","error_code":"overloaded"}));
        assert!(error);
        assert_eq!(output.as_deref(), Some("overloaded"));
        assert_eq!(
            result(&json!({"type":"advisor_result","text":"Public advice"})),
            (false, Some("Public advice".into()))
        );
    }

    #[test]
    fn server_tool_families_and_future_names_keep_their_identity() {
        for (name, kind) in [
            ("advisor", "advisor_tool_result"),
            ("web_search", "web_search_tool_result"),
            ("web_fetch", "web_fetch_tool_result"),
            ("code_execution", "code_execution_tool_result"),
            ("bash_code_execution", "bash_code_execution_tool_result"),
            (
                "text_editor_code_execution",
                "text_editor_code_execution_tool_result",
            ),
            ("tool_search_tool_regex", "tool_search_tool_result"),
            ("tool_search_tool_bm25", "tool_search_tool_result"),
            ("future", "future_tool_result"),
        ] {
            let mut tools = ServerTools::default();
            let start = block(
                json!({"type":"server_tool_use","id":"tool","name":name,"input":{"query":"test"}}),
            );
            let events = tools.block(None, &start);
            assert!(
                matches!(events.as_slice(), [AgentEvent::ToolCall { id, .. }] if id == "tool"),
                "{name}"
            );
            let end = block(
                json!({"type":kind,"tool_use_id":"tool","content":{"type":"result","text":"done"}}),
            );
            assert!(
                matches!(tools.block(None, &end).as_slice(), [AgentEvent::ToolResult { id, is_error: false, .. }] if id == "tool"),
                "{name}"
            );
        }
        assert!(!call("Agent", &Value::Null).is_subagent_spawn());
    }

    #[test]
    fn subagent_scopes_and_result_only_wrappers_are_supported() {
        let mut tools = ServerTools::default();
        let result = block(
            json!({"type":"advisor_tool_result","tool_use_id":"same-id","content":{"type":"advisor_result","text":"Advice"}}),
        );
        assert_eq!(tools.block(None, &result).len(), 2);
        for scope in ["child-a", "child-b"] {
            let events = tools.block(Some(scope), &result);
            assert_eq!(events.len(), 2);
            assert!(events.iter().all(|event| matches!(event, AgentEvent::Subagent { parent_tool_use_id, .. } if parent_tool_use_id == scope)));
            assert!(tools.block(Some(scope), &result).is_empty());
        }
        assert!(
            tools
                .block(
                    None,
                    &block(json!({"type":"advisor_tool_result","content":{}}))
                )
                .is_empty()
        );
        assert!(
            tools
                .block(
                    None,
                    &block(json!({"type":"tool_use","id":"ordinary","name":"Bash"}))
                )
                .is_empty()
        );
    }

    #[test]
    fn an_empty_result_without_a_call_cannot_create_an_orphan_on_turn_end() {
        let mut tools = ServerTools::default();
        let empty =
            block(json!({"type":"advisor_tool_result","tool_use_id":"missing","content":null}));
        assert!(tools.block(None, &empty).is_empty());
        assert!(tools.finish(None).is_empty());
    }

    #[test]
    fn output_is_bounded_unicode_and_preserves_nested_errors() {
        let (error, output) = result(
            &json!({"type":"code_execution_result","return_code":1,"stdout":"字".repeat(20000)}),
        );
        assert!(error);
        assert!(output.unwrap().len() <= 16 * 1024);
        assert!(
            result(&json!([{"type":"web_fetch_tool_result_error","error_code":"unavailable"}])).0
        );
        let (_, output) = result(
            &json!({"type":"encrypted_code_execution_result","encrypted_content":"secret","stdout":"secret"}),
        );
        assert!(!output.unwrap().contains("secret"));
    }
}
