//! Provider-owned discovery, full transcript export, and native forks on both
//! OpenCode HTTP generations. Never write directly to OpenCode's database.
use super::{OpencodeHarness, Protocol, Server, unwrap_data};
use crate::{HarnessError, saved_sessions::*};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use zeron_proto::HarnessId;

fn error(message: impl Into<String>) -> HarnessError {
    HarnessError::Protocol(message.into())
}
fn timestamp(v: &Value) -> i64 {
    v.as_i64()
        .or_else(|| {
            v.as_str()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|t| t.timestamp_millis())
        })
        .unwrap_or(0)
}
fn query(path: &str, params: &[(&str, &str)]) -> String {
    let mut u = reqwest::Url::parse("http://localhost").unwrap();
    u.set_path(path);
    u.query_pairs_mut().extend_pairs(params.iter().copied());
    format!("{}?{}", u.path(), u.query().unwrap_or_default())
}
fn id(value: &str) -> Result<&str, HarnessError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(error("Invalid OpenCode native session ID."));
    }
    Ok(value)
}
fn cwd(info: &Value) -> Option<String> {
    info["directory"].as_str().map(str::to_owned).or_else(|| {
        info.pointer("/location/directory")
            .and_then(Value::as_str)
            .map(|directory| {
                info["subpath"].as_str().map_or_else(
                    || directory.to_owned(),
                    |subpath| {
                        Path::new(directory)
                            .join(subpath)
                            .to_string_lossy()
                            .into_owned()
                    },
                )
            })
    })
}
impl OpencodeHarness {
    fn saved_root(&self) -> Result<PathBuf, HarnessError> {
        let path = self.saved_home.clone().unwrap_or_else(|| {
            std::env::var_os("XDG_DATA_HOME")
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| crate::executable::home_or_current_dir().join(".local/share"))
                .join("opencode")
        });
        Ok(path.canonicalize()?)
    }
    pub(super) fn saved_store_id(&self) -> Result<String, HarnessError> {
        Ok(store_id(HarnessId::Opencode, &self.saved_root()?))
    }
    pub(super) async fn list_saved(
        &self,
        cursor: Option<&str>,
    ) -> Result<SavedSessionPage, HarnessError> {
        let identity = match self.saved_store_id() {
            Ok(identity) => identity,
            Err(HarnessError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SavedSessionPage::default());
            }
            Err(e) => return Err(e),
        };
        let _guard = self.probe_lock.lock().await;
        let mut server = self.server(None, None).await?;
        let result = async {
            let protocol = server.protocol().await;
            let path = match protocol {
                Protocol::V1 => "/session?roots=true&limit=5001".into(),
                Protocol::V2 => {
                    let mut pairs = vec![("limit", "50")];
                    if let Some(cursor) = cursor {
                        pairs.push(("cursor", cursor));
                    }
                    query("/api/session", &pairs)
                }
            };
            let response = get(&server, &path, None).await?;
            let next = response
                .pointer("/cursor/next")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let data = unwrap_data(response);
            let mut rows = data
                .as_array()
                .ok_or_else(|| error("OpenCode returned an invalid session list."))?
                .clone();
            let mut next_cursor = next;
            if protocol == Protocol::V1 {
                if rows.len() > 5000 {
                    return Err(error(
                        "Too many OpenCode sessions. Narrow the source store before importing.",
                    ));
                }
                rows.sort_by_key(|r| {
                    (
                        std::cmp::Reverse(timestamp(&r["time"]["updated"])),
                        r["id"].as_str().unwrap_or_default().to_owned(),
                    )
                });
                let offset = cursor
                    .map(str::parse::<usize>)
                    .transpose()
                    .map_err(|_| error("Invalid OpenCode history cursor."))?
                    .unwrap_or(0);
                if offset > rows.len() {
                    return Err(error("OpenCode history changed. Scan again."));
                }
                let end = (offset + 50).min(rows.len());
                next_cursor = (end < rows.len()).then(|| end.to_string());
                rows = rows[offset..end].to_vec();
            }
            let mut sessions = vec![];
            for info in rows {
                // Child agents share the native store, but only user sessions
                // and explicit forks belong in the chooser.
                if info["parentID"].as_str().is_some() && info["fork"].is_null() {
                    continue;
                }
                let Some(mut session) = metadata(&info, &identity)? else {
                    continue;
                };
                let records = messages(&server, &session).await?;
                session.running = server
                    .session_running(&session.native_id, Some(&session.cwd))
                    .await?
                    || unfinished(&records, protocol);
                sessions.push(session);
            }
            Ok(SavedSessionPage {
                sessions,
                next_cursor,
            })
        }
        .await;
        server.shutdown(self.kill_grace).await;
        result
    }
    fn check_saved(&self, session: &SavedSession) -> Result<(), HarnessError> {
        if session.harness != HarnessId::Opencode || session.store_id != self.saved_store_id()? {
            return Err(error("The OpenCode history location changed. Scan again."));
        }
        id(&session.native_id)?;
        Ok(())
    }
    pub(super) async fn read_saved(
        &self,
        session: &SavedSession,
    ) -> Result<SavedSessionHistory, HarnessError> {
        self.check_saved(session)?;
        let _guard = self.probe_lock.lock().await;
        let mut server = self.server(Some(&session.cwd), None).await?;
        let result = async {
            let info = server
                .session_info(&session.native_id, Some(&session.cwd))
                .await?;
            if cwd(&info).as_deref() != Some(&session.cwd) {
                return Err(error(
                    "OpenCode session working directory changed. Scan again.",
                ));
            }
            history(&messages(&server, session).await?, server.protocol().await)
        }
        .await;
        server.shutdown(self.kill_grace).await;
        result
    }
    pub(super) async fn copy_saved(
        &self,
        session: &SavedSession,
    ) -> Result<SavedSession, HarnessError> {
        self.check_saved(session)?;
        let _guard = self.probe_lock.lock().await;
        let mut server = self.server(Some(&session.cwd), None).await?;
        let result = async {
            let info = server
                .session_info(&session.native_id, Some(&session.cwd))
                .await?;
            if cwd(&info).as_deref() != Some(&session.cwd) {
                return Err(error(
                    "OpenCode session working directory changed. Scan again.",
                ));
            }
            let records = messages(&server, session).await?;
            let protocol = server.protocol().await;
            if server
                .session_running(&session.native_id, Some(&session.cwd))
                .await?
                || unfinished(&records, protocol)
            {
                return Err(running());
            }
            let transcript = history(&records, protocol)?;
            if transcript.messages.is_empty() {
                return Err(error("OpenCode session has no conversation to copy."));
            }
            transcript.check_budget()?;
            let prefix = if protocol == Protocol::V2 {
                "/api/session"
            } else {
                "/session"
            };
            let info = unwrap_data(
                server
                    .post_json(
                        &format!("{prefix}/{}/fork", session.native_id),
                        Some(&session.cwd),
                        &json!({}),
                    )
                    .await?,
            );
            let copy = metadata(&info, &session.store_id)?
                .ok_or_else(|| error("OpenCode fork returned no local session."))?;
            if copy.native_id == session.native_id || copy.cwd != session.cwd {
                return Err(error(
                    "OpenCode did not create an independent copy in the original directory.",
                ));
            }
            Ok(copy)
        }
        .await;
        server.shutdown(self.kill_grace).await;
        result
    }
}
async fn get(server: &Server, path: &str, directory: Option<&str>) -> Result<Value, HarnessError> {
    use futures::StreamExt;
    let response = server.get_response(path, directory).await?;
    let mut stream = response.bytes_stream();
    let mut bytes = vec![];
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| error(e.to_string()))?;
        if bytes.len() + chunk.len() > MAX_SOURCE_BYTES as usize {
            return Err(error("OpenCode response exceeds the native import limit."));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes)
        .map_err(|e| error(format!("Invalid OpenCode history response: {e}")))
}
async fn messages(server: &Server, session: &SavedSession) -> Result<Vec<Value>, HarnessError> {
    let path = if server.protocol().await == Protocol::V2 {
        format!(
            "/api/experimental/session/{}/export",
            id(&session.native_id)?
        )
    } else {
        format!("/session/{}/message", id(&session.native_id)?)
    };
    let response = unwrap_data(get(server, &path, Some(&session.cwd)).await?);
    let values = if server.protocol().await == Protocol::V2 {
        &response["messages"]
    } else {
        &response
    };
    let records = values
        .as_array()
        .ok_or_else(|| error("OpenCode did not return the full native transcript."))?;
    if records.len() > MAX_HISTORY_MESSAGES {
        return Err(error("OpenCode transcript exceeds the message limit."));
    }
    Ok(records.clone())
}
fn metadata(info: &Value, identity: &str) -> Result<Option<SavedSession>, HarnessError> {
    let Some(cwd) = cwd(info) else {
        return Ok(None);
    };
    let native_id = info["id"]
        .as_str()
        .ok_or_else(|| error("OpenCode session has no ID."))?;
    id(native_id)?;
    let model = info.get("model").and_then(|m| {
        Some(format!(
            "{}/{}",
            m["providerID"]
                .as_str()
                .or_else(|| m["provider"].as_str())?,
            m["modelID"].as_str().or_else(|| m["id"].as_str())?
        ))
    });
    Ok(Some(SavedSession {
        harness: HarnessId::Opencode,
        native_id: native_id.into(),
        store_id: identity.into(),
        title: info["title"].as_str().unwrap_or("Untitled session").into(),
        cwd,
        created_at_ms: timestamp(&info["time"]["created"]),
        updated_at_ms: timestamp(&info["time"]["updated"]),
        model,
        locator: None,
        running: false,
    }))
}
fn unfinished(records: &[Value], protocol: Protocol) -> bool {
    if protocol == Protocol::V2 {
        // The durable idle marker also covers work owned by another OpenCode
        // process, which a newly booted server's active map cannot observe.
        return records
            .iter()
            .rev()
            .find(|r| {
                matches!(
                    r["type"].as_str(),
                    Some("user" | "assistant" | "idle" | "compaction")
                )
            })
            .is_some_and(|r| r["type"] != "idle");
    }
    records.last().is_some_and(|r| {
        let info = &r["info"];
        info["role"] == "user"
            || (info["role"] == "assistant"
                && info["time"]["completed"].is_null()
                && info["error"].is_null())
            || r["parts"].as_array().is_some_and(|parts| {
                parts.iter().any(|p| {
                    p["type"] == "tool"
                        && !matches!(p["state"]["status"].as_str(), Some("completed" | "error"))
                })
            })
    })
}
fn content_text(value: &Value) -> String {
    value.as_str().map(str::to_owned).unwrap_or_else(|| {
        value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    })
}
fn history(records: &[Value], protocol: Protocol) -> Result<SavedSessionHistory, HarnessError> {
    let mut history = SavedSessionHistory::default();
    let mut bytes = 0;
    for record in records {
        let info = if protocol == Protocol::V2 {
            record
        } else {
            &record["info"]
        };
        let kind = if protocol == Protocol::V2 {
            info["type"].as_str()
        } else {
            info["role"].as_str()
        };
        let role = match kind {
            Some("user") => SavedMessageRole::User,
            Some("assistant") => SavedMessageRole::Assistant,
            Some("compaction" | "system" | "synthetic") => SavedMessageRole::System,
            _ => continue,
        };
        let mut parts = vec![];
        if let Some(text) = info["text"].as_str() {
            parts.push(SavedHistoryPart::Text(text.into()));
        }
        if kind == Some("compaction") {
            if let Some(summary) = info["summary"].as_str() {
                parts.push(SavedHistoryPart::Text(summary.into()));
            }
            history.add_notice("OpenCode compacted this conversation; the provider's native context is retained in the copy.");
        }
        let content = if protocol == Protocol::V2 {
            info["content"].as_array()
        } else {
            record["parts"].as_array()
        };
        for part in content.into_iter().flatten() {
            match part["type"].as_str() {
                Some("text") => parts.push(SavedHistoryPart::Text(part["text"].as_str().unwrap_or_default().into())),
                Some("reasoning") => parts.push(SavedHistoryPart::Reasoning(part["text"].as_str().unwrap_or_default().into())),
                Some("tool") => {
                    let state = &part["state"];
                    let output = state["output"].as_str().map(str::to_owned)
                        .or_else(|| (!state["content"].is_null()).then(|| content_text(&state["content"])))
                        .or_else(|| {
                            (!state["error"].is_null()).then(|| {
                                state["error"].as_str()
                                    .or_else(|| state["error"]["message"].as_str())
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| state["error"].to_string())
                            })
                        });
                    parts.push(SavedHistoryPart::Tool {
                        call: super::oc_tool_call(
                            part["name"].as_str().or_else(|| part["tool"].as_str()).unwrap_or_default(),
                            &state["input"],
                        ),
                        output,
                        is_error: state["status"] == "error",
                    });
                }
                Some("compaction") => history.add_notice("OpenCode compacted this conversation; native context is retained in the copy."),
                Some("step-start"|"step-finish"|"snapshot"|"patch") => {},
                _ => history.add_notice("Attachments and unsupported blocks remain in OpenCode's native copy and are omitted from the imported transcript."),
            }
        }
        if info["files"].as_array().is_some_and(|f| !f.is_empty()) {
            history.add_notice("Attachments remain in OpenCode's native copy and are omitted from the imported transcript.");
        }
        if !parts.is_empty() {
            let id = info["id"]
                .as_str()
                .ok_or_else(|| error("OpenCode message has no ID."))?;
            history.push_checked(
                SavedHistoryMessage {
                    id: id.into(),
                    role,
                    timestamp_ms: timestamp(&info["time"]["created"]),
                    parts,
                },
                &mut bytes,
            )?;
        }
    }
    Ok(history)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Harness;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn wire(
        v2: bool,
    ) -> (
        tempfile::TempDir,
        OpencodeHarness,
        Arc<Mutex<Vec<String>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let root = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let harness = OpencodeHarness::new()
            .with_saved_session_home(root.path())
            .with_base_url(format!("http://{}", listener.local_addr().unwrap()));
        let requests = Arc::new(Mutex::new(vec![]));
        let recorded = requests.clone();
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let recorded = recorded.clone();
                tokio::spawn(async move {
                    let mut bytes = vec![];
                    let mut buf = [0; 4096];
                    loop {
                        let n = socket.read(&mut buf).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        bytes.extend_from_slice(&buf[..n]);
                        if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let header = String::from_utf8_lossy(&bytes);
                    let path = header
                        .lines()
                        .next()
                        .unwrap()
                        .split_whitespace()
                        .nth(1)
                        .unwrap();
                    recorded.lock().unwrap().push(path.into());
                    let native = if path.contains("copy") {
                        "copy"
                    } else if path.contains("page2") {
                        "page2"
                    } else {
                        "original"
                    };
                    let mut info = json!({"id":native,"directory":"/project","location":{"directory":"/project"},"title":"Native fixture","time":{"created":1000,"updated":2000}});
                    if path.ends_with("/fork?directory=%2Fproject")
                        || path.contains("/fork?")
                        || path.ends_with("/fork")
                    {
                        info["id"] = json!("copy");
                    }
                    let v1records = json!([
                        {"info":{"id":"u","role":"user","time":{"created":1000}},"parts":[{"type":"text","text":"remember teal"}]},
                        {"info":{"id":"a","role":"assistant","time":{"created":2000,"completed":2100}},"parts":[{"type":"reasoning","text":"reason"},{"type":"tool","tool":"bash","state":{"status":"completed","input":{"command":"pwd"},"output":"/project"}},{"type":"text","text":"done"}]}
                    ]);
                    let v2records = json!([
                        {"id":"u","type":"user","text":"remember teal","time":{"created":1000}},
                        {"id":"a","type":"assistant","content":[{"type":"reasoning","text":"reason"},{"type":"tool","name":"bash","state":{"status":"completed","input":{"command":"pwd"},"content":[{"type":"text","text":"/project"}]}},{"type":"text","text":"done"}],"time":{"created":2000,"completed":2100}},
                        {"id":"idle","type":"idle","outcome":"succeeded","time":{"created":2200}}
                    ]);
                    let path_only = path.split('?').next().unwrap();
                    let payload = if path_only == "/api/health" && v2 {
                        json!({"version":"2.0.32"})
                    } else if path_only == "/global/health" {
                        json!({"version":"1.18.31"})
                    } else if path_only == "/session/status" || path_only == "/api/session/active" {
                        if v2 { json!({"data":{}}) } else { json!({}) }
                    } else if path_only == "/session" || path_only == "/api/session" {
                        if v2 {
                            if path.contains("cursor=") {
                                json!({"data":[info],"cursor":{}})
                            } else {
                                json!({"data":[info],"cursor":{"next":"page2"}})
                            }
                        } else {
                            json!([info])
                        }
                    } else if path_only.ends_with("/message") {
                        v1records
                    } else if path_only.ends_with("/export") {
                        json!({"data":{"info":info,"messages":v2records}})
                    } else if v2 {
                        json!({"data":info})
                    } else {
                        info
                    };
                    let body = payload.to_string();
                    socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
                });
            }
        });
        (root, harness, requests, server)
    }
    #[tokio::test]
    async fn no_history_does_not_boot_a_native_server() {
        let dir = tempfile::tempdir().unwrap();
        let page = OpencodeHarness::new()
            .with_executable("missing-binary")
            .with_saved_session_home(dir.path().join("not-created"))
            .saved_sessions(None)
            .await
            .unwrap();
        assert!(page.sessions.is_empty() && page.next_cursor.is_none());
    }
    #[tokio::test]
    async fn both_wires_discover_read_tools_and_fork_full_history_with_separate_identity() {
        for v2 in [false, true] {
            let (_root, harness, requests, server) = wire(v2).await;
            let first = harness.saved_sessions(None).await.unwrap();
            let source = &first.sessions[0];
            assert!(!source.running);
            if v2 {
                let second = harness
                    .saved_sessions(first.next_cursor.as_deref())
                    .await
                    .unwrap();
                assert_eq!(second.sessions[0].native_id, "page2");
                assert!(second.next_cursor.is_none());
            }
            let copy = harness.copy_saved_session(source).await.unwrap();
            assert_eq!(copy.native_id, "copy");
            assert_eq!(copy.cwd, source.cwd);
            let history = harness.saved_session_history(&copy).await.unwrap();
            assert_eq!(history.messages.len(), 2);
            assert!(
                matches!(&history.messages[1].parts[1],SavedHistoryPart::Tool {output:Some(s),..} if s=="/project")
            );
            let requests = requests.lock().unwrap();
            assert!(requests.iter().any(|p| p.contains("/original/fork")));
            if v2 {
                assert!(
                    requests
                        .iter()
                        .any(|p| p.contains("/api/experimental/session/copy/export"))
                );
                assert!(!requests.iter().any(|p| p.contains("/copy/message")));
            }
            server.abort();
        }
    }
    #[tokio::test]
    #[ignore = "requires ZERON_OPENCODE_TEST_BINARY pointing to OpenCode 2.x"]
    async fn native_v2_fork_exports_complete_context_without_changing_the_source() {
        use crate::environment::{EnvironmentEntry, EnvironmentSnapshot};
        let binary =
            std::env::var_os("ZERON_OPENCODE_TEST_BINARY").expect("set ZERON_OPENCODE_TEST_BINARY");
        let root = tempfile::tempdir().unwrap();
        let mut environment = EnvironmentSnapshot::default();
        for (key, dir) in [
            ("XDG_DATA_HOME", "data"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
        ] {
            environment.entries.insert(
                key.into(),
                EnvironmentEntry::Set {
                    value: root.path().join(dir).to_string_lossy().into(),
                },
            );
        }
        let harness = OpencodeHarness::new()
            .with_executable(binary)
            .with_environment(Arc::new(environment))
            .with_saved_session_home(root.path().join("data/opencode"));
        let mut server = harness.server(root.path().to_str(), None).await.unwrap();
        assert_eq!(server.protocol().await, Protocol::V2);
        let mut info = unwrap_data(
            server
                .post_json(
                    "/api/session",
                    None,
                    &json!({"title":"Native import fixture","location":{"directory":root.path()}}),
                )
                .await
                .unwrap(),
        );
        info["id"] = json!("ses_native_fixture");
        let records = json!([
            {"id":"msg_fixtureuser","type":"user","text":"remember teal","files":[],"agents":[],"skills":[],"time":{"created":1000}},
            {"id":"msg_fixtureidle","type":"idle","outcome":"succeeded","time":{"created":2000}}
        ]);
        server
            .post_json(
                "/api/experimental/session/import",
                None,
                &json!({"info":info,"messages":records}),
            )
            .await
            .unwrap();
        let before = get(
            &server,
            "/api/experimental/session/ses_native_fixture/export",
            None,
        )
        .await
        .unwrap();
        server.shutdown(harness.kill_grace).await;
        let page = harness.saved_sessions(None).await.unwrap();
        let source = page
            .sessions
            .iter()
            .find(|s| s.native_id == "ses_native_fixture")
            .unwrap();
        assert!(!source.running);
        let copy = harness.copy_saved_session(source).await.unwrap();
        assert_ne!(copy.native_id, source.native_id);
        let history = harness.saved_session_history(&copy).await.unwrap();
        assert!(
            matches!(&history.messages[0].parts[0],SavedHistoryPart::Text(s) if s=="remember teal")
        );
        let mut server = harness.server(root.path().to_str(), None).await.unwrap();
        let after = get(
            &server,
            "/api/experimental/session/ses_native_fixture/export",
            None,
        )
        .await
        .unwrap();
        assert_eq!(before["data"]["messages"], after["data"]["messages"]);
        let exported = get(
            &server,
            &format!("/api/experimental/session/{}/export", copy.native_id),
            None,
        )
        .await
        .unwrap();
        assert_eq!(exported["data"]["messages"].as_array().unwrap().len(), 2);
        assert_eq!(exported["data"]["messages"][1]["type"], "idle");
        server.shutdown(harness.kill_grace).await;
    }
    #[test]
    fn durable_idle_and_pending_tool_state_prevent_copying_an_active_turn() {
        assert!(unfinished(&[json!({"type":"user"})], Protocol::V2));
        assert!(unfinished(
            &[json!({"type":"assistant","time":{"completed":3}})],
            Protocol::V2
        ));
        assert!(!unfinished(&[json!({"type":"idle"})], Protocol::V2));
        assert!(unfinished(
            &[
                json!({"info":{"role":"assistant","time":{"completed":3}},"parts":[{"type":"tool","state":{"status":"running"}}]})
            ],
            Protocol::V1
        ));
        assert!(!unfinished(
            &[json!({"info":{"role":"assistant","time":{"completed":3}},"parts":[]})],
            Protocol::V1
        ));
    }
}
