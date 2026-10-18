//! Normalize OMP's session events into Zeron's driver-neutral [`AgentEvent`].
//!
//! OMP reuses Pi's `assistantMessageEvent`/`tool_execution_*` shapes but renames
//! compaction to `auto_compaction_*`, adds `notice` frames and reports failures
//! on the final assistant message, so the mapping is close to Pi's without
//! assuming the two wires are interchangeable.
use serde_json::Value;
use std::collections::HashMap;
use zeron_proto::{AgentEvent, DoneStatus, ToolCall, ToolDiff};

#[derive(Default)]
pub(super) struct Normalizer {
    text: HashMap<usize, String>,
    thinking: HashMap<usize, String>,
    tools: HashMap<String, (String, Value)>,
    pub error: Option<String>,
    pub aborted: bool,
    pub window: Option<u64>,
    compaction_id: Option<String>,
}
impl Normalizer {
    pub fn reset(&mut self) {
        self.compaction_id = None;
        self.error = None;
        self.aborted = false;
        self.text.clear();
        self.thinking.clear();
    }
    pub fn status(&self) -> DoneStatus {
        if self.aborted {
            DoneStatus::Interrupted
        } else if self.error.is_some() {
            DoneStatus::Errored
        } else {
            DoneStatus::Completed
        }
    }
    pub fn map(&mut self, frame: &Value) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        match string(frame, "type") {
            "message_start" if frame["message"]["role"] == "assistant" => {
                self.text.clear();
                self.thinking.clear();
            }
            "message_update" => {
                let update = &frame["assistantMessageEvent"];
                let index = update["contentIndex"].as_u64().unwrap_or(0) as usize;
                let delta = string(update, "delta");
                match string(update, "type") {
                    "text_delta" => {
                        self.text.entry(index).or_default().push_str(delta);
                        events.push(AgentEvent::TextDelta { text: delta.into() });
                    }
                    "thinking_delta" => {
                        self.thinking.entry(index).or_default().push_str(delta);
                        events.push(AgentEvent::ReasoningDelta { text: delta.into() });
                    }
                    _ => {}
                }
            }
            "message_end" if frame["message"]["role"] == "assistant" => {
                let message = &frame["message"];
                self.error = (message["stopReason"] == "error").then(|| {
                    message["errorMessage"]
                        .as_str()
                        .filter(|text| !text.is_empty())
                        .unwrap_or("OMP provider failed")
                        .to_owned()
                });
                self.aborted = message["stopReason"] == "aborted";
                if let Some(content) = message["content"].as_array() {
                    for (index, part) in content.iter().enumerate() {
                        let (cache, value, reasoning) = match string(part, "type") {
                            "text" => (&mut self.text, string(part, "text"), false),
                            "thinking" => (&mut self.thinking, string(part, "thinking"), true),
                            _ => continue,
                        };
                        let previous = cache.entry(index).or_default();
                        if let Some(suffix) = value
                            .strip_prefix(previous.as_str())
                            .filter(|s| !s.is_empty())
                        {
                            events.push(if reasoning {
                                AgentEvent::ReasoningDelta {
                                    text: suffix.into(),
                                }
                            } else {
                                AgentEvent::TextDelta {
                                    text: suffix.into(),
                                }
                            });
                        }
                        *previous = value.into();
                    }
                }
                let usage = &message["usage"];
                if usage.is_object() {
                    let n = |key| usage[key].as_u64().unwrap_or(0);
                    let input = n("input") + n("cacheRead") + n("cacheWrite");
                    events.push(AgentEvent::Usage {
                        input_tokens: input,
                        output_tokens: n("output"),
                    });
                    events.push(AgentEvent::ContextUsage {
                        tokens: Some(input + n("output")),
                        window: self.window,
                    });
                }
            }
            "tool_execution_start" => {
                let id = string(frame, "toolCallId").to_owned();
                let name = string(frame, "toolName");
                if !self.tools.contains_key(&id) {
                    let args = frame["args"].clone();
                    events.push(AgentEvent::ToolCall {
                        id: id.clone(),
                        call: tool(name, &args),
                    });
                    self.tools.insert(id, (name.into(), args));
                }
            }
            "tool_execution_end" => {
                let id = string(frame, "toolCallId").to_owned();
                let info = self.tools.remove(&id);
                let result = &frame["result"];
                let output = result["content"].as_array().map(|items| {
                    items
                        .iter()
                        .filter_map(|v| v["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                });
                let diff = info.as_ref().and_then(|(name, args)| {
                    if frame["isError"] == true {
                        return None;
                    }
                    match name.as_str() {
                        "edit" | "ast_edit" => Some(ToolDiff {
                            path: string(args, "path").into(),
                            old_text: Some(cap(edit_old(args), 65536)),
                            new_text: cap(edit_new(args), 65536),
                        }),
                        "write" => Some(ToolDiff {
                            path: string(args, "path").into(),
                            old_text: None,
                            new_text: cap(string(args, "content"), 65536),
                        }),
                        _ => None,
                    }
                });
                events.push(AgentEvent::ToolResult {
                    id,
                    is_error: frame["isError"].as_bool().unwrap_or(false),
                    output: output.map(|s| cap(&s, 16384)),
                    diff,
                });
            }
            // OMP names its compaction lifecycle `auto_compaction_*`; Pi used
            // `compaction_*`. Accept both so a fork change cannot hide it.
            "compaction_start" | "auto_compaction_start" => {
                let id = self
                    .compaction_id
                    .get_or_insert_with(|| uuid::Uuid::new_v4().to_string())
                    .clone();
                events.push(AgentEvent::ContextCompaction {
                    id,
                    status: zeron_proto::CompactionStatus::InProgress,
                });
            }
            "compaction_end" | "auto_compaction_end" => {
                let id = self
                    .compaction_id
                    .take()
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                let status = if frame["aborted"] == true {
                    zeron_proto::CompactionStatus::Cancelled
                } else if frame["result"].is_object() || frame["skipped"] == true {
                    zeron_proto::CompactionStatus::Completed
                } else {
                    zeron_proto::CompactionStatus::Failed
                };
                events.push(AgentEvent::ContextCompaction { id, status });
                if let Some(tokens) = frame["result"]["estimatedTokensAfter"]
                    .as_u64()
                    .or_else(|| frame["result"]["tokensAfter"].as_u64())
                {
                    events.push(AgentEvent::ContextUsage {
                        tokens: Some(tokens),
                        window: self.window,
                    });
                }
            }
            // Extension notices. Only failures are surfaced; informational
            // notices would otherwise be injected into the transcript.
            "notice" if matches!(string(frame, "level"), "error" | "warning") => {
                let message = string(frame, "message");
                if !message.is_empty() {
                    events.push(AgentEvent::Error {
                        message: format!("OMP: {message}"),
                    });
                }
            }
            "extension_error" => {
                self.error = Some(string(frame, "error").into());
                events.push(AgentEvent::Error {
                    message: format!("OMP extension: {}", string(frame, "error")),
                });
            }
            _ => {}
        }
        events
    }
}
pub(super) fn string<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn edit_old(args: &Value) -> &str {
    let camel = string(args, "oldText");
    if camel.is_empty() {
        string(args, "old_string")
    } else {
        camel
    }
}
fn edit_new(args: &Value) -> &str {
    let camel = string(args, "newText");
    if camel.is_empty() {
        string(args, "new_string")
    } else {
        camel
    }
}
fn cap(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.into();
    }
    let mut at = limit;
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    format!("{}\n… [truncated]", &text[..at])
}
pub(super) fn tool(name: &str, args: &Value) -> ToolCall {
    match name {
        "bash" => ToolCall::Exec {
            command: string(args, "command").into(),
        },
        "read" => ToolCall::ReadFile {
            path: string(args, "path").into(),
        },
        "write" => ToolCall::WriteFile {
            path: string(args, "path").into(),
            content: args["content"].as_str().map(str::to_owned),
        },
        "edit" | "ast_edit" => ToolCall::EditFile {
            path: string(args, "path").into(),
            old_string: [string(args, "oldText"), string(args, "old_string")]
                .into_iter()
                .find(|s| !s.is_empty())
                .map(str::to_owned),
            new_string: [string(args, "newText"), string(args, "new_string")]
                .into_iter()
                .find(|s| !s.is_empty())
                .map(str::to_owned),
        },
        "grep" => ToolCall::Search {
            pattern: string(args, "pattern").into(),
            path: args["path"].as_str().map(str::to_owned),
        },
        "glob" | "find" => ToolCall::Glob {
            pattern: string(args, "pattern").into(),
        },
        "web_search" => ToolCall::WebSearch {
            query: string(args, "query").into(),
        },
        "fetch" => ToolCall::WebFetch {
            url: string(args, "url").into(),
            prompt: args["prompt"].as_str().map(str::to_owned),
        },
        name if name.starts_with("zeron_") => ToolCall::Mcp {
            server: "zeron".into(),
            tool: name[6..].into(),
            input: Some(args.clone()),
        },
        name if name.starts_with("mcp__") => {
            let (server, tool) = name[5..]
                .split_once('_')
                .map_or(("mcp", &name[5..]), |(s, t)| (s, t));
            ToolCall::Mcp {
                server: server.into(),
                tool: tool.into(),
                input: Some(args.clone()),
            }
        }
        _ => ToolCall::Unknown {
            name: name.into(),
            input: Some(args.clone()),
        },
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn compaction_lifecycle_preserves_id_and_updates_usage() {
        use zeron_proto::CompactionStatus;
        let mut n = Normalizer::default();
        let start = n.map(
            &json!({"type":"auto_compaction_start", "reason":"threshold", "action":"context-full"}),
        );
        assert_eq!(n.map(&json!({"type":"auto_compaction_start"})), start);
        let AgentEvent::ContextCompaction {
            id,
            status: CompactionStatus::InProgress,
        } = &start[0]
        else {
            panic!("{start:?}");
        };
        let end = n.map(
            &json!({"type":"auto_compaction_end", "result":{"tokensAfter":1200}, "aborted":false}),
        );
        assert_eq!(
            end[0],
            AgentEvent::ContextCompaction {
                id: id.clone(),
                status: CompactionStatus::Completed
            }
        );
        assert_eq!(
            end[1],
            AgentEvent::ContextUsage {
                tokens: Some(1200),
                window: None
            }
        );
        let next = n.map(&json!({"type":"auto_compaction_start"}));
        assert_ne!(start, next);
    }

    #[test]
    fn compaction_completed_only_failure_and_abort_are_distinct() {
        use zeron_proto::CompactionStatus;
        for (frame, expected) in [
            (
                json!({"type":"auto_compaction_end", "result":{}, "aborted":false}),
                CompactionStatus::Completed,
            ),
            (
                json!({"type":"auto_compaction_end", "aborted":true}),
                CompactionStatus::Cancelled,
            ),
            (
                json!({"type":"auto_compaction_end", "errorMessage":"failed", "willRetry":true}),
                CompactionStatus::Failed,
            ),
        ] {
            let events = Normalizer::default().map(&frame);
            assert!(
                matches!(&events[0], AgentEvent::ContextCompaction { status, .. } if *status == expected)
            );
        }
    }

    #[test]
    fn final_message_reconciles_without_duplicate_text_and_retry_clears_error() {
        let mut n = Normalizer::default();
        n.map(&json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"hel"}}));
        let events = n.map(&json!({"type":"message_end","message":{"role":"assistant","stopReason":"error","errorMessage":"temporary","content":[{"type":"text","text":"hello"}]}}));
        assert!(matches!(&events[0], AgentEvent::TextDelta {text} if text == "lo"));
        assert_eq!(n.status(), DoneStatus::Errored);
        n.map(&json!({"type":"message_end","message":{"role":"assistant","stopReason":"stop","content":[]}}));
        assert_eq!(n.status(), DoneStatus::Completed);
    }

