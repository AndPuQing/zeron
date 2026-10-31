//! A transcript read as usage: which harness ran each assistant turn, how
//! it ended, how long it took, and whether the person's next message asked
//! for it to be redone. The aggregation and heuristics live in
//! [`zeron_proto::usage_insights`]; this is the transcript-shape half.

use zeron_proto::HarnessId;
use zeron_proto::usage_insights::{
    ChatUsage, TurnOutcome, TurnRecord, classify_task, is_correction,
};

use crate::{MessagePart, MessageRole, MessageStatus, SessionMessageEntry};

/// One logical message: a root entry plus its continuation segments.
struct Message {
    role: MessageRole,
    parts: Vec<MessagePart>,
    created_at: i64,
    status: Option<MessageStatus>,
    duration_ms: Option<i64>,
}

/// Fold continuation segments into their roots with
/// [`crate::join_continuation_entries`]. The last segment also speaks for
/// how the turn ended, which the render-time join leaves on the root.
fn messages(entries: Vec<SessionMessageEntry>) -> Vec<Message> {
    let mut ended: std::collections::HashMap<String, MessageStatus> =
        std::collections::HashMap::new();
    for entry in &entries {
        if let (Some(root), Some(status)) = (&entry.continuation_of, &entry.status) {
            ended.insert(root.clone(), status.clone());
        }
    }
    crate::join_continuation_entries(entries)
        .into_iter()
        .map(|entry| Message {
            status: ended.remove(&entry.id).or(entry.status),
            role: entry.role,
            parts: entry.parts,
            created_at: entry.created_at,
            duration_ms: entry.duration_ms,
        })
        .collect()
}

/// A seam's harness: its slug, else its frozen display label.
fn seam_harness(slug: Option<&str>, label: &str) -> Option<String> {
    slug.and_then(HarnessId::from_slug)
        .or_else(|| {
            HarnessId::ALL
                .into_iter()
                .find(|id| zeron_proto::providers::provider_name(*id) == label)
        })
        .map(|id| id.as_str().to_owned())
}

