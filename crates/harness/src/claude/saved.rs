//! Bounded local transcript reading and an offline native fork. The fork uses
//! the Agent SDK's persisted format: new session/message UUIDs, remapped parent
//! links, no progress/team identity, and retained native context records.

use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use zeron_proto::HarnessId;

use super::{ClaudeHarness, normalize::decode_tool_use};
use crate::HarnessError;
use crate::saved_sessions::*;

fn protocol(message: impl Into<String>) -> HarnessError {
    HarnessError::Protocol(message.into())
}
fn uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn timestamp(record: &Value) -> Option<i64> {
    record["timestamp"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.timestamp_millis())
}
fn main_record(record: &Value) -> bool {
    record["isSidechain"] != true
        && record.get("parent_tool_use_id").is_none_or(Value::is_null)
        && matches!(
            record["type"].as_str(),
            Some("user" | "assistant" | "system" | "attachment" | "progress")
        )
        && record["uuid"].as_str().is_some()
}

impl ClaudeHarness {
    pub(super) fn saved_root(&self) -> Result<PathBuf, HarnessError> {
        storage_root(self.saved_home.as_deref(), "CLAUDE_CONFIG_DIR", ".claude")
    }

    pub(super) fn saved_store_id(&self) -> Result<String, HarnessError> {
        Ok(store_id(HarnessId::ClaudeCode, &self.saved_root()?))
    }

    pub(super) async fn list_saved(
        &self,
        cursor: Option<&str>,
    ) -> Result<SavedSessionPage, HarnessError> {
        let root = self.saved_root()?;
        let offset = cursor
            .map(str::parse::<usize>)
            .transpose()
            .map_err(|_| protocol("Invalid Claude history cursor."))?
            .unwrap_or(0);
        tokio::task::spawn_blocking(move || {
            let files = inventory(&root)?;
            if offset > files.len() {
                return Err(protocol("Claude history changed. Scan again."));
            }
            let end = (offset + 50).min(files.len());
            let identity = store_id(HarnessId::ClaudeCode, &root);
            let mut sessions = Vec::new();
            for path in &files[offset..end] {
                if let Some(session) = metadata(path, &identity)? {
                    sessions.push(session);
                }
            }
            Ok(SavedSessionPage {
                sessions,
                next_cursor: (end < files.len()).then(|| end.to_string()),
            })
        })
        .await
        .map_err(|e| protocol(e.to_string()))?
    }

    fn locate(&self, session: &SavedSession) -> Result<PathBuf, HarnessError> {
        let root = self.saved_root()?;
        if session.harness != HarnessId::ClaudeCode
            || session.store_id != store_id(HarnessId::ClaudeCode, &root)
            || uuid::Uuid::parse_str(&session.native_id).is_err()
        {
            return Err(protocol(
                "The Claude history location or session identity changed. Scan again.",
            ));
        }
        let path = session
            .locator
            .as_ref()
            .ok_or_else(|| protocol("The Claude session has no local transcript."))?;
        let canonical = std::fs::canonicalize(path)?;
        if canonical != *path
            || canonical.parent().and_then(Path::parent) != Some(root.join("projects").as_path())
            || canonical.file_name().and_then(|p| p.to_str())
                != Some(format!("{}.jsonl", session.native_id).as_str())
        {
            return Err(protocol(
                "The Claude transcript is outside its original history store.",
            ));
        }
        Ok(canonical)
    }

    pub(super) async fn read_saved(
        &self,
        session: &SavedSession,
    ) -> Result<SavedSessionHistory, HarnessError> {
        let path = self.locate(session)?;
        let session = session.clone();
        tokio::task::spawn_blocking(move || {
            let records = read_records(&path)?;
            check_cwd(&records, &session.cwd)?;
            normalize_history(&records, &session)
        })
        .await
        .map_err(|e| protocol(e.to_string()))?
    }

    pub(super) async fn copy_saved(
        &self,
        session: &SavedSession,
    ) -> Result<SavedSession, HarnessError> {
        let path = self.locate(session)?;
        let session = session.clone();
        tokio::task::spawn_blocking(move || {
            let records = read_records(&path)?;
            check_cwd(&records, &session.cwd)?;
            let boundary = completed_boundary(&records)?;
            // Reject oversized display history before creating any native file.
            normalize_history(&records[..=boundary], &session)?.check_budget()?;
            let new_id = uuid();
            let fork = fork_records(
                &records[..=boundary],
                &session.native_id,
                &new_id,
                &session.title,
            )?;
            let destination = path.with_file_name(format!("{new_id}.jsonl"));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let result = (|| {
                let mut file = options.open(&destination)?;
                for record in fork {
                    serde_json::to_writer(&mut file, &record)
                        .map_err(|e| protocol(e.to_string()))?;
                    file.write_all(b"\n")?;
                }
                file.sync_all()?;
                #[cfg(unix)]
                File::open(destination.parent().unwrap())?.sync_all()?;
                Ok::<_, HarnessError>(())
            })();
            if let Err(error) = result {
                let _ = std::fs::remove_file(&destination);
                return Err(error);
            }
            Ok(SavedSession {
                native_id: new_id,
                locator: Some(destination),
                running: false,
                ..session
            })
        })
        .await
        .map_err(|e| protocol(e.to_string()))?
    }