    #[test]
    fn tools_preserve_identity_and_edit_diff() {
        let mut n = Normalizer::default();
        let start = json!({"type":"tool_execution_start","toolCallId":"t","toolName":"edit","args":{"path":"a","oldText":"old","newText":"new"}});
        assert_eq!(n.map(&start).len(), 1);
        assert!(n.map(&start).is_empty());
        let events=n.map(&json!({"type":"tool_execution_end","toolCallId":"t","isError":false,"result":{"content":[{"type":"text","text":"ok"}]}}));
        assert!(
            matches!(&events[0],AgentEvent::ToolResult{id,diff:Some(diff),..} if id=="t" && diff.new_text=="new")
        );
    }

    #[test]
    fn omp_tool_names_and_error_notices_map_without_faking_capabilities() {
        assert_eq!(
            tool("bash", &json!({"command":"pwd"})),
            ToolCall::Exec {
                command: "pwd".into()
            }
        );
        assert_eq!(
            tool("glob", &json!({"pattern":"**/*.rs"})),
            ToolCall::Glob {
                pattern: "**/*.rs".into()
            }
        );
        // A tool Zeron has no renderer for stays an honest Unknown.
        assert!(matches!(
            tool("task", &json!({"agent":"scout"})),
            ToolCall::Unknown { name, .. } if name == "task"
        ));
        let events = Normalizer::default().map(&json!({
            "type":"notice","level":"error","message":"provider revoked"
        }));
        assert!(
            matches!(&events[0], AgentEvent::Error { message } if message == "OMP: provider revoked")
        );
        // Informational notices are not injected into the transcript.
        assert!(
            Normalizer::default()
                .map(&json!({"type":"notice","level":"info","message":"hi"}))
                .is_empty()
        );
    }
}
