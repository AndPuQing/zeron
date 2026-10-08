use std::collections::HashSet;
use std::future::Future;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zeron_proto::{AgentEvent, HarnessId};

use super::{CodexHarness, normalize};
use crate::jsonrpc::{Incoming, RpcClient};
use crate::process::{Command, Stdio};
use crate::saved_sessions::*;
use crate::{HarnessError, shutdown_child};

fn protocol(message: impl Into<String>) -> HarnessError {
    HarnessError::Protocol(message.into())
}

#[derive(Default, Serialize, Deserialize)]
struct ListCursor {
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    cursor: Option<String>,
}

impl CodexHarness {
    fn saved_root(&self) -> Result<std::path::PathBuf, HarnessError> {
        storage_root(self.saved_home.as_deref(), "CODEX_HOME", ".codex")
    }

    pub(super) fn saved_store_id(&self) -> Result<String, HarnessError> {
        Ok(store_id(HarnessId::Codex, &self.saved_root()?))
    }

    fn check_saved(&self, session: &SavedSession) -> Result<(), HarnessError> {
        if session.harness != HarnessId::Codex || session.store_id != self.saved_store_id()? {
            return Err(protocol(
                "The Codex history location changed. Scan again before importing.",
            ));
        }
        Ok(())
    }

    async fn saved_client<T, F, Fut>(&self, operation: F) -> Result<T, HarnessError>
    where
        F: FnOnce(RpcClient) -> Fut,
        Fut: Future<Output = Result<T, HarnessError>>,
    {
        let exe = self.resolve_executable()?;
        let mut command = Command::new(&exe);
        crate::compose_child_path(&mut command, &exe);
        command
            .arg("app-server")
            .env("CODEX_HOME", self.saved_root()?)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            shutdown_child(&mut child, self.kill_grace).await;
            return Err(protocol("Codex app-server has no stdio."));
        };
        let (client, mut incoming) = RpcClient::bounded(stdin, stdout, MAX_RECORD_BYTES);
        let responder = client.clone();
        let notifications = tokio::spawn(async move {
            while let Some(message) = incoming.recv().await {
                if let Incoming::Request { id, .. } = message {
                    responder.respond_error(
                        &id,
                        -32601,
                        "History import does not execute interactive requests.",
                    );
                }
            }
        });
        let work = async {
            client.request("initialize", json!({
                "clientInfo": {"name": "zerun-session-import", "title": "Zerun", "version": env!("CARGO_PKG_VERSION")},
                "capabilities": {"experimentalApi": true}
            })).await?;
            client.notify("initialized", None);
            operation(client).await
        };
        let result = tokio::time::timeout(Duration::from_secs(60), work)
            .await
            .unwrap_or_else(|_| Err(protocol("Codex history operation timed out. Try again.")));
        shutdown_child(&mut child, self.kill_grace).await;
        notifications.abort();
        result
    }

    pub(super) async fn list_saved(
        &self,
        cursor: Option<&str>,
    ) -> Result<SavedSessionPage, HarnessError> {
        let cursor: ListCursor = cursor
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| protocol("Invalid Codex history cursor."))?
            .unwrap_or_default();
        let identity = self.saved_store_id()?;
        self.saved_client(move |client| async move {
            let response = client.request("thread/list", json!({
                "limit": 50, "cursor": cursor.cursor, "sortKey": "updated_at",
                "sourceKinds": ["cli", "vscode", "appServer"], "modelProviders": [], "archived": cursor.archived
            })).await?;
            let data = response["data"].as_array().ok_or_else(|| protocol("Codex returned an invalid session list."))?;
            if data.len() > 50 { return Err(protocol("Codex returned an oversized session list.")); }
            let mut sessions = Vec::new();
            for thread in data {
                if thread.get("parentThreadId").is_some_and(|id| !id.is_null())
                    || thread["source"].as_object().is_some_and(|source| source.keys().any(|key| key.starts_with("subAgent")))
                    || thread["source"].as_str().is_some_and(|source| source.starts_with("subAgent")) { continue; }
                if let Some(session) = metadata(thread, &identity) { sessions.push(session); }
            }
            let next = response["nextCursor"].as_str().map(str::to_owned);
            if next.is_some() && (next == cursor.cursor || data.is_empty()) {
                return Err(protocol("Codex did not advance the session cursor."));
            }
            let next_cursor = if next.is_some() || !cursor.archived {
                Some(serde_json::to_string(&ListCursor { archived: next.is_none() || cursor.archived, cursor: next })
                    .map_err(|e| protocol(e.to_string()))?)
            } else { None };
            Ok(SavedSessionPage { sessions, next_cursor })
        }).await
    }

    pub(super) async fn read_saved(
        &self,
        session: &SavedSession,
    ) -> Result<SavedSessionHistory, HarnessError> {
        self.check_saved(session)?;
        let session = session.clone();
        self.saved_client(move |client| async move {
            let info = client.request("thread/read", json!({"threadId": session.native_id, "includeTurns": false})).await?;
            if info["thread"]["cwd"].as_str() != Some(session.cwd.as_str()) || info["thread"]["id"].as_str() != Some(session.native_id.as_str()) {
                return Err(protocol("The Codex session's working directory changed. Scan again."));
            }
            let mut history = SavedSessionHistory::default();
            let mut budget = 2usize;
            let mut cursor: Option<String> = None;
            let mut cursors = HashSet::new();
            let mut turns = HashSet::new();
            loop {
                let page = client.request("thread/turns/list", json!({
                    "threadId": session.native_id, "limit": 5, "cursor": cursor,
                    "sortDirection": "asc", "itemsView": "full"
                })).await.map_err(|error| protocol(format!("Could not read paginated Codex history. Update Codex if this API is unsupported: {error}")))?;
                let data = page["data"].as_array().ok_or_else(|| protocol("Codex returned an invalid history page."))?;
                if data.len() > 5 { return Err(protocol("Codex returned an oversized history page.")); }
                for turn in data {
                    if turn["itemsView"].as_str().is_some_and(|view| view != "full") {
                        return Err(protocol("Codex did not return full history items. Update Codex and retry."));
                    }
                    let id = turn["id"].as_str().ok_or_else(|| protocol("A Codex turn has no identity."))?;
                    if !turns.insert(id.to_owned()) { continue; }
                    let items = turn["items"].as_array().ok_or_else(|| protocol("Codex did not return history items."))?;
                    for (index, item) in items.iter().enumerate() {
                        let mut item_notice = None;
                        if let Some(mut message) = history_item(item, &session, id, index, &mut item_notice) {
                            message.timestamp_ms = time_ms(item).or_else(|| time_ms(turn)).unwrap_or(session.created_at_ms);
                            history.push_checked(message, &mut budget)?;
                        }
                        if let Some(notice) = item_notice { history.add_notice(&notice); }
                    }
                }
                cursor = page["nextCursor"].as_str().map(str::to_owned);
                let Some(next) = &cursor else { break };
                if data.is_empty() || !cursors.insert(next.clone()) { return Err(protocol("Codex repeated a history cursor.")); }
            }
            Ok(history)
        }).await
    }

    pub(super) async fn copy_saved(
        &self,
        session: &SavedSession,
    ) -> Result<SavedSession, HarnessError> {
        self.check_saved(session)?;
        let mut session = session.clone();
        self.saved_client(move |client| async move {
            let info = client.request("thread/read", json!({"threadId": session.native_id, "includeTurns": false})).await?;
            if info["thread"]["cwd"].as_str() != Some(session.cwd.as_str()) {
                return Err(protocol("The Codex session's working directory changed. Scan again."));
            }
            let page = client.request("thread/turns/list", json!({
                "threadId": session.native_id, "limit": 1, "sortDirection": "desc", "itemsView": "notLoaded"
            })).await?;
            let turns = page["data"].as_array().ok_or_else(|| protocol("Codex could not verify the copy boundary."))?;
            let last = turns.first();
            if last.and_then(|turn| turn["id"].as_str()).is_none_or(str::is_empty) {
                return Err(protocol("The Codex session has no completed conversation to copy."));
            }
            if let Some(last) = last {
                let complete = match last["status"].as_str() {
                    Some("completed" | "interrupted" | "failed" | "aborted") => true,
                    None => last["completedAt"].as_i64().is_some_and(|time| time > 0),
                    _ => false,
                };
                if !complete { return Err(protocol("Finish or stop the source Codex turn before importing.")); }
            }
            let mut params = json!({"threadId": session.native_id, "cwd": session.cwd,
                "excludeTurns": true, "deferGoalContinuation": true});
            if let Some(id) = last.and_then(|turn| turn["id"].as_str()) { params["lastTurnId"] = id.into(); }
            let response = client.request("thread/fork", params).await?;
            let id = response["thread"]["id"].as_str().filter(|id| !id.is_empty() && *id != session.native_id)
                .ok_or_else(|| protocol("Codex did not create an independent native session copy."))?;
            session.native_id = id.to_owned();
            Ok(session)
        }).await
    }
}

