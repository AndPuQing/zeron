//! Native saved-session routing for ACP providers. Hermes owns its fork and
//! replay; no prompt or client-side tool is ever executed while importing.
use super::{AcpHarness, Incoming, RpcClient, initialize_params};
use crate::{HarnessError, saved_sessions::*};
use serde_json::{Value, json};
use std::{collections::HashMap, path::PathBuf, time::Duration};
use zeron_proto::{AgentEvent, HarnessId};

pub(super) fn error(message: impl Into<String>) -> HarnessError {
    HarnessError::Protocol(message.into())
}
pub(super) fn timestamp(v: &Value) -> i64 {
    v.as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.timestamp_millis())
        .or_else(|| v.as_i64())
        .unwrap_or(0)
}
impl AcpHarness {
    pub(super) fn import_root(&self) -> Result<PathBuf, HarnessError> {
        let (variable, default) = match self.spec.id {
            HarnessId::Hermes => ("HERMES_HOME", ".hermes"),
            HarnessId::Copilot => ("COPILOT_HOME", ".copilot"),
            HarnessId::Grok => ("GROK_HOME", ".grok"),
            _ => return Err(unsupported()),
        };
        storage_root(self.saved_home.as_deref(), variable, default)
    }
    pub(super) fn import_store_id(&self) -> Result<String, HarnessError> {
        if self.spec.id == HarnessId::Dsh {
            return self.dsh_saved_store_id();
        }
        Ok(store_id(self.spec.id, &self.import_root()?))
    }
    pub(super) fn check_import(&self, session: &SavedSession) -> Result<(), HarnessError> {
        if session.harness != self.spec.id || session.store_id != self.import_store_id()? {
            return Err(error("The provider history location changed. Scan again."));
        }
        if session.native_id.is_empty()
            || session.native_id.contains(['/', '\\'])
            || session.native_id == "."
            || session.native_id == ".."
        {
            return Err(error("Invalid native session id."));
        }
        Ok(())
    }
    pub(super) async fn import_list(
        &self,
        cursor: Option<&str>,
    ) -> Result<SavedSessionPage, HarnessError> {
        if self.spec.id == HarnessId::Dsh {
            return self.dsh_saved_list(cursor).await;
        }
        match self.import_root() {
            Err(HarnessError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SavedSessionPage::default());
            }
            Err(e) => return Err(e),
            Ok(_) => {}
        }
        if self.spec.id == HarnessId::Copilot {
            return self.copilot_list(cursor).await;
        }
        if self.spec.id == HarnessId::Grok {
            return self.grok_list(cursor).await;
        }
        let (response, _) = self
            .hermes_saved_request("session/list", json!({"cursor":cursor}), None)
            .await?;
        let sessions = response["sessions"]
            .as_array()
            .ok_or_else(|| error("Invalid native session list."))?
            .iter()
            .map(|row| {
                let native_id = row["sessionId"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| error("Missing native session id."))?
                    .to_owned();
                let updated = timestamp(&row["updatedAt"]);
                Ok(SavedSession {
                    harness: self.spec.id,
                    native_id,
                    store_id: self.import_store_id()?,
                    title: row["title"].as_str().unwrap_or("Hermes session").into(),
                    cwd: row["cwd"].as_str().unwrap_or_default().into(),
                    created_at_ms: updated,
                    updated_at_ms: updated,
                    model: None,
                    locator: None,
                    running: false,
                })
            })
            .collect::<Result<Vec<_>, HarnessError>>()?;
        Ok(SavedSessionPage {
            sessions,
            next_cursor: response["nextCursor"].as_str().map(str::to_owned),
        })
    }
    pub(super) async fn import_history(
        &self,
        session: &SavedSession,
    ) -> Result<SavedSessionHistory, HarnessError> {
        if self.spec.id == HarnessId::Dsh {
            return self.dsh_saved_call("history", None, Some(session)).await;
        }
        self.check_import(session)?;
        if self.spec.id == HarnessId::Copilot {
            return self.copilot_history(session).await;
        }
        if self.spec.id == HarnessId::Grok {
            return self.grok_history(session).await;
        }
        let (_, history) = self
            .hermes_saved_request(
                "session/load",
                json!({"sessionId":session.native_id,"cwd":session.cwd,"mcpServers":[]}),
                Some(&session.native_id),
            )
            .await?;
        if history.messages.is_empty() {
            return Err(error(
                "The native provider did not replay any saved messages.",
            ));
        }
        Ok(history)
    }
    pub(super) async fn import_copy(
        &self,
        session: &SavedSession,
    ) -> Result<SavedSession, HarnessError> {
        if self.spec.id == HarnessId::Dsh {
            return self.dsh_saved_call("copy", None, Some(session)).await;
        }
        self.check_import(session)?;
        if session.running {
            return Err(running());
        }
        if self.spec.id == HarnessId::Copilot {
            return self.copilot_copy(session).await;
        }
        if self.spec.id == HarnessId::Grok {
            return self.grok_copy(session).await;
        }
        let (response, _) = self
            .hermes_saved_request(
                "session/fork",
                json!({"sessionId":session.native_id,"cwd":session.cwd,"mcpServers":[]}),
                None,
            )
            .await?;
        let id = response["sessionId"]
            .as_str()
            .filter(|id| !id.is_empty() && *id != session.native_id)
            .ok_or_else(|| error("Hermes did not create an independent saved session."))?;
        Ok(SavedSession {
            native_id: id.into(),
            model: response["models"]["currentModelId"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| session.model.clone()),
            ..session.clone()
        })
    }
    async fn hermes_saved_request(
        &self,
        method: &str,
        params: Value,
        replay_id: Option<&str>,
    ) -> Result<(Value, SavedSessionHistory), HarnessError> {
        let (_scratch, mut child, _stderr, prompted) =
            self.spawn_agent(None, false, &[], false).await?;
        let (client, mut incoming) = RpcClient::bounded(
            child
                .stdin
                .take()
                .ok_or_else(|| error("Missing native stdin."))?,
            child
                .stdout
                .take()
                .ok_or_else(|| error("Missing native stdout."))?,
            MAX_RECORD_BYTES,
        );
        let operation = async {
            let (init, _) = collect(
                &client,
                &mut incoming,
                "initialize",
                initialize_params(self.spec.id),
                None,
            )
            .await?;
            let caps = &init["agentCapabilities"];
            if caps["loadSession"] != true
                || caps["sessionCapabilities"]["fork"].is_null()
                || caps["sessionCapabilities"]["list"].is_null()
            {
                return Err(error(
                    "This Hermes version cannot list, fork and restore persistent sessions. Update Hermes and its ACP extra.",
                ));
            }
            collect(&client, &mut incoming, method, params, replay_id).await
        };
        let result = tokio::time::timeout(
            Duration::from_secs(90),
            super::unless_sign_in_prompted(operation, &prompted, self.spec.display_name),
        )
        .await
        .map_err(|_| error("Native history request timed out."));
        child.shutdown(self.kill_grace).await;
        result?
    }
}