    /// Validate the file before spawning --resume. Native CLI failures remain
    /// errors; the stream additionally checks the actual resumed session ID.
    pub(super) async fn check_resume(&self, id: &str, cwd: &str) -> Result<(), HarnessError> {
        let root = self.saved_root()?;
        let id = id.to_owned();
        let cwd = cwd.to_owned();
        tokio::task::spawn_blocking(move || {
            if uuid::Uuid::parse_str(&id).is_err() {
                return Err(protocol("The imported Claude copy ID is invalid."));
            }
            let path = inventory(&root)?
                .into_iter()
                .find(|path| path.file_stem().and_then(|s| s.to_str()) == Some(&id))
                .ok_or_else(|| {
                    protocol("The imported Claude copy is missing from the provider history store.")
                })?;
            let session = metadata(&path, "")?
                .ok_or_else(|| protocol("The imported Claude copy contains no conversation."))?;
            if session.cwd != cwd {
                return Err(protocol(
                    "The imported Claude copy's working directory changed.",
                ));
            }
            Ok(())
        })
        .await
        .map_err(|e| protocol(e.to_string()))?
    }
}

fn inventory(root: &Path) -> Result<Vec<PathBuf>, HarnessError> {
    let projects = root.join("projects");
    if !projects.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    let mut visited = 0usize;
    for project in std::fs::read_dir(&projects)? {
        let project = project?;
        visited += 1;
        if visited > 20_000 {
            return Err(protocol(
                "The Claude history inventory exceeds the import limit.",
            ));
        }
        if !project.file_type()?.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(project.path())? {
            let entry = entry?;
            visited += 1;
            if visited > 20_000 {
                return Err(protocol(
                    "The Claude history inventory exceeds the import limit.",
                ));
            }
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|p| p.to_str()) == Some("jsonl")
                && path
                    .file_stem()
                    .and_then(|p| p.to_str())
                    .is_some_and(|p| uuid::Uuid::parse_str(p).is_ok())
            {
                files.push(path);
            }
        }
    }
    // Stable filename order makes page boundaries deterministic. UI sorts the
    // accumulated results by native activity time.
    files.sort();
    Ok(files)
}

fn metadata(path: &Path, identity: &str) -> Result<Option<SavedSession>, HarnessError> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    const SAMPLE: u64 = 256 * 1024;
    let mut samples = Vec::new();
    Read::by_ref(&mut file)
        .take(SAMPLE)
        .read_to_end(&mut samples)?;
    let mut head = samples.len();
    if size > SAMPLE {
        // Do not join a partial head record to the first complete tail record.
        samples.truncate(
            samples
                .iter()
                .rposition(|b| *b == b'\n')
                .map_or(0, |i| i + 1),
        );
        head = samples.len();
        let mut tail = Vec::new();
        file.seek(SeekFrom::Start(size.saturating_sub(SAMPLE)))?;
        file.take(SAMPLE).read_to_end(&mut tail)?;
        if let Some(start) = tail.iter().position(|b| *b == b'\n') {
            samples.extend_from_slice(&tail[start + 1..]);
        }
    }
    let native_id = path
        .file_stem()
        .and_then(|p| p.to_str())
        .unwrap_or_default();
    let mut cwd = None;
    let mut title = None;
    let mut first = None;
    let mut last = None;
    let mut model = None;
    let mut prompt = None;
    let mut main = false;
    // Records from the tail sample (or the whole small file) decide whether
    // the newest turn is still running; head records cannot.
    let mut recent = Vec::new();
    let (first_sample, last_sample) = samples.split_at(head);
    let lines = first_sample
        .split(|b| *b == b'\n')
        .map(|line| (size <= SAMPLE, line))
        .chain(last_sample.split(|b| *b == b'\n').map(|line| (true, line)));
    for (is_recent, line) in lines {
        let Ok(record) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        if record["sessionId"]
            .as_str()
            .is_some_and(|id| id != native_id)
            || record["isSidechain"] == true
        {
            continue;
        }
        if let Some(path) = record["cwd"].as_str() {
            cwd = Some(path.to_owned());
        }
        if record["type"] == "relocated"
            && let Some(path) = record["relocatedCwd"].as_str()
        {
            cwd = Some(path.to_owned());
        }
        if let Some(name) = record["customTitle"]
            .as_str()
            .or_else(|| record["aiTitle"].as_str())
        {
            title = Some(name.to_owned());
        }
        if main_record(&record) {
            main |= matches!(record["type"].as_str(), Some("user" | "assistant"));
            if let Some(time) = timestamp(&record) {
                first = first.or(Some(time));
                last = Some(time);
            }
            if let Some(name) = record["message"]["model"].as_str() {
                model = Some(name.to_owned());
            }
            if prompt.is_none() && record["type"] == "user" {
                prompt = message_text(&record["message"]["content"]);
            }
        }
        if is_recent {
            recent.push(record);
        }
    }
    if !main {
        return Ok(None);
    }
    let fallback = std::fs::metadata(path)?
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    Ok(Some(SavedSession {
        harness: HarnessId::ClaudeCode,
        native_id: native_id.into(),
        store_id: identity.into(),
        title: title
            .or(prompt)
            .unwrap_or_else(|| native_id.into())
            .chars()
            .take(200)
            .collect(),
        cwd: cwd.unwrap_or_default(),
        created_at_ms: first.unwrap_or(fallback),
        updated_at_ms: last.unwrap_or(fallback),
        model,
        locator: Some(path.to_owned()),
        running: turn_finished(&recent) == Some(false),
    }))
}

