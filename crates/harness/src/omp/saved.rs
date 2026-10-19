//! OMP's native v3 tree logs. Unlike Pi, an OMP file opens with a fixed-width
//! `title` slot before the `session` header, `model_change` carries a
//! `provider/id` string, and extra entry types (`title_change`, `custom`,
//! `custom_message`, `mode_change`, `session_init`, ...) share the tree. Copies
//! retain every native entry and signature; the visible transcript follows the
//! selected leaf rather than abandoned branches.
use super::{OmpHarness, sessions};
use crate::{HarnessError, saved_sessions::*};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
};
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
fn is_entry(record: &Value) -> bool {
    record["id"].is_string() && !matches!(record["type"].as_str(), Some("session" | "title"))
}
/// The `session` header is the record after OMP's optional first-line `title`
/// slot; never assume index 0.
fn session_index(records: &[Value]) -> Option<usize> {
    records.iter().position(|r| r["type"] == "session")
}

impl OmpHarness {
    fn saved_root(&self) -> Result<PathBuf, HarnessError> {
        Ok(sessions::agent_dir(self.agent_dir.clone()).canonicalize()?)
    }
    pub(super) fn saved_store_id(&self) -> Result<String, HarnessError> {
        Ok(store_id(HarnessId::Omp, &self.saved_root()?))
    }
    fn saved_roots(&self) -> Result<Vec<PathBuf>, HarnessError> {
        let agent = self.saved_root()?;
        let mut roots = vec![agent.join("sessions")];
        if let Some(dir) = std::env::var_os("PI_CODING_AGENT_SESSION_DIR") {
            let path = PathBuf::from(dir);
            if path.is_absolute() {
                roots.push(path);
            }
        }
        Ok(roots)
    }
    pub(super) async fn list_saved(
        &self,
        cursor: Option<&str>,
    ) -> Result<SavedSessionPage, HarnessError> {
        let roots = match self.saved_roots() {
            Ok(roots) => roots,
            Err(HarnessError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SavedSessionPage::default());
            }
            Err(e) => return Err(e),
        };
        let identity = self.saved_store_id()?;
        let offset = cursor
            .map(str::parse::<usize>)
            .transpose()
            .map_err(|_| error("Invalid OMP history cursor."))?
            .unwrap_or(0);
        tokio::task::spawn_blocking(move || {
            let files = inventory(&roots)?;
            if offset > files.len() {
                return Err(error("OMP history changed. Scan again."));
            }
            let end = (offset + 50).min(files.len());
            let mut sessions = vec![];
            for path in &files[offset..end] {
                let records = read_records(path)?;
                if let Some(session) = metadata(&records, path, &identity)? {
                    sessions.push(session);
                }
            }
            Ok(SavedSessionPage {
                sessions,
                next_cursor: (end < files.len()).then(|| end.to_string()),
            })
        })
        .await
        .map_err(|e| error(e.to_string()))?
    }
    fn locate_saved(&self, session: &SavedSession) -> Result<PathBuf, HarnessError> {
        if session.harness != HarnessId::Omp || session.store_id != self.saved_store_id()? {
            return Err(error("The OMP history location changed. Scan again."));
        }
        let path = session
            .locator
            .as_ref()
            .ok_or_else(|| error("OMP session has no native file."))?;
        let canonical = path.canonicalize()?;
        if canonical != *path
            || !self
                .saved_roots()?
                .iter()
                .filter_map(|r| r.canonicalize().ok())
                .any(|r| canonical.starts_with(r))
        {
            return Err(error("The OMP transcript is outside its history store."));
        }
        Ok(canonical)
    }
    pub(super) async fn read_saved(
        &self,
        session: &SavedSession,
    ) -> Result<SavedSessionHistory, HarnessError> {
        let path = self.locate_saved(session)?;
        let session = session.clone();
        tokio::task::spawn_blocking(move || {
            let records = read_records(&path)?;
            check_identity(&records, &session)?;
            history(&records)
        })
        .await
        .map_err(|e| error(e.to_string()))?
    }
    pub(super) async fn copy_saved(
        &self,
        session: &SavedSession,
    ) -> Result<SavedSession, HarnessError> {
        let path = self.locate_saved(session)?;
        let session = session.clone();
        let index = sessions::Store::new(self.session_store.clone(), self.agent_dir.clone());
        tokio::task::spawn_blocking(move || {
            let mut records = read_records(&path)?;
            check_identity(&records, &session)?;
            if busy(&active_branch(&records)?) {
                return Err(running());
            }
            let transcript = history(&records)?;
            if transcript.messages.is_empty() {
                return Err(error("The OMP session has no conversation to copy."));
            }
            transcript.check_budget()?;
            let header = session_index(&records)
                .ok_or_else(|| error("OMP session is missing its header."))?;
            let id = uuid::Uuid::new_v4().to_string();
            records[header]["id"] = json!(id);
            records[header]["parentSession"] = json!(path);
            records[header]["timestamp"] = json!(chrono::Utc::now().to_rfc3339());
            let destination = path.with_file_name(format!(
                "{}_{}.jsonl",
                chrono::Utc::now().format("%Y-%m-%dT%H-%M-%S"),
                id
            ));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&destination)?;
            let result = (|| {
                for record in records {
                    serde_json::to_writer(&mut file, &record).map_err(|e| error(e.to_string()))?;
                    file.write_all(b"\n")?;
                }
                file.sync_all()?;
                #[cfg(unix)]
                File::open(destination.parent().unwrap())?.sync_all()?;
                index.remember(&id, &destination)?;
                Ok::<_, HarnessError>(())
            })();
            if let Err(cause) = result {
                let _ = std::fs::remove_file(&destination);
                return Err(cause);
            }
            Ok(SavedSession {
                native_id: id,
                locator: Some(destination),
                running: false,
                ..session
            })
        })
        .await
        .map_err(|e| error(e.to_string()))?
    }
}
fn inventory(roots: &[PathBuf]) -> Result<Vec<PathBuf>, HarnessError> {
    let mut files = vec![];
    let mut seen = HashSet::new();
    let mut count = 0;
    let mut dirs: Vec<_> = roots.iter().cloned().map(|r| (r, 0)).collect();
    while let Some((dir, depth)) = dirs.pop() {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let entry = entry?;
            count += 1;
            if count > 20000 {
                return Err(error("OMP discovery exceeded 20000 entries."));
            }
            let kind = entry.file_type()?;
            if kind.is_dir() && depth < 2 {
                dirs.push((entry.path(), depth + 1));
            } else if kind.is_file() && entry.path().extension().is_some_and(|e| e == "jsonl") {
                let path = entry.path().canonicalize()?;
                if seen.insert(path.clone()) {
                    files.push(path);
                }
            }
        }
    }
    files.sort_by_cached_key(|p| {
        (
            std::cmp::Reverse(p.metadata().and_then(|m| m.modified()).ok()),
            p.clone(),
        )
    });
    Ok(files)
}
fn read_records(path: &Path) -> Result<Vec<Value>, HarnessError> {
    let file = File::open(path)?;
    let before = file.metadata()?;
    if before.len() > MAX_SOURCE_BYTES {
        return Err(error("OMP transcript exceeds the native import limit."));
    }
    let mut reader = BufReader::new(file.take(MAX_SOURCE_BYTES + 1));
    let mut records = vec![];
    loop {
        let mut line = Vec::new();
        let n = reader
            .by_ref()
            .take((MAX_RECORD_BYTES + 1) as u64)
            .read_until(b'\n', &mut line)?;
        if n == 0 {
            break;
        }
        if n > MAX_RECORD_BYTES {
            return Err(error("OMP history record exceeds the import limit."));
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        records.push(
            serde_json::from_slice(&line)
                .map_err(|e| error(format!("OMP history is incomplete or invalid: {e}")))?,
        );
    }
    let after = path.metadata()?;
    if before.len() != after.len() || before.modified()? != after.modified()? {
        return Err(error("OMP history changed while reading. Scan again."));
    }
    Ok(records)
}
fn check_identity(records: &[Value], session: &SavedSession) -> Result<(), HarnessError> {
    if !sessions::omp_marker(records) {
        return Err(error(
            "The transcript has no OMP session markers; it belongs to another agent.",
        ));
    }
    let header = session_index(records)
        .map(|index| &records[index])
        .ok_or_else(|| error("OMP session is empty."))?;
    if header["id"].as_str() != Some(&session.native_id)
        || header["cwd"].as_str() != Some(&session.cwd)
    {
        return Err(error(
            "OMP session identity or working directory changed. Scan again.",
        ));
    }
    if !matches!(header["version"].as_u64(), None | Some(1..=3)) {
        return Err(error("OMP import requires session format v1, v2 or v3."));
    }
    Ok(())
}
fn active_branch(records: &[Value]) -> Result<Vec<&Value>, HarnessError> {
    let mut by_id = HashMap::new();
    for r in records.iter().filter(|r| is_entry(r)) {
        let id = r["id"]
            .as_str()
            .ok_or_else(|| error("OMP history entry has no identity."))?;
        if by_id.insert(id, r).is_some() {
            return Err(error("OMP history contains duplicate entry IDs."));
        }
    }
    let mut result = vec![];
    let mut seen = HashSet::new();
    let mut id = records
        .iter()
        .rev()
        .find(|r| is_entry(r))
        .and_then(|r| r["id"].as_str());
    while let Some(current) = id {
        if !seen.insert(current) {
            return Err(error("OMP history contains a parent cycle."));
        }
        let r = by_id
            .get(current)
            .ok_or_else(|| error("OMP history is missing a parent entry."))?;
        result.push(*r);
        id = match &r["parentId"] {
            Value::Null => None,
            Value::String(s) => Some(s.as_str()),
            _ => return Err(error("OMP entry has an invalid parent.")),
        };
    }
    result.reverse();
    Ok(result)
}
fn busy(branch: &[&Value]) -> bool {
    let mut pending = HashSet::new();
    let mut waiting = false;
    for r in branch {
        let m = &r["message"];
        match m["role"].as_str() {
            Some("user") => waiting = true,
            Some("assistant") => {
                waiting = matches!(
                    m["stopReason"].as_str(),
                    None | Some("toolUse" | "tool_calls")
                );
                if let Some(parts) = m["content"].as_array() {
                    for p in parts {
                        if p["type"] == "toolCall"
                            && let Some(id) = p["id"].as_str()
                        {
                            pending.insert(id);
                        }
                    }
                }
            }
            Some("toolResult") => {
                if let Some(id) = m["toolCallId"].as_str() {
                    pending.remove(id);
                }
            }
            _ => {}
        }
    }
    waiting || !pending.is_empty()
}
fn metadata(
    records: &[Value],
    path: &Path,
    identity: &str,
) -> Result<Option<SavedSession>, HarnessError> {
    let Some(header_index) = session_index(records) else {
        return Ok(None);
    };
    // Pi and OMP can be pointed at one native directory; only OMP-marked files
    // are OMP history, so a Pi file is never listed under the OMP importer.
    if !sessions::omp_marker(records) {
        return Ok(None);
    }
    let header = &records[header_index];
    let mut session = SavedSession {
        harness: HarnessId::Omp,
        native_id: header["id"].as_str().unwrap_or_default().into(),
        store_id: identity.into(),
        title: "Untitled session".into(),
        cwd: header["cwd"].as_str().unwrap_or_default().into(),
        created_at_ms: timestamp(&header["timestamp"]),
        updated_at_ms: timestamp(&header["timestamp"]),
        model: None,
        locator: Some(path.to_path_buf()),
        running: false,
    };
    check_identity(records, &session)?;
    // The mutable title lives in a fixed-width first-line slot; `session.title`
    // and `title_change` entries are the fallbacks.
    let mut title = records
        .iter()
        .find(|r| r["type"] == "title")
        .and_then(|r| r["title"].as_str())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| {
            header["title"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .map(str::to_owned)
        });
    let branch = active_branch(records)?;
    let mut titled = title.is_some();
    for r in &branch {
        session.updated_at_ms = session.updated_at_ms.max(timestamp(&r["timestamp"]));
        if r["type"] == "title_change" {
            if let Some(name) = r["title"].as_str().filter(|s| !s.trim().is_empty()) {
                title = Some(name.into());
                titled = true;
            }
        }
        let m = &r["message"];
        if !titled && m["role"] == "user" {
            let text = text(&m["content"]);
            if !text.trim().is_empty() {
                title = Some(text.chars().take(160).collect());
                titled = true;
            }
        }
        if r["type"] == "model_change" {
            session.model = r["model"].as_str().map(str::to_owned);
        }
        if m["role"] == "assistant"
            && let (Some(p), Some(id)) = (m["provider"].as_str(), m["model"].as_str())
        {
            session.model = Some(format!("{p}/{id}"));
        }
    }
    if let Some(title) = title {
        session.title = title;
    }
    session.running = busy(&branch);
    Ok(Some(session))
}
fn text(v: &Value) -> String {
    v.as_str().map(str::to_owned).unwrap_or_else(|| {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|p| p["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    })
}
fn history(records: &[Value]) -> Result<SavedSessionHistory, HarnessError> {
    let branch = active_branch(records)?;
    let mut history = SavedSessionHistory::default();
    let mut bytes = 0;
    let mut tools: HashMap<String, (usize, usize)> = HashMap::new();
    for r in branch {
        let m = &r["message"];
        if r["type"] == "custom_message" {
            let content = text(&m["content"]);
            if r["display"] != false && !content.trim().is_empty() {
                history.add_notice("Extension messages are imported as system notes; their native records remain in the copy.");
                history.push_checked(
                    SavedHistoryMessage {
                        id: r["id"].as_str().unwrap_or_default().into(),
                        role: SavedMessageRole::System,
                        timestamp_ms: timestamp(&r["timestamp"]),
                        parts: vec![SavedHistoryPart::Text(content)],
                    },
                    &mut bytes,
                )?;
            } else {
                history.add_notice("Hidden extension messages remain in OMP's native copy and are omitted from the imported transcript.");
            }
            continue;
        }
        if m["role"] == "toolResult" {
            if let Some(&(mi, pi)) = m["toolCallId"].as_str().and_then(|id| tools.get(id)) {
                if let SavedHistoryPart::Tool {
                    output, is_error, ..
                } = &mut history.messages[mi].parts[pi]
                {
                    *output = Some(text(&m["content"]));
                    *is_error = m["isError"] == true;
                }
            }
            if m["content"]
                .as_array()
                .is_some_and(|parts| parts.iter().any(|p| p["type"] != "text"))
            {
                history.add_notice("Attachments in tool results remain in OMP's native copy and are omitted from the imported transcript.");
            }
            continue;
        }
        let role = match m["role"].as_str() {
            Some("user") => SavedMessageRole::User,
            Some("assistant") => SavedMessageRole::Assistant,
            // OMP's developer-role messages seed the model context; they are
            // imported as system notes rather than dropped silently.
            Some("developer") => SavedMessageRole::System,
            _ => {
                if m["role"].is_string() {
                    history.add_notice("Custom message types remain in OMP's native copy and are omitted from the imported transcript.");
                }
                if matches!(r["type"].as_str(), Some("compaction" | "branch_summary")) {
                    history.add_notice("OMP compacted or branched this conversation; native context records remain in the copy.");
                    if let Some(summary) = r["summary"].as_str() {
                        history.push_checked(
                            SavedHistoryMessage {
                                id: r["id"].as_str().unwrap_or_default().into(),
                                role: SavedMessageRole::System,
                                timestamp_ms: timestamp(&r["timestamp"]),
                                parts: vec![SavedHistoryPart::Text(summary.into())],
                            },
                            &mut bytes,
                        )?;
                    }
                }
                continue;
            }
        };
        let mut parts = vec![];
        if let Some(s) = m["content"].as_str() {
            parts.push(SavedHistoryPart::Text(s.into()));
        } else if let Some(content) = m["content"].as_array() {
            for part in content {
                match part["type"].as_str() {
                Some("text") => parts.push(SavedHistoryPart::Text(part["text"].as_str().unwrap_or_default().into())),
                Some("thinking") => parts.push(SavedHistoryPart::Reasoning(part["thinking"].as_str().unwrap_or_default().into())),
                Some("toolCall") => { let id = part["id"].as_str().ok_or_else(|| error("OMP tool call has no ID."))?; tools.insert(id.to_owned(), (history.messages.len(), parts.len())); parts.push(SavedHistoryPart::Tool { call: super::normalize::tool(part["name"].as_str().unwrap_or_default(), &part["arguments"]), output: None, is_error: false }); }
                _ => history.add_notice("Attachments and unsupported blocks remain in OMP's native copy and are omitted from the imported transcript."),
            }
            }
        }
        if !parts.is_empty() {
            history.push_checked(
                SavedHistoryMessage {
                    id: r["id"].as_str().unwrap_or_default().into(),
                    role,
                    timestamp_ms: timestamp(&r["timestamp"]),
                    parts,
                },
                &mut bytes,
            )?;
        }
    }
    history.check_budget()?;
    Ok(history)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Harness;

    fn fixture() -> (tempfile::TempDir, OmpHarness, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions/project/source.jsonl");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let records = vec![
            json!({"type":"title","v":1,"title":"Teal session","updatedAt":"2026-10-01T00:00:00Z","pad":""}),
            json!({"type":"session","version":3,"id":"11111111-2222-3333-4444-555555555555","cwd":dir.path(),"timestamp":"2026-10-01T00:00:00Z"}),
            json!({"type":"model_change","id":"model","parentId":null,"model":"fixture/test"}),
            json!({"type":"message","id":"u","parentId":"model","message":{"role":"user","content":[{"type":"text","text":"remember teal"}]}}),
            json!({"type":"message","id":"abandoned","parentId":"u","message":{"role":"assistant","content":[{"type":"text","text":"old branch"}],"stopReason":"stop"}}),
            json!({"type":"message","id":"tool","parentId":"u","message":{"role":"assistant","provider":"fixture","model":"test","content":[{"type":"thinking","thinking":"reasoning","thinkingSignature":"signed"},{"type":"toolCall","id":"call","name":"bash","arguments":{"command":"pwd"}}],"stopReason":"toolUse"}}),
            json!({"type":"message","id":"result","parentId":"tool","message":{"role":"toolResult","toolCallId":"call","content":[{"type":"text","text":"/project"}],"isError":false}}),
            json!({"type":"message","id":"a","parentId":"result","message":{"role":"assistant","content":[{"type":"text","text":"current branch","textSignature":"signed-text"}],"stopReason":"stop"}}),
            json!({"type":"custom","id":"extra","parentId":"a","customType":"fixture","data":{"retained":true}}),
        ];
        std::fs::write(
            &path,
            records.iter().map(|r| format!("{r}\n")).collect::<String>(),
        )
        .unwrap();
        let harness = OmpHarness::new().with_agent_dir(dir.path());
        (dir, harness, path)
    }
    #[tokio::test]
    async fn a_provider_with_no_history_has_an_empty_page() {
        let dir = tempfile::tempdir().unwrap();
        let page = OmpHarness::new()
            .with_agent_dir(dir.path().join("not-created"))
            .saved_sessions(None)
            .await
            .unwrap();
        assert!(page.sessions.is_empty() && page.next_cursor.is_none());
    }
    #[tokio::test]
    async fn pi_files_in_a_shared_agent_dir_are_not_omp_history() {
        let dir = tempfile::tempdir().unwrap();
        let sessions = dir.path().join("sessions/project");
        std::fs::create_dir_all(&sessions).unwrap();
        let omp = [
            json!({"type":"title","v":1,"title":"OMP one","updatedAt":"2026-10-01T00:00:00Z","pad":""}),
            json!({"type":"session","version":3,"id":"omp-id","cwd":dir.path(),"timestamp":"2026-10-01T00:00:00Z"}),
            json!({"type":"message","id":"u","parentId":null,"message":{"role":"user","content":[{"type":"text","text":"hello omp"}]}}),
        ];
        std::fs::write(
            sessions.join("omp.jsonl"),
            omp.iter().map(|r| format!("{r}\n")).collect::<String>(),
        )
        .unwrap();
        let pi = [
            json!({"type":"session","version":3,"id":"pi-id","cwd":dir.path(),"timestamp":"2026-10-01T00:00:00Z"}),
            json!({"type":"model_change","id":"m","parentId":null,"provider":"p","modelId":"m"}),
            json!({"type":"message","id":"u","parentId":"m","message":{"role":"user","content":[{"type":"text","text":"hello pi"}]}}),
        ];
        std::fs::write(
            sessions.join("pi.jsonl"),
            pi.iter().map(|r| format!("{r}\n")).collect::<String>(),
        )
        .unwrap();
        let page = OmpHarness::new()
            .with_agent_dir(dir.path())
            .saved_sessions(None)
            .await
            .unwrap();
        assert_eq!(
            page.sessions
                .iter()
                .map(|s| s.native_id.as_str())
                .collect::<Vec<_>>(),
            ["omp-id"]
        );
    }
    #[tokio::test]
    async fn native_copy_retains_tree_signatures_and_resolves_for_resume_without_changing_source() {
        let (dir, harness, path) = fixture();
        let original = std::fs::read(&path).unwrap();
        let source = harness
            .saved_sessions(None)
            .await
            .unwrap()
            .sessions
            .remove(0);
        assert!(!source.running);
        assert_eq!(source.title, "Teal session");
        assert_eq!(source.model.as_deref(), Some("fixture/test"));
        let history = harness.saved_session_history(&source).await.unwrap();
        assert!(!format!("{history:?}").contains("old branch"));
        assert_eq!(history.messages.len(), 3);
        assert!(
            matches!(&history.messages[1].parts[1], SavedHistoryPart::Tool { output:Some(s),..} if s == "/project")
        );
        let copy = harness.copy_saved_session(&source).await.unwrap();
        assert_ne!(copy.native_id, source.native_id);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let records = read_records(copy.locator.as_ref().unwrap()).unwrap();
        // The title slot keeps the copy identifiable and the header points home.
        assert_eq!(records[0]["type"], "title");
        let header = session_index(&records).unwrap();
        assert_eq!(
            records[header]["parentSession"],
            json!(path.canonicalize().unwrap())
        );
        assert_eq!(
            records[5]["message"]["content"][0]["thinkingSignature"],
            "signed"
        );
        assert_eq!(records.last().unwrap()["data"]["retained"], true);
        assert!(records.iter().any(|r| r["id"] == "abandoned"));
        let store = sessions::Store::new(None, Some(dir.path().into()));
        assert_eq!(
            store.resolve(&copy.native_id, dir.path()).unwrap(),
            *copy.locator.as_ref().unwrap()
        );
        assert_eq!(
            harness
                .saved_session_history(&copy)
                .await
                .unwrap()
                .messages
                .len(),
            3
        );
    }
    #[tokio::test]
    async fn busy_changed_identity_and_incomplete_sources_are_rejected() {
        let (_dir, harness, path) = fixture();
        let source = harness
            .saved_sessions(None)
            .await
            .unwrap()
            .sessions
            .remove(0);
        let mut forged = source.clone();
        forged.native_id = "different".into();
        assert!(harness.copy_saved_session(&forged).await.is_err());
        OpenOptions::new().append(true).open(&path).unwrap().write_all(format!("{}\n",json!({"type":"message","id":"pending","parentId":"extra","message":{"role":"user","content":"pending"}})).as_bytes()).unwrap();
        assert!(harness.saved_sessions(None).await.unwrap().sessions[0].running);
        assert!(harness.copy_saved_session(&source).await.is_err());
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
        std::fs::write(&path, b"{\"type\":").unwrap();
        assert!(harness.saved_session_history(&source).await.is_err());
    }
    #[test]
    fn broken_parent_chains_and_cycles_are_never_silently_flattened() {
        let header = json!({"type":"session"});
        for rows in [
            vec![header.clone(), json!({"id":"x","parentId":"missing"})],
            vec![header.clone(), json!({"id":"x","parentId":"x"})],
            vec![header, json!({"id":"x"}), json!({"id":"x"})],
        ] {
            assert!(active_branch(&rows).is_err());
        }
    }
    #[test]
    fn omp_and_pi_share_no_store_identity() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_ne!(
            store_id(HarnessId::Omp, root),
            store_id(HarnessId::Pi, root)
        );
    }
}