pub(super) async fn collect(
    client: &RpcClient,
    incoming: &mut tokio::sync::mpsc::Receiver<Incoming>,
    method: &str,
    params: Value,
    replay_id: Option<&str>,
) -> Result<(Value, SavedSessionHistory), HarnessError> {
    let request = client.request(method, params);
    tokio::pin!(request);
    let mut history = Replay::default();
    // The JSON-RPC reader enqueues notifications before resolving the response.
    // Drain queued frames after the response too, preserving the complete replay.
    let response = loop {
        tokio::select! {
            response=&mut request => break response?,
            frame=incoming.recv() => handle(frame,client,replay_id,&mut history)?,
        }
    };
    while let Ok(frame) = incoming.try_recv() {
        if matches!(frame, Incoming::Eof) {
            break;
        }
        handle(Some(frame), client, replay_id, &mut history)?;
    }
    history.history.check_budget()?;
    Ok((response, history.history))
}
fn handle(
    frame: Option<Incoming>,
    client: &RpcClient,
    id: Option<&str>,
    history: &mut Replay,
) -> Result<(), HarnessError> {
    match frame {
        Some(Incoming::Request { id, .. }) => {
            client.respond_error(&id, -32601, "Tools are unavailable during session import")
        }
        Some(Incoming::Notification { method, params })
            if method == "session/update" && id.is_some_and(|id| params["sessionId"] == id) =>
        {
            history.push(&params["update"])?
        }
        Some(Incoming::Eof) | None => return Err(error("Native history stream closed.")),
        _ => {}
    }
    Ok(())
}
#[derive(Default)]
pub(super) struct Replay {
    history: SavedSessionHistory,
    tools: HashMap<String, (usize, usize)>,
    bytes: usize,
}
impl Replay {
    pub(super) fn finish(self) -> SavedSessionHistory {
        self.history
    }
    pub(super) fn push(&mut self, update: &Value) -> Result<(), HarnessError> {
        let kind = update["sessionUpdate"].as_str().unwrap_or_default();
        if kind == "user_message_chunk" {
            if let Some(text) = super::normalize::chunk_text(update) {
                self.text(SavedMessageRole::User, SavedHistoryPart::Text(text))?;
            }
        } else {
            for event in super::normalize::map_update(update) {
                match event {
                    AgentEvent::TextDelta { text } => {
                        self.text(SavedMessageRole::Assistant, SavedHistoryPart::Text(text))?
                    }
                    AgentEvent::ReasoningDelta { text } => self.text(
                        SavedMessageRole::Assistant,
                        SavedHistoryPart::Reasoning(text),
                    )?,
                    AgentEvent::ToolCall { id, call } => {
                        if let Some(&(m, p)) = self.tools.get(&id) {
                            if let SavedHistoryPart::Tool { call: old, .. } =
                                &mut self.history.messages[m].parts[p]
                            {
                                *old = call;
                            }
                            continue;
                        }
                        let index = self.history.messages.len();
                        self.text(
                            SavedMessageRole::Assistant,
                            SavedHistoryPart::Tool {
                                call,
                                output: None,
                                is_error: false,
                            },
                        )?;
                        self.tools.insert(id, (index, 0));
                    }
                    AgentEvent::ToolResult {
                        id,
                        output,
                        is_error,
                        ..
                    } => {
                        if output
                            .as_deref()
                            .is_some_and(|s| s.ends_with("… [truncated]"))
                        {
                            self.history.add_notice("Long tool output is shortened for display; the full result remains in the native session.");
                        }
                        if let Some(&(m, p)) = self.tools.get(&id) {
                            if let SavedHistoryPart::Tool {
                                output: old,
                                is_error: failed,
                                ..
                            } = &mut self.history.messages[m].parts[p]
                            {
                                *old = output;
                                *failed = is_error;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if matches!(kind, "user_message_chunk" | "agent_message_chunk")
            && update["content"]["type"] != "text"
        {
            self.history
                .add_notice("Non-text content remains in the native session.");
        }
        Ok(())
    }
    fn text(&mut self, role: SavedMessageRole, part: SavedHistoryPart) -> Result<(), HarnessError> {
        self.history.push_checked(
            SavedHistoryMessage {
                id: format!("acp-{}", self.history.messages.len()),
                role,
                timestamp_ms: 0,
                parts: vec![part],
            },
            &mut self.bytes,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Harness;
    #[test]
    fn replay_preserves_reasoning_tools_and_replaces_tool_shape_updates() {
        let mut replay = Replay::default();
        for update in [
            json!({"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"Remember teal"}}),
            json!({"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"Reason"}}),
            json!({"sessionUpdate":"tool_call","toolCallId":"t","kind":"execute","title":"pwd","status":"in_progress","rawInput":{"command":"pwd"}}),
            json!({"sessionUpdate":"tool_call_update","toolCallId":"t","kind":"execute","rawInput":{"command":"pwd"},"status":"completed","content":[{"type":"content","content":{"type":"text","text":"/fixture"}}]}),
            json!({"sessionUpdate":"agent_message_chunk","content":{"type":"image","data":"fixture"}}),
        ] {
            replay.push(&update).unwrap();
        }
        let history = replay.finish();
        assert_eq!(history.messages.len(), 3);
        assert!(history.notice.is_some());
        assert!(
            matches!(&history.messages[2].parts[0],SavedHistoryPart::Tool{output:Some(s),..}if s=="/fixture")
        );
    }
    #[tokio::test]
    #[ignore = "requires ZERON_HERMES_TEST_SOURCE, ZERON_HERMES_TEST_PYTHON, ZERON_HERMES_TEST_BINARY"]
    async fn native_hermes_fork_survives_restart_and_keeps_source_rows() {
        let home = tempfile::tempdir().unwrap();
        async fn fixture(home: &std::path::Path, action: &str, id: &str) -> String {
            let output =
                tokio::process::Command::new(std::env::var_os("ZERON_HERMES_TEST_PYTHON").unwrap())
                    .arg(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/tests/fixtures/hermes_saved.py"
                    ))
                    .args([action, id])
                    .env("HERMES_HOME", home)
                    .output()
                    .await
                    .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap()
        }
        fixture(home.path(), "create", "hermes-native-source").await;
        let before = fixture(home.path(), "snapshot", "hermes-native-source").await;
        let mut environment = crate::environment::EnvironmentSnapshot::default();
        for (name, value) in [
            ("OPENAI_API_KEY", "fixture-not-a-real-key"),
            ("OPENAI_BASE_URL", "http://127.0.0.1:9/v1"),
        ] {
            environment.entries.insert(
                name.into(),
                crate::environment::EnvironmentEntry::Set {
                    value: value.into(),
                },
            );
        }
        let harness = AcpHarness::hermes()
            .with_executable(std::env::var_os("ZERON_HERMES_TEST_BINARY").unwrap())
            .with_saved_session_home(home.path())
            .with_environment(std::sync::Arc::new(environment));
        let page = harness.saved_sessions(None).await.unwrap();
        assert_eq!(page.sessions.len(), 1);
        let copy = harness.copy_saved_session(&page.sessions[0]).await.unwrap();
        assert_ne!(copy.native_id, "hermes-native-source");
        let history = harness.saved_session_history(&copy).await.unwrap();
        assert!(
            history
                .messages
                .iter()
                .flat_map(|m| &m.parts)
                .any(|p| matches!(p,SavedHistoryPart::Text(s)if s=="Native Hermes answer"))
        );
        assert!(history.messages.iter().flat_map(|m| &m.parts).any(
            |p| matches!(p,SavedHistoryPart::Tool{output:Some(s),..}if s.contains("/fixture"))
        ));
        fixture(home.path(), "append", &copy.native_id).await;
        let resumed = harness.saved_session_history(&copy).await.unwrap();
        assert!(
            resumed
                .messages
                .iter()
                .flat_map(|m| &m.parts)
                .any(|p| matches!(p,SavedHistoryPart::Text(s)if s=="Independent continuation"))
        );
        assert_eq!(
            fixture(home.path(), "snapshot", "hermes-native-source").await,
            before
        );
    }
}