fn read_records(path: &Path) -> Result<Vec<Value>, HarnessError> {
    let file = File::open(path)?;
    let before = file.metadata()?;
    if before.len() > MAX_SOURCE_BYTES {
        return Err(protocol(
            "The Claude transcript exceeds the import limit; no history was truncated.",
        ));
    }
    let mut reader = BufReader::new(file);
    let mut records = Vec::new();
    let mut total = 0u64;
    loop {
        let mut line = Vec::new();
        let count = reader
            .by_ref()
            .take(MAX_RECORD_BYTES as u64 + 1)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if count > MAX_RECORD_BYTES || total > MAX_SOURCE_BYTES {
            return Err(protocol(
                "The Claude transcript exceeds the import limit; no history was truncated.",
            ));
        }
        if line.last() != Some(&b'\n') {
            return Err(protocol(
                "The Claude transcript has an incomplete record. Finish or stop the source turn and retry.",
            ));
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        records.push(
            serde_json::from_slice(&line).map_err(|e| {
                protocol(format!("The Claude transcript has an invalid record: {e}"))
            })?,
        );
        if records.len() > 100_000 {
            return Err(protocol(
                "The Claude transcript has too many records to import.",
            ));
        }
    }
    let after = std::fs::metadata(path)?;
    if before.len() != after.len() || before.modified()? != after.modified()? {
        return Err(protocol(
            "The Claude session changed while it was read. Finish or stop the source turn and retry.",
        ));
    }
    Ok(records)
}

fn check_cwd(records: &[Value], expected: &str) -> Result<(), HarnessError> {
    let cwd = records.iter().rev().find_map(|r| {
        if r["type"] == "relocated" {
            r["relocatedCwd"].as_str()
        } else if main_record(r) {
            r["cwd"].as_str()
        } else {
            None
        }
    });
    if cwd != Some(expected) {
        return Err(protocol(
            "The Claude session's working directory changed. Scan again.",
        ));
    }
    Ok(())
}

/// Local slash-command and `!` bash records are written as user records but are
/// not conversation turns. Only their printed output proves the CLI was idle; a
/// bare `<command-name>` may be a prompt command whose turn is still running.
fn local_command(record: &Value) -> Option<bool> {
    if record["type"] != "user" {
        return None;
    }
    if record["isMeta"] == true {
        return Some(false);
    }
    let text = message_text(&record["message"]["content"])?;
    let text = text.trim_start();
    if [
        "<local-command-stdout>",
        "<local-command-stderr>",
        "<bash-stdout>",
        "<bash-stderr>",
    ]
    .iter()
    .any(|tag| text.starts_with(tag))
    {
        Some(true)
    } else if text.starts_with("<command-name>") || text.starts_with("<bash-input>") {
        Some(false)
    } else {
        None
    }
}

fn completed_boundary(records: &[Value]) -> Result<usize, HarnessError> {
    match turn_finished(records) {
        None => Err(protocol("The Claude session has no conversation to copy.")),
        Some(false) => Err(running()),
        Some(true) => Ok(records.len() - 1),
    }
}

/// Whether the newest conversation turn reached a native boundary; `None`
/// when the records contain no conversation.
fn turn_finished(records: &[Value]) -> Option<bool> {
    let mut turns = records
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, r)| main_record(r) && matches!(r["type"].as_str(), Some("assistant" | "user")))
        .peekable();
    // Skip a trailing local-command block only when its newest real record is
    // printed output, then judge the conversation record before it.
    let mut compacted = false;
    if turns
        .clone()
        .find(|(_, r)| r["isMeta"] != true)
        .is_some_and(|(_, r)| local_command(r) == Some(true))
    {
        while let Some((_, r)) = turns.next_if(|(_, r)| local_command(r).is_some()) {
            compacted |= message_text(&r["message"]["content"])
                .is_some_and(|s| s.trim_start().starts_with("<command-name>/compact<"));
        }
    }
    let (index, last) = turns.next()?;
    // A manual /compact ends idle on its summary; an automatic one mid-turn
    // leaves no command output behind and stays rejected.
    let compacted = compacted && last["isCompactSummary"] == true;
    let end_turn = last["type"] == "assistant"
        && matches!(
            last["message"]["stop_reason"].as_str(),
            Some("end_turn" | "stop_sequence" | "max_tokens")
        );
    let interrupted = last["type"] == "user"
        && message_text(&last["message"]["content"])
            .is_some_and(|s| s.trim_start().starts_with("[Request interrupted"));
    let duration = records[index + 1..]
        .iter()
        .any(|r| main_record(r) && r["type"] == "system" && r["subtype"] == "turn_duration");
    Some(end_turn || interrupted || duration || compacted)
}

