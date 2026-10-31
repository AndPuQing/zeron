//! Stable ownership of child traffic. A thread belongs to its original spawn
//! call; activity ids and early thread notifications are never document ids.

use std::collections::{HashMap, HashSet};

use serde_json::Value;
use zeron_proto::AgentEvent;

use super::normalize::{ChildStream, Phase, collab_spawn_child, item_type, map_item};

const MAX_PENDING_BYTES: usize = 4 * 1024 * 1024;

#[derive(Default)]
struct Pending {
    events: Vec<AgentEvent>,
    bytes: usize,
}

pub(super) struct Subagents {
    root: String,
    spawns: HashMap<String, String>,
    pending: HashMap<String, Pending>,
    pending_bytes: usize,
    warned_overflow: bool,
    streams: HashMap<String, ChildStream>,
    emitted_spawns: HashSet<String>,
    resolved_spawns: HashSet<String>,
}

impl Subagents {
    pub(super) fn new(root: String) -> Self {
        Self {
            root,
            spawns: HashMap::new(),
            pending: HashMap::new(),
            pending_bytes: 0,
            warned_overflow: false,
            streams: HashMap::new(),
            emitted_spawns: HashSet::new(),
            resolved_spawns: HashSet::new(),
        }
    }

    /// thread/resume returns the stored parent items. Rebuild ownership without
    /// replaying chips or content that already lives in Zeron's documents.
    pub(super) fn restore(&mut self, thread: &Value) {
        for item in thread
            .get("turns")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|turn| {
                turn.get("items")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
            })
        {
            if super::normalize::is_collab_spawn(item)
                || matches!(item_type(item), "subAgentActivity" | "sub_agent_activity")
            {
                self.parent_item(Phase::Completed, item);
            }
        }
    }

    pub(super) fn notification(
        &mut self,
        child: &str,
        method: &str,
        params: &Value,
    ) -> Vec<AgentEvent> {
        let events = self
            .streams
            .entry(child.to_owned())
            .or_default()
            .map(child, method, params);
        self.route(child, events)
    }

    pub(super) fn parent_item(&mut self, phase: Phase, item: &Value) -> Vec<AgentEvent> {
        let activity = matches!(item_type(item), "subAgentActivity" | "sub_agent_activity");
        let child = if activity {
            let path = item.get("agentPath").and_then(Value::as_str).unwrap_or("");
            if matches!(path, "/" | "/root") {
                return Vec::new();
            }
            if !matches!(
                item.get("kind").and_then(Value::as_str),
                Some("started" | "spawned")
            ) {
                return Vec::new();
            }
            item.get("agentThreadId").and_then(Value::as_str)
        } else {
            collab_spawn_child(item)
        };
        if child == Some(self.root.as_str()) {
            return Vec::new();
        }
        let mut item = std::borrow::Cow::Borrowed(item);
        let mut buffered = Vec::new();
        if let Some(child) = child {
            let call = item
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            buffered = self.bind(child, &call);
            if let Some(owner) = self.spawns.get(child) {
                item.to_mut()["id"] = owner.clone().into();
            }
        }
        let mut events: Vec<_> = map_item(phase, &item)
            .into_iter()
            .filter(|event| match event {
                // Completed items refresh tool metadata, but a spawn chip must
                // appear once even when its parent text segment has already ended.
                AgentEvent::ToolCall { id, call } if call.is_subagent_spawn() => {
                    self.emitted_spawns.insert(id.clone())
                }
                AgentEvent::ToolResult { id, .. } if self.emitted_spawns.contains(id) => {
                    self.resolved_spawns.insert(id.clone())
                }
                _ => true,
            })
            .collect();
        events.extend(buffered);
        events
    }

    /// Call only for a spawn. Emit its parent chip BEFORE the returned events,
    /// so even a child that finished before registration binds to a real chip.
    pub(super) fn bind(&mut self, child: &str, spawn: &str) -> Vec<AgentEvent> {
        if child.is_empty() || child == self.root || spawn.is_empty() {
            return Vec::new();
        }
        let owner = self
            .spawns
            .entry(child.to_owned())
            .or_insert_with(|| spawn.to_owned());
        let Some(pending) = self.pending.remove(child) else {
            return Vec::new();
        };
        self.pending_bytes -= pending.bytes;
        pending.events.into_iter().map(|e| tag(owner, e)).collect()
    }

    pub(super) fn route(&mut self, child: &str, events: Vec<AgentEvent>) -> Vec<AgentEvent> {
        if child.is_empty() || child == self.root {
            return Vec::new();
        }
        if let Some(owner) = self.spawns.get(child) {
            return events.into_iter().map(|e| tag(owner, e)).collect();
        }
        // A child's output may beat both thread/started and the spawn result.
        // Bound the aggregate backlog for threads that never acquire a spawn.
        for event in events {
            let bytes = serde_json::to_vec(&event)
                .map_or(MAX_PENDING_BYTES, |v| v.len())
                .saturating_add(image_payload_bytes(&event));
            if self.pending_bytes.saturating_add(bytes) > MAX_PENDING_BYTES {
                if !self.warned_overflow {
                    tracing::warn!("Codex unbound child backlog full; dropping excess events");
                    self.warned_overflow = true;
                }
                continue;
            }
            self.pending_bytes += bytes;
            let pending = self.pending.entry(child.to_owned()).or_default();
            pending.bytes += bytes;
            pending.events.push(event);
        }
        Vec::new()
    }
}

