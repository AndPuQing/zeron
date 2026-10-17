//! DSH's bundled ACP bridge reports built-in tools as `kind: "other"`.
//! Decode only verified built-in schemas, so shared document sanitization can
//! retain the same summaries as native drivers without exposing unknown inputs.
//! Source: @deepseek-ai/dsh-{acp,tool-*} 0.2.1-rc.2.

use serde_json::Value;
use zeron_proto::{TodoItem, TodoStatus, ToolCall};

pub(super) fn typed_call(update: &Value) -> Option<ToolCall> {
    if !matches!(
        update.get("kind").and_then(Value::as_str),
        None | Some("other")
    ) {
        return None; // Explicit ACP kinds are authoritative.
    }
    let name = update.get("title")?.as_str()?;
    let input = update.get("rawInput")?.as_object()?;
    let text = |key: &str| {
        input
            .get(key)?
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned)
    };
    let optional_text = |key: &str| input.get(key).and_then(Value::as_str).map(str::to_owned);
    Some(match name {
        "bash" | "pwsh" => ToolCall::Exec {
            command: text("command")?,
        },
        "read" => ToolCall::ReadFile {
            path: text("file_path")?,
        },
        "write" => ToolCall::WriteFile {
            path: text("file_path")?,
            content: optional_text("content"),
        },
        "edit" => ToolCall::EditFile {
            path: text("file_path")?,
            old_string: optional_text("old_string"),
            new_string: optional_text("new_string"),
        },
        "str_replace_editor" => match text("command")?.as_str() {
            "view" => ToolCall::ReadFile {
                path: text("path")?,
            },
            "create" => ToolCall::WriteFile {
                path: text("path")?,
                content: optional_text("file_text"),
            },
            "str_replace" | "insert" => ToolCall::EditFile {
                path: text("path")?,
                old_string: optional_text("old_str"),
                new_string: optional_text("new_str"),
            },
            _ => return None,
        },
        "grep" => ToolCall::Search {
            pattern: text("pattern")?,
            path: text("path"),
        },
        "glob" => ToolCall::Glob {
            pattern: text("pattern")?,
        },
        "web_fetch" => ToolCall::WebFetch {
            url: text("url")?,
            prompt: None,
        },
        "web_search" => {
            let queries: Option<Vec<_>> = input
                .get("queries")?
                .as_array()?
                .iter()
                .map(|v| v.as_str().filter(|s| !s.trim().is_empty()))
                .collect();
            let queries = queries?;
            if queries.is_empty() {
                return None;
            }
            ToolCall::WebSearch {
                query: queries.join("\n"),
            }
        }
        "todo_write" => {
            let items: Option<Vec<_>> = input
                .get("todos")?
                .as_array()?
                .iter()
                .map(|item| {
                    let text = item.get("content")?.as_str()?;
                    let status = match item.get("status")?.as_str()? {
                        "pending" => TodoStatus::Pending,
                        "in_progress" => TodoStatus::InProgress,
                        "completed" => TodoStatus::Completed,
                        _ => return None,
                    };
                    Some(TodoItem::new(text, status))
                })
                .collect();
            ToolCall::Todo { items: items? }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::normalize::map_update_for;
    use serde_json::json;
    use zeron_proto::{AgentEvent, HarnessId};

    #[test]
    fn dsh_builtin_schemas_map_to_typed_calls() {
        let cases: Vec<Value> =
            serde_json::from_str(include_str!("../../tests/fixtures/dsh-tool-calls.json")).unwrap();
        for case in cases {
            let update = json!({"sessionUpdate":"tool_call", "toolCallId":"call", "kind":"other", "title":case["name"], "rawInput":case["input"], "status":"in_progress"});
            let expected: ToolCall = serde_json::from_value(case["expected"].clone()).unwrap();
            assert_eq!(
                map_update_for(HarnessId::Dsh, &update),
                vec![AgentEvent::ToolCall {
                    id: "call".into(),
                    call: expected
                }],
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn dsh_mapping_is_provider_scoped_and_explicit_kinds_win() {
        let update = json!({"sessionUpdate":"tool_call", "toolCallId":"call", "kind":"other", "title":"bash", "rawInput":{"command":"pwd"}});
        for provider in [
            HarnessId::Copilot,
            HarnessId::Devin,
            HarnessId::Grok,
            HarnessId::Hermes,
            HarnessId::Antigravity,
        ] {
            assert!(matches!(
                &map_update_for(provider, &update)[0],
                AgentEvent::ToolCall {
                    call: ToolCall::Unknown { .. },
                    ..
                }
            ));
        }
        let mut explicit = update;
        explicit["kind"] = json!("read");
        explicit["rawInput"] = json!({"path":"actual.txt", "command":"pwd"});
        assert!(
            matches!(&map_update_for(HarnessId::Dsh, &explicit)[0], AgentEvent::ToolCall { call: ToolCall::ReadFile { path }, .. } if path == "actual.txt")
        );
    }

    #[test]
    fn dsh_completion_does_not_replace_call_and_malformed_inputs_stay_unknown() {
        let complete = json!({"sessionUpdate":"tool_call_update", "toolCallId":"call", "status":"completed", "content":[{"type":"content", "content":{"type":"text", "text":"done"}}]});
        assert!(
            matches!(map_update_for(HarnessId::Dsh, &complete).as_slice(), [AgentEvent::ToolResult { output:Some(output), .. }] if output == "done")
        );
        for raw in [
            Value::Null,
            json!("{bad json"),
            json!({}),
            json!({"command":5}),
            json!({"command":"  "}),
        ] {
            let update = json!({"sessionUpdate":"tool_call", "toolCallId":"call", "kind":"other", "title":"bash", "rawInput":raw});
            assert!(matches!(
                &map_update_for(HarnessId::Dsh, &update)[0],
                AgentEvent::ToolCall {
                    call: ToolCall::Unknown { .. },
                    ..
                }
            ));
        }
        let update = json!({"sessionUpdate":"tool_call_update", "toolCallId":"call", "title":"read", "rawInput":{"file_path":"late.txt"}});
        assert!(
            matches!(&map_update_for(HarnessId::Dsh, &update)[0], AgentEvent::ToolCall { call: ToolCall::ReadFile { path }, .. } if path == "late.txt")
        );
    }
}