fn message_text(content: &Value) -> Option<String> {
    if let Some(text) = content.as_str() {
        return Some(text.into());
    }
    let text = content
        .as_array()?
        .iter()
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

fn effective_chain(records: &[Value]) -> Result<Vec<usize>, HarnessError> {
    let positions = records
        .iter()
        .enumerate()
        .filter(|(_, r)| main_record(r))
        .map(|(i, r)| (r["uuid"].as_str().unwrap(), i))
        .collect::<HashMap<_, _>>();
    let mut next = records.iter().rposition(|r| {
        main_record(r)
            && (matches!(r["type"].as_str(), Some("user" | "assistant"))
                || r["subtype"] == "compact_boundary")
    });
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    while let Some(index) = next {
        if !seen.insert(index) {
            return Err(protocol("The Claude transcript contains a parent cycle."));
        }
        let record = &records[index];
        chain.push(index);
        next = record["parentUuid"]
            .as_str()
            .and_then(|id| positions.get(id).copied());
    }
    chain.reverse();
    Ok(chain)
}

fn normalize_history(
    records: &[Value],
    session: &SavedSession,
) -> Result<SavedSessionHistory, HarnessError> {
    let mut history = SavedSessionHistory::default();
    let mut budget = 2usize;
    if records
        .iter()
        .any(|r| r["subtype"] == "compact_boundary" || r["isCompactSummary"] == true)
    {
        history.add_notice("This session was compacted. The display history follows its current native conversation chain.");
    }
    let mut results = HashMap::new();
    let chain = effective_chain(records)?;
    for &index in &chain {
        if let Some(blocks) = records[index]["message"]["content"].as_array() {
            for b in blocks {
                if b["type"] == "tool_result"
                    && let Some(id) = b["tool_use_id"].as_str()
                {
                    results.insert(id, b);
                }
            }
        }
    }
    for index in chain {
        let record = &records[index];
        let mut parts = Vec::new();
        let role = match record["type"].as_str() {
            Some("user") if record["isCompactSummary"] == true => SavedMessageRole::System,
            Some("user") => SavedMessageRole::User,
            Some("assistant") => SavedMessageRole::Assistant,
            Some("system") if record["subtype"] == "compact_boundary" => {
                parts.push(SavedHistoryPart::Text(
                    "Native session context was compacted here.".into(),
                ));
                SavedMessageRole::System
            }
            _ => continue,
        };
        let content = &record["message"]["content"];
        if let Some(text) = content.as_str() {
            parts.push(SavedHistoryPart::Text(text.into()));
        }
        if let Some(blocks) = content.as_array() {
            for block in blocks {
                match block["type"].as_str() {
                    Some("text") => { if let Some(text) = block["text"].as_str() { parts.push(SavedHistoryPart::Text(text.into())); } }
                    Some("thinking") => { if let Some(text) = block["thinking"].as_str() { parts.push(SavedHistoryPart::Reasoning(text.into())); } }
                    Some("tool_use") => {
                        let result = block["id"].as_str().and_then(|id| results.get(id));
                        parts.push(SavedHistoryPart::Tool { call: decode_tool_use(block["name"].as_str().unwrap_or("Tool"), &block["input"]),
                            output: result.and_then(|b| message_text(&b["content"])), is_error: result.is_some_and(|b| b["is_error"] == true) });
                    }
                    Some("tool_result") => {}
                    Some("image" | "document") => {
                        parts.push(SavedHistoryPart::Text("[Attachment retained in the native session]".into()));
                        history.add_notice("Historical attachments remain in the native copy; the history view shows placeholders.");
                    }
                    _ => history.add_notice("Some provider-internal content is not displayed; the native copy retains it."),
                }
            }
        }
        if !parts.is_empty() {
            history.push_checked(
                SavedHistoryMessage {
                    id: record["uuid"].as_str().unwrap().into(),
                    role,
                    timestamp_ms: timestamp(record).unwrap_or(session.created_at_ms),
                    parts,
                },
                &mut budget,
            )?;
        }
    }
    Ok(history)
}

fn fork_records(
    records: &[Value],
    source_id: &str,
    new_id: &str,
    title: &str,
) -> Result<Vec<Value>, HarnessError> {
    let main = records
        .iter()
        .filter(|r| main_record(r))
        .collect::<Vec<_>>();
    let map = main
        .iter()
        .map(|r| (r["uuid"].as_str().unwrap(), uuid()))
        .collect::<HashMap<_, _>>();
    let lookup = main
        .iter()
        .map(|r| (r["uuid"].as_str().unwrap(), *r))
        .collect::<HashMap<_, _>>();
    let mut fork = Vec::new();
    if records.iter().any(|r| r["type"] == "history-suppression") {
        fork.push(json!({"type":"history-suppression","sessionId":new_id,"cause":"fork_inherit"}));
    }
    for source in main.iter().filter(|r| r["type"] != "progress") {
        let mut record = (*source).clone();
        let old = source["uuid"].as_str().unwrap();
        record["uuid"] = map[old].clone().into();
        record["sessionId"] = new_id.into();
        record["isSidechain"] = false.into();
        let mut parent = source["parentUuid"].as_str();
        let mut seen = HashSet::new();
        while let Some(id) = parent {
            if !seen.insert(id) {
                return Err(protocol("The Claude transcript contains a parent cycle."));
            }
            let Some(entry) = lookup.get(id) else {
                parent = None;
                break;
            };
            if entry["type"] != "progress" {
                break;
            }
            parent = entry["parentUuid"].as_str();
        }
        record["parentUuid"] = parent
            .and_then(|id| map.get(id))
            .map(|s| Value::String(s.clone()))
            .unwrap_or(Value::Null);
        if let Some(id) = record["logicalParentUuid"].as_str() {
            record["logicalParentUuid"] = map
                .get(id)
                .map(|s| Value::String(s.clone()))
                .unwrap_or(Value::Null);
        }
        if record["type"] == "system" && record["subtype"] == "model_refusal_fallback" {
            record["neutralizedByFork"] = true.into();
        }
        if record["attachment"]["type"] == "deferred_tools_record"
            && let Some(ids) = record["attachment"]["nameOnlyAnnouncements"].as_array()
        {
            record["attachment"]["nameOnlyAnnouncements"] = ids
                .iter()
                .filter_map(|id| id.as_str().and_then(|id| map.get(id)).cloned())
                .collect::<Vec<_>>()
                .into();
        }
        for field in [
            "teamName",
            "agentName",
            "sessionKind",
            "slug",
            "sourceToolAssistantUUID",
        ] {
            record.as_object_mut().unwrap().remove(field);
        }
        record["forkedFrom"] = json!({"sessionId":source_id,"messageUuid":old});
        fork.push(record);
    }
    if let Some(last) = fork.last_mut() {
        last["timestamp"] = chrono::Utc::now().to_rfc3339().into();
    }
    for record in records {
        if record["sessionId"] != source_id {
            continue;
        }
        if matches!(
            record["type"].as_str(),
            Some("content-replacement" | "atis-latch" | "relocated")
        ) {
            let mut copy = record.clone();
            copy["sessionId"] = new_id.into();
            if copy.get("uuid").is_some() {
                copy["uuid"] = uuid().into();
            }
            fork.push(copy);
        }
    }
    fork.push(json!({"type":"custom-title","sessionId":new_id,"customTitle":title,"uuid":uuid(),"timestamp":chrono::Utc::now().to_rfc3339()}));
    Ok(fork)
}