fn metadata(thread: &Value, identity: &str) -> Option<SavedSession> {
    let native_id = thread["id"].as_str()?.to_owned();
    let cwd = thread["cwd"].as_str()?.to_owned();
    let title = thread["name"]
        .as_str()
        .filter(|name| !name.trim().is_empty())
        .or_else(|| thread["preview"].as_str())
        .unwrap_or(&native_id)
        .chars()
        .take(200)
        .collect();
    Some(SavedSession {
        harness: HarnessId::Codex,
        native_id,
        store_id: identity.into(),
        title,
        cwd,
        created_at_ms: thread["createdAt"].as_i64()?.checked_mul(1000)?,
        updated_at_ms: thread["updatedAt"].as_i64()?.checked_mul(1000)?,
        model: thread["model"].as_str().map(str::to_owned),
        locator: None,
    })
}

fn time_ms(value: &Value) -> Option<i64> {
    ["createdAt", "startedAt", "timestamp"]
        .iter()
        .find_map(|key| {
            value[key]
                .as_i64()
                .and_then(|time| {
                    if time < 10_000_000_000 {
                        time.checked_mul(1000)
                    } else {
                        Some(time)
                    }
                })
                .or_else(|| {
                    value[key]
                        .as_str()
                        .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
                        .map(|t| t.timestamp_millis())
                })
        })
}