// Image payloads intentionally skip serialization until the engine stores
// them, but they still occupy memory while waiting for a spawn binding.
fn image_payload_bytes(event: &AgentEvent) -> usize {
    match event {
        AgentEvent::ToolResult { image_payloads, .. } => {
            image_payloads.iter().fold(0usize, |bytes, image| {
                bytes.saturating_add(image.data_base64.len())
            })
        }
        AgentEvent::Subagent { event, .. } => image_payload_bytes(event),
        _ => 0,
    }
}

fn tag(spawn: &str, event: AgentEvent) -> AgentEvent {
    AgentEvent::Subagent {
        parent_tool_use_id: spawn.to_owned(),
        event: Box::new(event),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn text(text: &str) -> AgentEvent {
        AgentEvent::TextDelta { text: text.into() }
    }

    fn screenshot(id: &str, data: &str) -> Value {
        json!({"item": {
            "type": "mcpToolCall", "id": id, "status": "completed",
            "result": {"content": [{"type": "image", "data": data, "mimeType": "image/png"}]}
        }})
    }

    #[test]
    fn unbound_images_count_towards_the_backlog_limit() {
        let mut children = Subagents::new("root".into());
        let image = screenshot("screenshot", &"A".repeat(MAX_PENDING_BYTES + 16 * 1024));
        assert!(
            children
                .notification("unknown", "item/completed", &image)
                .is_empty()
        );
        assert!(children.pending.is_empty());
        assert_eq!(children.pending_bytes, 0);

        // Known children bypass this backlog and keep their full image.
        children.bind("known", "spawn");
        let events = children.notification("known", "item/completed", &image);
        assert!(
            matches!(events.as_slice(), [AgentEvent::Subagent { event, .. }]
            if matches!(event.as_ref(), AgentEvent::ToolResult { image_payloads, .. }
                if image_payloads.len() == 1 && image_payloads[0].data_base64.len() > MAX_PENDING_BYTES))
        );
    }

    #[test]
    fn buffered_image_bytes_are_aggregated_and_released_after_binding() {
        let mut children = Subagents::new("root".into());
        let data = "A".repeat(MAX_PENDING_BYTES / 4);
        for index in 0..4 {
            children.notification(
                "early",
                "item/completed",
                &screenshot(&index.to_string(), &data),
            );
        }
        assert_eq!(children.pending["early"].events.len(), 3);
        assert!(children.pending_bytes > data.len() * 3);
        assert!(children.pending_bytes <= MAX_PENDING_BYTES);
        let events = children.bind("early", "spawn");
        assert_eq!(events.len(), 3);
        assert!(
            events
                .iter()
                .all(|event| image_payload_bytes(event) == data.len())
        );
        assert!(children.pending.is_empty());
        assert_eq!(children.pending_bytes, 0);
        children.notification("other", "item/completed", &screenshot("new", &data));
        assert_eq!(children.pending["other"].events.len(), 1);
    }

    #[test]
    fn binding_is_stable_and_children_are_independent() {
        let mut children = Subagents::new("root".into());
        children.bind("alpha", "spawn-alpha");
        children.bind("beta", "spawn-beta");
        children.bind("alpha", "later-activity");
        assert_eq!(
            children.route("alpha", vec![text("a")]),
            vec![tag("spawn-alpha", text("a"))]
        );
        assert_eq!(
            children.route("beta", vec![text("b")]),
            vec![tag("spawn-beta", text("b"))]
        );
    }

    #[test]
    fn early_content_waits_for_the_spawn_and_drains_once_in_order() {
        let mut children = Subagents::new("root".into());
        assert!(
            children
                .route("alpha", vec![text("first"), text("second")])
                .is_empty()
        );
        assert_eq!(
            children.bind("alpha", "spawn"),
            vec![tag("spawn", text("first")), tag("spawn", text("second"))]
        );
        assert!(children.bind("alpha", "spawn").is_empty());
        assert_eq!(children.pending_bytes, 0);
    }

    #[test]
    fn root_and_missing_ids_never_bind_or_buffer() {
        let mut children = Subagents::new("root".into());
        for child in ["root", ""] {
            assert!(children.bind(child, "spawn").is_empty());
            assert!(children.route(child, vec![text("noise")]).is_empty());
        }
        children.bind("alpha", "");
        assert!(children.spawns.is_empty());
        assert!(children.pending.is_empty());
    }

    #[test]
    fn unbound_backlog_is_bounded_without_affecting_known_children() {
        let mut children = Subagents::new("root".into());
        children.route("unknown", vec![text(&"x".repeat(MAX_PENDING_BYTES))]);
        assert!(children.pending.is_empty());
        children.bind("alpha", "spawn");
        assert_eq!(
            children.route("alpha", vec![text("ok")]),
            vec![tag("spawn", text("ok"))]
        );
    }
}