/// The text a person typed: their text parts, or `None` for a message
/// another chat's agent sent through `send_message`.
fn human_text(message: &Message) -> Option<String> {
    let text: String = message
        .parts
        .iter()
        .filter_map(|part| match part {
            MessagePart::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    (zeron_proto::chat_attribution::attributed_message(&text).is_none()).then_some(text)
}

/// Whether a message holds a seam or a context operation (a compaction).
fn has_context_operation(message: &Message) -> bool {
    message.parts.iter().any(|part| {
        matches!(
            part,
            MessagePart::Compaction { .. } | MessagePart::Switch { .. } | MessagePart::Fork { .. }
        )
    })
}

/// Whether an assistant message holds a reply rather than only a seam or a
/// context operation.
fn is_reply(message: &Message) -> bool {
    message.parts.iter().any(|part| {
        matches!(
            part,
            MessagePart::Text { .. }
                | MessagePart::Tool { .. }
                | MessagePart::Error { .. }
                | MessagePart::Input { .. }
                | MessagePart::Image { .. }
                | MessagePart::Visual { .. }
        )
    })
}

/// Read a chat's transcript as usage.
///
/// `latest_harness` is the slug of the harness that runs the chat now. It
/// owns every turn after the last provider-switch seam, and every turn when
/// there is none. Turns before a switch belong to the seam's `from`
/// harness; turns between seams whose harness cannot be told are left out.
/// History above a fork's seam was copied from the source chat and is
/// counted there, not here. Live turns are skipped until they end.
pub fn chat_usage(entries: Vec<SessionMessageEntry>, latest_harness: &str) -> ChatUsage {
    let mut messages = messages(entries);
    if let Some(fork) = messages.iter().rposition(|message| {
        message
            .parts
            .iter()
            .any(|part| matches!(part, MessagePart::Fork { .. }))
    }) {
        messages.drain(..=fork);
    }
    let is_switch = |part: &MessagePart| matches!(part, MessagePart::Switch { .. });
    let last_switch = messages
        .iter()
        .rposition(|message| message.parts.iter().any(is_switch));

    // The harness before the first seam is that seam's `from`.
    let first_switch = messages
        .iter()
        .flat_map(|m| &m.parts)
        .find_map(|part| match part {
            MessagePart::Switch {
                from, from_harness, ..
            } => Some(seam_harness(from_harness.as_deref(), from)),
            _ => None,
        });
    let mut harness: Option<String> = match first_switch {
        Some(from) => from,
        None => Some(latest_harness.to_owned()),
    };

    let mut category = None;
    let mut turns: Vec<TurnRecord> = Vec::new();
    // The last completed reply the person has not answered yet.
    let mut answerable: Option<usize> = None;
    for (ix, message) in messages.iter().enumerate() {
        if Some(ix) == last_switch {
            // Whatever the last seam's label says, the harness running the
            // chat now ran everything after it.
            harness = Some(latest_harness.to_owned());
        } else {
            for part in &message.parts {
                if let MessagePart::Switch { to, to_harness, .. } = part {
                    harness = seam_harness(to_harness.as_deref(), to);
                }
            }
        }
        match message.role {
            MessageRole::System => {}
            MessageRole::User => {
                let Some(text) = human_text(message) else {
                    continue;
                };
                if text.trim().is_empty() {
                    continue;
                }
                if category.is_none() {
                    category = Some(classify_task(&text));
                }
                if let Some(ix) = answerable.take()
                    && is_correction(&text)
                {
                    turns[ix].reworked = true;
                }
            }
            MessageRole::Assistant => {
                let outcome = match message.status {
                    Some(MessageStatus::Streaming) => continue,
                    Some(MessageStatus::Aborted) => TurnOutcome::Interrupted,
                    _ if message
                        .parts
                        .iter()
                        .any(|part| matches!(part, MessagePart::Error { .. })) =>
                    {
                        TurnOutcome::Failed
                    }
                    _ => TurnOutcome::Completed,
                };
                // A turn stopped before it said anything is still a turn; a
                // stopped compaction or seam never was one.
                if !is_reply(message)
                    && (outcome != TurnOutcome::Interrupted || has_context_operation(message))
                {
                    continue;
                }
                let Some(harness) = harness.clone() else {
                    continue;
                };
                answerable = (outcome == TurnOutcome::Completed).then_some(turns.len());
                turns.push(TurnRecord {
                    harness,
                    started_at: message.created_at,
                    duration_ms: message.duration_ms,
                    outcome,
                    reworked: false,
                });
            }
        }
    }
    ChatUsage {
        category: category.unwrap_or(zeron_proto::usage_insights::TaskCategory::Other),
        turns,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeron_proto::usage_insights::TaskCategory;

    fn entry(id: &str, role: MessageRole, parts: Vec<MessagePart>) -> SessionMessageEntry {
        SessionMessageEntry {
            id: id.into(),
            role,
            parts,
            created_at: 1_000,
            device_id: "d".into(),
            status: Some(MessageStatus::Complete),
            continuation_of: None,
            duration_ms: None,
        }
    }

    fn user(id: &str, text: &str) -> SessionMessageEntry {
        entry(
            id,
            MessageRole::User,
            vec![MessagePart::Text {
                id: format!("{id}-t"),
                text: text.into(),
            }],
        )
    }

    fn reply(id: &str, duration: i64) -> SessionMessageEntry {
        SessionMessageEntry {
            duration_ms: Some(duration),
            ..entry(
                id,
                MessageRole::Assistant,
                vec![
                    MessagePart::Tool {
                        id: format!("{id}-tool"),
                        call: zeron_proto::ToolCall::Unknown {
                            name: "shell".into(),
                            input: None,
                        },
                        is_error: false,
                        resolved: true,
                        output: None,
                        diff: None,
                        output_ref: None,
                        output_bytes: None,
                        diff_ref: None,
                        diff_stats: None,
                        subagent_ref: None,
                        subagent_status: None,
                        subagent_tail: None,
                        preview_paths: Vec::new(),
                    },
                    MessagePart::Text {
                        id: format!("{id}-t"),
                        text: "done".into(),
                    },
                ],
            )
        }
    }

    fn switch(from: &str, to: &str) -> SessionMessageEntry {
        entry(
            &format!("switch-{to}"),
            MessageRole::System,
            vec![MessagePart::Switch {
                id: format!("switch-{to}"),
                from: String::new(),
                to: String::new(),
                from_harness: Some(from.into()),
                to_harness: Some(to.into()),
            }],
        )
    }

    #[test]
    fn turns_carry_outcome_duration_tools_and_rework() {
        let mut failed = reply("a2", 50);
        failed.parts.push(MessagePart::Error {
            id: "e".into(),
            message: "boom".into(),
        });
        let mut stopped = reply("a3", 10);
        stopped.status = Some(MessageStatus::Aborted);
        let mut live = reply("a4", 0);
        live.status = Some(MessageStatus::Streaming);
        let usage = chat_usage(
            vec![
                user("u1", "Fix the crash on launch"),
                reply("a1", 400),
                user("u2", "still crashes on Windows"),
                failed,
                user("u3", "try once more"),
                stopped,
                user("u4", "ok go on"),
                live,
            ],
            "codex",
        );
        assert_eq!(usage.category, TaskCategory::Bugfix);
        assert_eq!(usage.turns.len(), 3);
        let first = &usage.turns[0];
        assert_eq!(first.harness, "codex");
        assert_eq!(first.outcome, TurnOutcome::Completed);
        assert_eq!(first.duration_ms, Some(400));
        assert!(first.reworked);
        assert_eq!(usage.turns[1].outcome, TurnOutcome::Failed);
        assert!(
            !usage.turns[1].reworked,
            "only completed replies are reworked"
        );
        assert_eq!(usage.turns[2].outcome, TurnOutcome::Interrupted);
    }

    #[test]
    fn switch_seams_attribute_turns_to_the_provider_that_ran_them() {
        let usage = chat_usage(
            vec![
                user("u1", "Add a settings page"),
                reply("a1", 100),
                switch("claude-code", "codex"),
                user("u2", "keep going"),
                reply("a2", 200),
            ],
            "codex",
        );
        let harnesses: Vec<&str> = usage.turns.iter().map(|t| t.harness.as_str()).collect();
        assert_eq!(harnesses, ["claude-code", "codex"]);
        assert_eq!(usage.category, TaskCategory::Feature);
    }

    #[test]
    fn continuations_join_and_the_last_segment_ends_the_turn() {
        let mut tail = reply("a1b", 900);
        tail.continuation_of = Some("a1".into());
        tail.status = Some(MessageStatus::Aborted);
        let usage = chat_usage(
            vec![
                user("u1", "Explain the sync engine"),
                reply("a1", 300),
                tail,
            ],
            "pi",
        );
        assert_eq!(usage.turns.len(), 1);
        assert_eq!(usage.turns[0].outcome, TurnOutcome::Interrupted);
        assert_eq!(usage.turns[0].duration_ms, Some(900));
    }

    #[test]
    fn the_running_harness_owns_turns_after_an_unreadable_last_seam() {
        let mut unreadable = switch("claude-code", "codex");
        if let MessagePart::Switch { to, to_harness, .. } = &mut unreadable.parts[0] {
            *to = "A renamed provider".into();
            *to_harness = None;
        }
        let usage = chat_usage(
            vec![
                user("u1", "Add a settings page"),
                reply("a1", 100),
                unreadable,
                user("u2", "keep going"),
                reply("a2", 200),
            ],
            "codex",
        );
        let harnesses: Vec<&str> = usage.turns.iter().map(|t| t.harness.as_str()).collect();
        assert_eq!(harnesses, ["claude-code", "codex"]);
    }

    #[test]
    fn forked_history_and_agent_messages_are_not_this_chats_usage() {
        let fork = entry(
            "fork",
            MessageRole::System,
            vec![MessagePart::Fork {
                id: "fork".into(),
                source_chat_id: "source".into(),
                source_title: "Source".into(),
            }],
        );
        let agent = "[Message from Zeron chat Planner (abc12345). Reply to it with the Zeron `send_message` tool, chat abc12345-full.]\n\nno, that is wrong";
        let usage = chat_usage(
            vec![
                user("u0", "Refactor the store"),
                reply("a0", 100),
                fork,
                user("u1", "Write docs for the store"),
                reply("a1", 100),
                user("u2", agent),
            ],
            "claude-code",
        );
        assert_eq!(usage.category, TaskCategory::Docs);
        assert_eq!(usage.turns.len(), 1);
        assert!(
            !usage.turns[0].reworked,
            "an agent's message is not the person's"
        );
    }

    #[test]
    fn seam_only_and_empty_messages_are_not_turns() {
        let compaction = entry(
            "c",
            MessageRole::Assistant,
            vec![MessagePart::Compaction {
                id: "c".into(),
                status: zeron_proto::CompactionStatus::Completed,
            }],
        );
        let mut stopped_compaction = compaction.clone();
        stopped_compaction.id = "c2".into();
        stopped_compaction.status = Some(MessageStatus::Aborted);
        let usage = chat_usage(vec![compaction, stopped_compaction], "codex");
        assert!(usage.turns.is_empty());
        assert_eq!(usage.category, TaskCategory::Other);

        // Stopped before saying anything: still a stopped turn.
        let mut silent = entry("a", MessageRole::Assistant, Vec::new());
        silent.status = Some(MessageStatus::Aborted);
        let usage = chat_usage(vec![user("u", "Fix the build"), silent], "codex");
        assert_eq!(usage.turns.len(), 1);
        assert_eq!(usage.turns[0].outcome, TurnOutcome::Interrupted);
    }
}
