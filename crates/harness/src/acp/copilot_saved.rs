//! Copilot's native headless RPC owns discovery and fork. Its persistent event
//! log supplies the complete transcript without opening or prompting the source.
use super::{
    AcpHarness,
    saved::{error, timestamp},
};
use crate::{
    HarnessError,
    process::{ChildStdin, ChildStdout, Stdio},
    saved_sessions::*,
};
use serde_json::{Value, json};
use std::{collections::HashMap, path::PathBuf, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use zeron_proto::{HarnessId, ToolCall};

impl AcpHarness {
    async fn copilot_request(&self, method: &str, params: Value) -> Result<Value, HarnessError> {
        let mut command = self
            .cli_command(&["--headless", "--stdio", "--log-level", "none"])
            .await?;
        command
            .env("COPILOT_HOME", self.import_root()?)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        crate::process::owned::configure(&mut command);
        let mut child = crate::process::owned::Child::new(command.spawn()?);
        let mut wire = Wire {
            input: child
                .stdin
                .take()
                .ok_or_else(|| error("Missing Copilot stdin."))?,
            output: BufReader::new(
                child
                    .stdout
                    .take()
                    .ok_or_else(|| error("Missing Copilot stdout."))?,
            ),
            id: 0,
            bytes: 0,
        };
        let result = tokio::time::timeout(Duration::from_secs(90), wire.request(method, params))
            .await
            .map_err(|_| error("Copilot history request timed out."));
        child.shutdown(self.kill_grace).await;
        result?
    }
    pub(super) async fn copilot_list(
        &self,
        cursor: Option<&str>,
    ) -> Result<SavedSessionPage, HarnessError> {
        let offset = cursor
            .map(str::parse::<usize>)
            .transpose()
            .map_err(|_| error("Invalid Copilot history cursor."))?
            .unwrap_or(0);
        let response = self
            .copilot_request("sessions.list", json!({"source":"local"}))
            .await?;
        let rows = response["sessions"]
            .as_array()
            .ok_or_else(|| error("Invalid Copilot history list."))?;
        if rows.len() > 5000 || offset > rows.len() {
            return Err(error(
                "Copilot history changed or exceeds the scan limit. Scan again.",
            ));
        }
        let end = (offset + 50).min(rows.len());
        let mut sessions = vec![];
        for row in &rows[offset..end] {
            if row["isRemote"] == true || row["isDetached"] == true {
                continue;
            }
            let id = row["sessionId"]
                .as_str()
                .filter(|s| uuid::Uuid::parse_str(s).is_ok())
                .ok_or_else(|| error("Invalid Copilot session id."))?;
            let mut session = SavedSession {
                harness: HarnessId::Copilot,
                native_id: id.into(),
                store_id: self.import_store_id()?,
                title: row["name"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .or_else(|| row["summary"].as_str())
                    .unwrap_or("Copilot session")
                    .into(),
                cwd: row["context"]["cwd"].as_str().unwrap_or_default().into(),
                created_at_ms: timestamp(&row["startTime"]),
                updated_at_ms: timestamp(&row["modifiedTime"]),
                model: None,
                locator: None,
                running: false,
            };
            let events = self.copilot_events(&session)?;
            session.running = busy(&events);
            session.model = events
                .iter()
                .rev()
                .find_map(|e| e["data"]["model"].as_str())
                .map(str::to_owned);
            sessions.push(session);
        }
        Ok(SavedSessionPage {
            sessions,
            next_cursor: (end < rows.len()).then(|| end.to_string()),
        })
    }
    fn copilot_event_path(&self, session: &SavedSession) -> Result<PathBuf, HarnessError> {
        self.check_import(session)?;
        uuid::Uuid::parse_str(&session.native_id)
            .map_err(|_| error("Invalid Copilot session id."))?;
        let root = self.import_root()?;
        let path = root
            .join("session-state")
            .join(&session.native_id)
            .join("events.jsonl");
        for component in [path.parent().unwrap(), path.as_path()] {
            if std::fs::symlink_metadata(component)?
                .file_type()
                .is_symlink()
            {
                return Err(error("Copilot history must not be a symbolic link."));
            }
        }
        if !path.canonicalize()?.starts_with(root) {
            return Err(error("Copilot history is outside its store."));
        }
        Ok(path)
    }
    fn copilot_events(&self, session: &SavedSession) -> Result<Vec<Value>, HarnessError> {
        read_events(&self.copilot_event_path(session)?)
    }
    pub(super) async fn copilot_copy(
        &self,
        session: &SavedSession,
    ) -> Result<SavedSession, HarnessError> {
        let before = self.copilot_events(session)?;
        if busy(&before) {
            return Err(running());
        }
        let response = self
            .copilot_request("sessions.fork", json!({"sessionId":session.native_id}))
            .await?;
        let id = response["sessionId"]
            .as_str()
            .filter(|id| *id != session.native_id && uuid::Uuid::parse_str(id).is_ok())
            .ok_or_else(|| {
                error("Copilot did not create an independent native fork. Update Copilot CLI.")
            })?;
        let copy = SavedSession {
            native_id: id.into(),
            ..session.clone()
        };
        if self.copilot_events(session)? != before {
            return Err(error(
                "The source changed while Copilot was copying it. Finish its turn and retry.",
            ));
        }
        // A native fork is durable before the headless process exits. Never
        // publish a receipt for a missing or empty clone.
        if self.copilot_events(&copy)?.is_empty() {
            return Err(error("The Copilot fork was not persisted."));
        }
        Ok(copy)
    }
    pub(super) async fn copilot_history(
        &self,
        session: &SavedSession,
    ) -> Result<SavedSessionHistory, HarnessError> {
        history(&self.copilot_events(session)?)
    }
}
fn read_events(path: &std::path::Path) -> Result<Vec<Value>, HarnessError> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_SOURCE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(error("Copilot history exceeds the import limit."));
    }
    bytes
        .split(|b| *b == b'\n')
        .filter(|l| !l.iter().all(u8::is_ascii_whitespace))
        .map(|line| {
            if line.len() > MAX_RECORD_BYTES {
                return Err(error("Copilot history record exceeds the import limit."));
            }
            serde_json::from_slice(line).map_err(|e| error(format!("Invalid Copilot history: {e}")))
        })
        .collect()
}
fn busy(events: &[Value]) -> bool {
    let mut active = false;
    for event in events {
        // Child-agent turns can finish while the root conversation is still
        // working. Only the root's boundary can make the source selectable.
        if event["agentId"].as_str().is_some()
            || event["data"]["parentToolCallId"].as_str().is_some()
        {
            continue;
        }
        match event["type"].as_str().unwrap_or_default() {
            "user.message" | "assistant.turn_start" => active = true,
            "assistant.turn_end" | "abort" | "session.shutdown" => active = false,
            _ => {}
        }
    }
    active
}
fn history(events: &[Value]) -> Result<SavedSessionHistory, HarnessError> {
    let mut history = SavedSessionHistory::default();
    let mut bytes = 0;
    let mut tools = HashMap::new();
    for (index, event) in events.iter().enumerate() {
        let data = &event["data"];
        let kind = event["type"].as_str().unwrap_or_default();
        let mut parts = vec![];
        let role = match kind {
            "user.message" => SavedMessageRole::User,
            "system.message" => SavedMessageRole::System,
            "assistant.message" | "assistant.reasoning" | "tool.execution_start" => {
                SavedMessageRole::Assistant
            }
            "tool.execution_complete" => {
                if let Some(&(m, p)) = data["toolCallId"].as_str().and_then(|id| tools.get(id)) {
                    let message: &mut SavedHistoryMessage = &mut history.messages[m];
                    if let SavedHistoryPart::Tool {
                        output, is_error, ..
                    } = &mut message.parts[p]
                    {
                        *output = data["result"]["content"]
                            .as_str()
                            .map(str::to_owned)
                            .or_else(|| data["result"].as_str().map(str::to_owned));
                        *is_error = data["success"] == false;
                        if output.is_none() && !data["error"].is_null() {
                            *output = Some(data["error"].to_string());
                        }
                    }
                }
                continue;
            }
            _ => continue,
        };
        if kind == "tool.execution_start" {
            parts.push(SavedHistoryPart::Tool {
                call: ToolCall::Unknown {
                    name: data["toolName"].as_str().unwrap_or("Tool").into(),
                    input: data.get("arguments").cloned(),
                },
                output: None,
                is_error: false,
            });
            if let Some(id) = data["toolCallId"].as_str() {
                tools.insert(id.to_owned(), (history.messages.len(), 0));
            }
        } else if kind == "assistant.reasoning" {
            if let Some(text) = data["content"]
                .as_str()
                .or_else(|| data["reasoningText"].as_str())
            {
                parts.push(SavedHistoryPart::Reasoning(text.into()));
            }
        } else if let Some(text) = data["content"].as_str() {
            parts.push(SavedHistoryPart::Text(text.into()));
        }
        if data["attachments"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
        {
            history.add_notice("Attachments remain in the native Copilot session.");
        }
        if !parts.is_empty() {
            history.push_checked(
                SavedHistoryMessage {
                    id: format!("copilot-{index}"),
                    role,
                    timestamp_ms: timestamp(&event["timestamp"]),
                    parts,
                },
                &mut bytes,
            )?;
        }
    }
    history.check_budget()?;
    Ok(history)
}

/// Copilot's headless API uses Content-Length framing, unlike ACP JSONL.
struct Wire {
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    id: u64,
    bytes: usize,
}
impl Wire {
    async fn request(&mut self, method: &str, params: Value) -> Result<Value, HarnessError> {
        self.id += 1;
        let id = self.id;
        self.write(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?;
        loop {
            let mut length = None;
            let mut headers = 0;
            loop {
                let mut line = String::new();
                (&mut self.output).take(8193).read_line(&mut line).await?;
                headers += line.len();
                if line.is_empty() || headers > 8192 {
                    return Err(error("Invalid Copilot RPC headers."));
                }
                if line == "\r\n" || line == "\n" {
                    break;
                }
                if let Some((key, value)) = line.split_once(':') {
                    if key.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse::<usize>().ok();
                    }
                }
            }
            let length = length
                .filter(|n| *n <= MAX_HISTORY_BYTES)
                .ok_or_else(|| error("Copilot RPC response exceeds the import limit."))?;
            self.bytes += length;
            if self.bytes > MAX_SOURCE_BYTES as usize {
                return Err(error("Copilot RPC stream exceeds the import limit."));
            }
            let mut bytes = vec![0; length];
            self.output.read_exact(&mut bytes).await?;
            let frame: Value = serde_json::from_slice(&bytes).map_err(|e| error(e.to_string()))?;
            if frame["id"] == id && frame.get("method").is_none() {
                if let Some(e) = frame.get("error") {
                    return Err(error(format!("Copilot history: {e}")));
                }
                return frame
                    .get("result")
                    .cloned()
                    .ok_or_else(|| error("Missing Copilot RPC result."));
            }
            if frame.get("method").is_some() && frame.get("id").is_some() {
                self.write(&json!({"jsonrpc":"2.0","id":frame["id"],"error":{"code":-32601,"message":"Tools are unavailable during session import"}})).await?;
            }
        }
    }
    async fn write(&mut self, frame: &Value) -> Result<(), HarnessError> {
        let bytes = serde_json::to_vec(frame).map_err(|e| error(e.to_string()))?;
        self.input
            .write_all(format!("Content-Length: {}\r\n\r\n", bytes.len()).as_bytes())
            .await?;
        self.input.write_all(&bytes).await?;
        self.input.flush().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Harness;
    fn fixture(id: &str, cwd: &PathBuf) -> Vec<Value> {
        let mut rows = vec![];
        let mut previous = Value::Null;
        for (kind, data) in [
            (
                "session.start",
                json!({"sessionId":id,"version":1,"producer":"copilot","copilotVersion":"1.0.98","startTime":"2026-10-17T10:00:00Z","context":{"cwd":cwd}}),
            ),
            (
                "user.message",
                json!({"content":"Remember teal","attachments":[{"type":"file","path":"/fixture.png","displayName":"fixture.png"}]}),
            ),
            ("assistant.turn_start", json!({"turnId":"0"})),
            (
                "tool.execution_start",
                json!({"toolCallId":"tool-1","toolName":"shell","arguments":{"command":"pwd"}}),
            ),
            (
                "tool.execution_complete",
                json!({"toolCallId":"tool-1","success":true,"result":{"content":"/fixture"}}),
            ),
            (
                "assistant.reasoning",
                json!({"reasoningId":"reasoning-0","content":"Reasoning retained"}),
            ),
            (
                "assistant.message",
                json!({"messageId":"answer","content":"Teal retained","model":"fixture-model"}),
            ),
            ("assistant.turn_end", json!({"turnId":"0"})),
        ] {
            let id = uuid::Uuid::new_v4().to_string();
            rows.push(json!({"type":kind,"data":data,"id":id,"parentId":previous,"timestamp":"2026-10-17T10:00:00Z"}));
            previous = Value::String(id);
        }
        rows
    }
    fn write_events(path: &std::path::Path, rows: &[Value]) {
        let bytes = rows
            .iter()
            .map(|v| serde_json::to_string(v).unwrap() + "\n")
            .collect::<String>();
        std::fs::write(path, bytes).unwrap();
    }
    #[test]
    fn completed_and_interrupted_turns_keep_tools_and_reject_pending_turns() {
        let mut rows = fixture("source", &PathBuf::from("/fixture"));
        assert!(!busy(&rows));
        let history = history(&rows).unwrap();
        assert!(history.messages.iter().flat_map(|m| &m.parts).any(
            |p| matches!(p,SavedHistoryPart::Tool{output:Some(s),is_error:false,..}if s=="/fixture")
        ));
        assert!(
            history
                .messages
                .iter()
                .flat_map(|m| &m.parts)
                .any(|p| matches!(p,SavedHistoryPart::Reasoning(s)if s=="Reasoning retained"))
        );
        assert!(history.notice.is_some());
        rows.push(json!({"type":"user.message","data":{"content":"pending"}}));
        assert!(busy(&rows));
        rows.push(
            json!({"type":"assistant.turn_end","agentId":"child","data":{"turnId":"child-turn"}}),
        );
        assert!(busy(&rows));
        rows.push(json!({"type":"abort"}));
        assert!(!busy(&rows));
    }
    #[tokio::test]
    #[ignore = "requires ZERON_COPILOT_TEST_BINARY, native CLI 1.0.98 or later"]
    async fn native_fork_is_durable_and_source_events_are_unchanged() {
        let home = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let cwd = home.path().to_path_buf();
        let dir = home.path().join("session-state").join(&id);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("events.jsonl");
        let rows = fixture(&id, &cwd);
        write_events(&path, &rows);
        std::fs::write(dir.join("workspace.yaml"),format!("id: {id}\ncwd: {}\nsummary: Native fixture\ncreated_at: 2026-10-17T10:00:00Z\nupdated_at: 2026-10-17T10:00:00Z\n",cwd.display())).unwrap();
        let before = std::fs::read(&path).unwrap();
        let harness = AcpHarness::copilot()
            .with_executable(std::env::var_os("ZERON_COPILOT_TEST_BINARY").unwrap())
            .with_saved_session_home(home.path());
        let page = harness.saved_sessions(None).await.unwrap();
        assert_eq!(page.sessions.len(), 1);
        let source = &page.sessions[0];
        assert!(!source.running);
        let mut pending = rows.clone();
        pending.push(json!({"type":"user.message","data":{"content":"Pending"}}));
        write_events(&path, &pending);
        assert!(harness.copy_saved_session(source).await.is_err());
        std::fs::write(&path, &before).unwrap();
        let copy = harness.copy_saved_session(source).await.unwrap();
        assert_ne!(copy.native_id, id);
        assert_eq!(copy.cwd, source.cwd);
        assert!(
            harness
                .saved_session_history(&copy)
                .await
                .unwrap()
                .messages
                .iter()
                .flat_map(|m| &m.parts)
                .any(|p| matches!(p,SavedHistoryPart::Text(s)if s=="Teal retained"))
        );
        // The normal ACP resume surface can load the fork in a separate process.
        let mut cmd = harness.cli_command(&["--acp"]).await.unwrap();
        cmd.env("COPILOT_HOME", home.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        crate::process::owned::configure(&mut cmd);
        let mut child = crate::process::owned::Child::new(cmd.spawn().unwrap());
        let (client, mut incoming) = crate::jsonrpc::RpcClient::bounded(
            child.stdin.take().unwrap(),
            child.stdout.take().unwrap(),
            MAX_RECORD_BYTES,
        );
        let result = tokio::time::timeout(Duration::from_secs(60), async {
            super::super::saved::collect(
                &client,
                &mut incoming,
                "initialize",
                super::super::initialize_params(HarnessId::Copilot),
                None,
            )
            .await
            .unwrap();
            super::super::saved::collect(
                &client,
                &mut incoming,
                "session/load",
                json!({"sessionId":copy.native_id,"cwd":copy.cwd,"mcpServers":[]}),
                Some(&copy.native_id),
            )
            .await
        })
        .await;
        child.shutdown(Duration::from_millis(100)).await;
        let (_, replay) = result.unwrap().unwrap();
        assert!(!replay.messages.is_empty());
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
}
