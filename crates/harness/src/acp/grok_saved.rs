//! Grok 1.x's documented per-session directory format. Preserve native model
//! history, compaction, plans and checkpoints; only the copied identity changes.
use super::{
    AcpHarness,
    saved::{Replay, error, timestamp},
};
use crate::{HarnessError, saved_sessions::*};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use zeron_proto::{HarnessId, ToolCall};

impl AcpHarness {
    pub(super) async fn grok_list(
        &self,
        cursor: Option<&str>,
    ) -> Result<SavedSessionPage, HarnessError> {
        let root = self.import_root()?;
        let identity = self.import_store_id()?;
        let offset = cursor
            .map(str::parse::<usize>)
            .transpose()
            .map_err(|_| error("Invalid Grok history cursor."))?
            .unwrap_or(0);
        tokio::task::spawn_blocking(move || {
            let mut rows = Vec::new();
            for group in directories(&root.join("sessions"))? {
                for path in directories(&group)? {
                    if path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .is_none_or(|id| uuid::Uuid::parse_str(id).is_err())
                    {
                        continue;
                    }
                    if path.join("summary.json").is_file() {
                        let summary: Value =
                            serde_json::from_slice(&read(&path.join("summary.json"))?)
                                .map_err(|e| error(e.to_string()))?;
                        if summary["hidden"] == true {
                            continue;
                        }
                        let session = metadata(&root, &path, &summary, &identity)?;
                        if path.join("chat_history.jsonl").is_file()
                            || path.join("updates.jsonl").is_file()
                        {
                            rows.push(session);
                        }
                    }
                    if rows.len() > 5000 {
                        return Err(error("Grok history exceeds the scan limit."));
                    }
                }
            }
            rows.sort_by(|a, b| {
                b.updated_at_ms
                    .cmp(&a.updated_at_ms)
                    .then(a.native_id.cmp(&b.native_id))
            });
            if offset > rows.len() {
                return Err(error("Grok history changed. Scan again."));
            }
            let end = (offset + 50).min(rows.len());
            Ok(SavedSessionPage {
                sessions: rows[offset..end].to_vec(),
                next_cursor: (end < rows.len()).then(|| end.to_string()),
            })
        })
        .await
        .map_err(|e| error(e.to_string()))?
    }
    pub(super) async fn grok_history(
        &self,
        session: &SavedSession,
    ) -> Result<SavedSessionHistory, HarnessError> {
        self.check_import(session)?;
        let root = self.import_root()?;
        let session = session.clone();
        tokio::task::spawn_blocking(move || {
            let path = locate(&root, &session)?;
            if path.join("updates.jsonl").is_file() {
                let mut replay = Replay::default();
                for row in records(&path.join("updates.jsonl"))? {
                    replay.push(row.get("update").unwrap_or(&row))?;
                }
                let history = replay.finish();
                if !history.messages.is_empty() {
                    history.check_budget()?;
                    return Ok(history);
                }
            }
            chat_history(&records(&path.join("chat_history.jsonl"))?)
        })
        .await
        .map_err(|e| error(e.to_string()))?
    }
    pub(super) async fn grok_copy(
        &self,
        session: &SavedSession,
    ) -> Result<SavedSession, HarnessError> {
        self.check_import(session)?;
        let root = self.import_root()?;
        let session = session.clone();
        tokio::task::spawn_blocking(move || {
            let path = locate(&root, &session)?;
            let mut before = vec![];
            snapshot(&path, &path, &mut before, &mut 0, 0)?;
            let summary_bytes = before
                .iter()
                .find(|(p, _)| p == Path::new("summary.json"))
                .ok_or_else(|| error("Missing Grok summary."))?
                .1
                .clone();
            let mut summary: Value =
                serde_json::from_slice(&summary_bytes).map_err(|e| error(e.to_string()))?;
            let latest = metadata(&root, &path, &summary, &session.store_id)?;
            if latest.running {
                return Err(running());
            }
            if latest.cwd != session.cwd {
                return Err(error("Grok session working directory changed. Scan again."));
            }
            let id = uuid::Uuid::new_v4().to_string();
            let parent = path.parent().unwrap();
            let stage = parent.join(format!(".zeron-import-{id}"));
            let destination = parent.join(&id);
            fs::create_dir(&stage)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&stage, fs::Permissions::from_mode(0o700))?;
            }
            let result = (|| {
                for (relative, bytes) in &before {
                    let target = stage.join(relative);
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(&target, bytes)?;
                }
                summary["info"]["id"] = Value::String(id.clone());
                // These describe a dependency on another native session. The
                // copied directory already contains the complete local state.
                for key in [
                    "parent_session_id",
                    "fork_context_source",
                    "fork_parent_prompt_id",
                    "pending_cwd_switch_reminder",
                    "destination_cwd",
                ] {
                    summary.as_object_mut().unwrap().remove(key);
                }
                fs::write(
                    stage.join("summary.json"),
                    serde_json::to_vec_pretty(&summary).map_err(|e| error(e.to_string()))?,
                )?;
                if stage.join("updates.jsonl").is_file() {
                    let mut output = Vec::new();
                    for mut update in records(&stage.join("updates.jsonl"))? {
                        if update["sessionId"] == session.native_id {
                            update["sessionId"] = Value::String(id.clone());
                        }
                        serde_json::to_writer(&mut output, &update)
                            .map_err(|e| error(e.to_string()))?;
                        output.push(b'\n');
                    }
                    fs::write(stage.join("updates.jsonl"), output)?;
                }
                let mut after = vec![];
                snapshot(&path, &path, &mut after, &mut 0, 0)?;
                if before != after || active(&root, &session.native_id)? {
                    return Err(error(
                        "Grok history changed while copying. Finish its turn and retry.",
                    ));
                }
                fs::rename(&stage, &destination)?;
                Ok(SavedSession {
                    native_id: id.clone(),
                    locator: Some(destination.clone()),
                    running: false,
                    ..session
                })
            })();
            if result.is_err() {
                let _ = fs::remove_dir_all(&stage);
            }
            result
        })
        .await
        .map_err(|e| error(e.to_string()))?
    }
}
fn directories(root: &Path) -> Result<Vec<PathBuf>, HarnessError> {
    if !root.exists() {
        return Ok(vec![]);
    }
    if fs::symlink_metadata(root)?.file_type().is_symlink() {
        return Err(error("Grok history must not traverse symbolic links."));
    }
    let mut dirs = vec![];
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            dirs.push(entry.path());
        }
    }
    dirs.sort();
    Ok(dirs)
}
fn read(path: &Path) -> Result<Vec<u8>, HarnessError> {
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(error("Grok history must not contain symbolic links."));
    }
    let mut bytes = vec![];
    fs::File::open(path)?
        .take(MAX_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(error("Grok history exceeds the import limit."));
    }
    Ok(bytes)
}
fn records(path: &Path) -> Result<Vec<Value>, HarnessError> {
    read(path)?
        .split(|b| *b == b'\n')
        .filter(|s| !s.iter().all(u8::is_ascii_whitespace))
        .map(|line| {
            if line.len() > MAX_RECORD_BYTES {
                return Err(error("Grok record exceeds the import limit."));
            }
            serde_json::from_slice(line).map_err(|e| error(format!("Invalid Grok history: {e}")))
        })
        .collect()
}
fn active(root: &Path, id: &str) -> Result<bool, HarnessError> {
    let path = root.join("active_sessions.json");
    if !path.exists() {
        return Ok(false);
    }
    let value: Value = serde_json::from_slice(&read(&path)?)
        .map_err(|e| error(format!("Invalid Grok active-session index: {e}")))?;
    // Be conservative for an open native session, including a live idle pager:
    // it can start another turn without consulting Zeron's execution lease.
    fn contains(v: &Value, id: &str) -> bool {
        match v {
            Value::String(s) => s == id,
            Value::Array(a) => a.iter().any(|v| contains(v, id)),
            Value::Object(o) => o.values().any(|v| contains(v, id)),
            _ => false,
        }
    }
    Ok(contains(&value, id))
}
fn metadata(
    root: &Path,
    path: &Path,
    value: &Value,
    identity: &str,
) -> Result<SavedSession, HarnessError> {
    let id = value["info"]["id"]
        .as_str()
        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
        .ok_or_else(|| error("Invalid Grok session identity."))?;
    if path.file_name().and_then(|s| s.to_str()) != Some(id)
        || value["chat_format_version"].as_u64() != Some(1)
    {
        return Err(error(
            "Unsupported Grok saved-session format. Update Zeron before importing.",
        ));
    }
    Ok(SavedSession {
        harness: HarnessId::Grok,
        native_id: id.into(),
        store_id: identity.into(),
        title: value["generated_title"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| value["session_summary"].as_str().filter(|s| !s.is_empty()))
            .unwrap_or("Grok session")
            .into(),
        cwd: value["info"]["cwd"].as_str().unwrap_or_default().into(),
        created_at_ms: timestamp(&value["created_at"]),
        updated_at_ms: timestamp(&value["updated_at"]),
        model: value["current_model_id"].as_str().map(str::to_owned),
        locator: Some(path.into()),
        running: active(root, id)?,
    })
}
fn locate(root: &Path, session: &SavedSession) -> Result<PathBuf, HarnessError> {
    let path = session
        .locator
        .as_ref()
        .ok_or_else(|| error("Missing Grok history location."))?;
    let base = root.join("sessions").canonicalize()?;
    if path.file_name().and_then(|s| s.to_str()) != Some(session.native_id.as_str())
        || path.canonicalize()? != *path
        || !path.starts_with(&base)
        || path.strip_prefix(base).unwrap().components().count() != 2
    {
        return Err(error("Invalid Grok history location."));
    }
    Ok(path.clone())
}
fn snapshot(
    base: &Path,
    dir: &Path,
    out: &mut Vec<(PathBuf, Vec<u8>)>,
    bytes: &mut u64,
    depth: usize,
) -> Result<(), HarnessError> {
    if depth > 12 {
        return Err(error("Grok history nesting exceeds the import limit."));
    }
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(error("Grok history contains a symbolic link."));
        }
        if kind.is_dir() {
            snapshot(base, &path, out, bytes, depth + 1)?;
        } else if kind.is_file() {
            if path.extension().is_some_and(|s| s == "lock") {
                continue;
            }
            *bytes += entry.metadata()?.len();
            if *bytes > MAX_SOURCE_BYTES || out.len() >= 20000 {
                return Err(error("Grok native history exceeds the import limit."));
            }
            out.push((path.strip_prefix(base).unwrap().into(), read(&path)?));
        } else {
            return Err(error("Grok history contains an unsupported file."));
        }
    }
    Ok(())
}
fn chat_history(records: &[Value]) -> Result<SavedSessionHistory, HarnessError> {
    let mut history = SavedSessionHistory::default();
    let mut bytes = 0;
    let mut tools = HashMap::new();
    for (i, row) in records.iter().enumerate() {
        let mut parts = vec![];
        let role = match row["type"].as_str().unwrap_or_default() {
            "user" => SavedMessageRole::User,
            "assistant" | "reasoning" => SavedMessageRole::Assistant,
            "tool_result" => {
                if let Some(&m) = row["tool_call_id"].as_str().and_then(|id| tools.get(id)) {
                    let message: &mut SavedHistoryMessage = &mut history.messages[m];
                    if let SavedHistoryPart::Tool { output, .. } = &mut message.parts[0] {
                        *output = row["content"].as_str().map(str::to_owned);
                    }
                }
                continue;
            }
            _ => continue,
        };
        if row["type"] == "reasoning" {
            if let Some(summary) = row["summary"].as_array() {
                for value in summary {
                    if let Some(text) = value["text"].as_str() {
                        parts.push(SavedHistoryPart::Reasoning(text.into()));
                    }
                }
            }
        } else if let Some(text) = row["content"].as_str() {
            parts.push(SavedHistoryPart::Text(text.into()));
        } else if let Some(content) = row["content"].as_array() {
            for block in content {
                if let Some(text) = block["text"].as_str() {
                    parts.push(SavedHistoryPart::Text(text.into()));
                } else {
                    history.add_notice("Non-text content remains in the native Grok session.");
                }
            }
        }
        if !parts.is_empty() {
            history.push_checked(
                SavedHistoryMessage {
                    id: format!("grok-{i}"),
                    role,
                    timestamp_ms: 0,
                    parts,
                },
                &mut bytes,
            )?;
        }
        if let Some(calls) = row["tool_calls"].as_array() {
            for call in calls {
                if let Some(id) = call["id"].as_str() {
                    tools.insert(id.to_owned(), history.messages.len());
                }
                history.push_checked(
                    SavedHistoryMessage {
                        id: format!("grok-tool-{}", history.messages.len()),
                        role: SavedMessageRole::Assistant,
                        timestamp_ms: 0,
                        parts: vec![SavedHistoryPart::Tool {
                            call: ToolCall::Unknown {
                                name: call["name"].as_str().unwrap_or("Tool").into(),
                                input: call["arguments"]
                                    .as_str()
                                    .and_then(|s| serde_json::from_str(s).ok()),
                            },
                            output: None,
                            is_error: false,
                        }],
                    },
                    &mut bytes,
                )?;
            }
        }
    }
    history.add_notice("The displayed transcript uses Grok's retained model history; full native logs and compaction checkpoints remain in the independent copy.");
    history.check_budget()?;
    if history.messages.is_empty() {
        return Err(error("Grok did not contain any saved messages."));
    }
    Ok(history)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Harness;
    use serde_json::json;
    fn fixture(root: &Path) -> SavedSession {
        // Match storage_root's identity, including Windows' verbatim prefix.
        let canonical_root = root.canonicalize().unwrap();
        let root = canonical_root.as_path();
        let id = uuid::Uuid::new_v4().to_string();
        let path = root
            .join("sessions")
            .join(
                root.to_string_lossy()
                    .bytes()
                    .map(|b| {
                        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                            (b as char).to_string()
                        } else {
                            format!("%{b:02X}")
                        }
                    })
                    .collect::<String>(),
            )
            .join(&id);
        fs::create_dir_all(path.join("compaction_checkpoints")).unwrap();
        let summary = json!({"info":{"id":id,"cwd":root},"session_summary":"Grok fixture","generated_title":"Grok fixture","grok_home":root,"agent_name":"grok-build-plan","sandbox_profile":"off","reasoning_effort":"high","next_trace_turn":1,"created_at":"2026-10-17T10:00:00Z","updated_at":"2026-10-17T10:00:00Z","current_model_id":"grok-4.6","chat_format_version":1,"num_messages":3,"num_chat_messages":3});
        fs::write(
            path.join("summary.json"),
            serde_json::to_vec(&summary).unwrap(),
        )
        .unwrap();
        fs::write(path.join("chat_history.jsonl"),"{\"type\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"Remember teal\"}],\"prompt_index\":0}\n{\"type\":\"assistant\",\"content\":\"Native Grok answer\",\"tool_calls\":[],\"model_id\":\"grok-4.6\"}\n").unwrap();
        let updates = [
            json!({"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"Remember teal"}}),
            json!({"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"Native reasoning"}}),
            json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Native Grok answer"}}),
        ];
        fs::write(
            path.join("updates.jsonl"),
            updates
                .iter()
                .map(|v| serde_json::to_string(&json!({"sessionId":id,"update":v})).unwrap() + "\n")
                .collect::<String>(),
        )
        .unwrap();
        fs::write(
            path.join("compaction_checkpoints/opaque.json"),
            b"{\"signature\":\"retained verbatim\"}",
        )
        .unwrap();
        metadata(root, &path, &summary, &store_id(HarnessId::Grok, root)).unwrap()
    }
    #[tokio::test]
    async fn copies_full_native_state_and_rejects_open_sessions_and_symlinks() {
        let home = tempfile::tempdir().unwrap();
        let source = fixture(home.path());
        let harness = AcpHarness::grok().with_saved_session_home(home.path());
        let page = harness.saved_sessions(None).await.unwrap();
        assert_eq!(page.sessions.len(), 1);
        assert_eq!(page.sessions[0].store_id, source.store_id);
        assert_eq!(page.sessions[0].locator, source.locator);
        assert!(!page.sessions[0].running);
        fs::write(
            home.path().join("active_sessions.json"),
            serde_json::to_vec(&json!([{"session_id":source.native_id,"pid":std::process::id()}]))
                .unwrap(),
        )
        .unwrap();
        assert!(harness.saved_sessions(None).await.unwrap().sessions[0].running);
        assert!(harness.copy_saved_session(&source).await.is_err());
        fs::write(home.path().join("active_sessions.json"), "[]").unwrap();
        let path = source.locator.as_ref().unwrap();
        let mut before = vec![];
        snapshot(path, path, &mut before, &mut 0, 0).unwrap();
        let copy = harness.copy_saved_session(&source).await.unwrap();
        assert_ne!(copy.native_id, source.native_id);
        let history = harness.saved_session_history(&copy).await.unwrap();
        assert_eq!(history.messages.len(), 3);
        assert!(
            matches!(&history.messages[1].parts[0],SavedHistoryPart::Reasoning(s)if s=="Native reasoning")
        );
        let mut after = vec![];
        snapshot(path, path, &mut after, &mut 0, 0).unwrap();
        assert_eq!(before, after);
        let copy_path = copy.locator.unwrap();
        assert_eq!(
            fs::read(copy_path.join("compaction_checkpoints/opaque.json")).unwrap(),
            b"{\"signature\":\"retained verbatim\"}"
        );
        fs::remove_file(copy_path.join("updates.jsonl")).unwrap();
        let fallback =
            chat_history(&records(&copy_path.join("chat_history.jsonl")).unwrap()).unwrap();
        assert!(fallback.messages.iter().any(|m| matches!(
            &m.parts[0], SavedHistoryPart::Text(s) if s == "Remember teal"
        )));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(home.path(), path.join("linked")).unwrap();
            assert!(harness.copy_saved_session(&source).await.is_err());
        }
    }
    #[tokio::test]
    #[ignore = "requires ZERON_GROK_TEST_BINARY (Grok 1.0.4)"]
    async fn native_grok_exports_and_restores_the_independent_copy() {
        let home = tempfile::tempdir().unwrap();
        let source = fixture(home.path());
        let source_path = source.locator.as_ref().unwrap();
        let mut before = vec![];
        snapshot(source_path, source_path, &mut before, &mut 0, 0).unwrap();
        let harness = AcpHarness::grok().with_saved_session_home(home.path());
        let copy = harness.copy_saved_session(&source).await.unwrap();
        let binary = PathBuf::from(std::env::var_os("ZERON_GROK_TEST_BINARY").unwrap());
        let mut cmd = crate::process::Command::new(&binary);
        cmd.args(["--no-auto-update", "agent", "--no-leader", "stdio"])
            .env("GROK_HOME", home.path())
            .env("XAI_API_KEY", "fixture-not-a-real-key")
            .env("XAI_API_BASE_URL", "http://127.0.0.1:9/v1")
            .current_dir(home.path())
            .stdin(crate::process::Stdio::piped())
            .stdout(crate::process::Stdio::piped())
            .stderr(crate::process::Stdio::null())
            .kill_on_drop(true);
        crate::process::owned::configure(&mut cmd);
        let mut child = crate::process::owned::Child::new(cmd.spawn().unwrap());
        let (client, mut incoming) = crate::jsonrpc::RpcClient::bounded(
            child.stdin.take().unwrap(),
            child.stdout.take().unwrap(),
            MAX_RECORD_BYTES,
        );
        let result = tokio::time::timeout(std::time::Duration::from_secs(60), async {
            super::super::saved::collect(
                &client,
                &mut incoming,
                "initialize",
                super::super::initialize_params(HarnessId::Grok),
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
        child.shutdown(std::time::Duration::from_millis(100)).await;
        let output = tokio::process::Command::new(&binary)
            .args(["--no-auto-update", "export", &copy.native_id])
            .env("GROK_HOME", home.path())
            .current_dir(home.path())
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("Native Grok answer"));
        // Grok 1.0.4 restores the model context without replaying transcript
        // chunks on session/load. Export above validates the native transcript;
        // the load response confirms restoration under the independent ID.
        let (loaded, _) = result.unwrap().unwrap();
        assert_eq!(loaded["_meta"]["sessionId"], copy.native_id);
        assert_eq!(loaded["_meta"]["x.ai/sessionDetail"]["cwd"], copy.cwd);
        let mut after = vec![];
        snapshot(source_path, source_path, &mut after, &mut 0, 0).unwrap();
        assert_eq!(before, after, "native restore changed the source");
    }
}