fn history_item(
    item: &Value,
    session: &SavedSession,
    turn: &str,
    index: usize,
    notice: &mut Option<String>,
) -> Option<SavedHistoryMessage> {
    let id = item["id"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{turn}:{index}"));
    let mut role = SavedMessageRole::Assistant;
    let kind = item["type"].as_str().unwrap_or("");
    let parts = match kind {
        "userMessage" => {
            role = SavedMessageRole::User;
            item["content"].as_array()?.iter().filter_map(|part| {
                if part["type"] == "text" { part["text"].as_str().map(|text| SavedHistoryPart::Text(text.into())) }
                else if matches!(part["type"].as_str(), Some("image" | "localImage")) {
                    *notice = Some("Historical images remain in the native copy; this history view shows image placeholders.".into());
                    Some(SavedHistoryPart::Text("[Image attachment retained in the native session]".into()))
                } else {
                    *notice = Some("Some native input attachments are not displayed; the native copy retains them.".into());
                    Some(SavedHistoryPart::Text("[Native input retained in the session]".into()))
                }
            }).collect()
        }
        "agentMessage" => vec![SavedHistoryPart::Text(item["text"].as_str()?.into())],
        "reasoning" => {
            let text = ["summary", "content"]
                .into_iter()
                .filter_map(|key| item[key].as_array())
                .flatten()
                .filter_map(|value| value.as_str().or_else(|| value["text"].as_str()))
                .collect::<Vec<_>>()
                .join("\n");
            vec![SavedHistoryPart::Reasoning(text)]
        }
        "contextCompaction" => {
            role = SavedMessageRole::System;
            vec![SavedHistoryPart::Text(
                "Native session context was compacted here.".into(),
            )]
        }
        _ => {
            let events = normalize::map_item(normalize::Phase::Started, item)
                .into_iter()
                .chain(normalize::map_item(normalize::Phase::Completed, item));
            let mut parts = Vec::new();
            for event in events {
                match event {
                    AgentEvent::ToolCall { call, .. } => {
                        if parts.is_empty() {
                            parts.push(SavedHistoryPart::Tool {
                                call,
                                output: None,
                                is_error: false,
                            });
                        }
                    }
                    AgentEvent::ToolResult {
                        output, is_error, ..
                    } => {
                        if let Some(SavedHistoryPart::Tool {
                            output: saved_output,
                            is_error: saved_error,
                            ..
                        }) = parts.first_mut()
                        {
                            *saved_output = output;
                            *saved_error = is_error;
                        }
                    }
                    _ => {}
                }
            }
            if parts.is_empty() {
                *notice = Some("Some provider-internal artifacts are not displayed; the native copy retains its context.".into());
            }
            parts
        }
    };
    if parts.is_empty() {
        return None;
    }
    Some(SavedHistoryMessage {
        id,
        role,
        timestamp_ms: session.created_at_ms,
        parts,
    })
}
