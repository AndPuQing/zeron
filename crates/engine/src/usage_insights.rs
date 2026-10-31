//! Personal usage insights over the transcripts stored on this engine.
//!
//! Like transcript search, the cache is derived data. `usage-insights.sqlite3`
//! holds one row per chat: its [`ChatUsage`] (`zeron_doc::usage`), and the
//! snapshot `saved_at` and harness attribution it was read with. A request
//! re-reads the chats where either changed since, within a time budget, then
//! aggregates every row. Nothing leaves the device; deleting the file only
//! costs a re-read, so a cache that won't open is rebuilt rather than
//! switching the feature off.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use rusqlite::{Connection, params};
use zeron_doc::SessionDoc;
use zeron_proto::Chat;
use zeron_proto::usage_insights::{ChatUsage, UsageInsightsReport, aggregate};
use zeron_sync::DocsStore;

use crate::EngineError;
use crate::workspace_host::WorkspaceHost;

/// Bump to re-read every chat when what a summary records changes.
const SCHEMA_VERSION: i64 = 2;
const REFRESH_BUDGET: Duration = Duration::from_secs(2);
const CACHE_FILE: &str = "usage-insights.sqlite3";

#[derive(Clone)]
pub struct UsageInsights {
    inner: Arc<Inner>,
}

struct Inner {
    cache: Mutex<Connection>,
    refreshing: Mutex<()>,
    store: Arc<DocsStore>,
    workspace: WorkspaceHost,
    /// Chats whose snapshot could not be loaded: tried after every other
    /// chat, so one bad row neither fails the report nor delays the rest.
    unloadable: Mutex<HashSet<String>>,
}

impl UsageInsights {
    pub fn open(
        dir: &Path,
        store: Arc<DocsStore>,
        workspace: WorkspaceHost,
    ) -> Result<Self, EngineError> {
        Ok(Self {
            inner: Arc::new(Inner {
                cache: Mutex::new(open_cache(dir)?),
                refreshing: Mutex::new(()),
                store,
                workspace,
                unloadable: Mutex::new(HashSet::new()),
            }),
        })
    }

    /// The report over turns since `since_ms` (all when `None`). `indexing`
    /// says some changed chats are still unread; ask again for them.
    pub async fn report(&self, since_ms: Option<i64>) -> Result<UsageInsightsReport, EngineError> {
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            let indexing = inner.refresh(REFRESH_BUDGET)?;
            let mut report = aggregate(&inner.cached()?, since_ms);
            report.indexing = indexing;
            report.generated_at = chrono::Utc::now().timestamp_millis();
            Ok(report)
        })
        .await
        .map_err(|e| EngineError::Other(e.to_string()))?
    }
}

/// The cache in `dir`. One that won't open (corrupt, not a database) is
/// deleted and rebuilt from the transcripts; one that can't be written at
/// all is kept in memory for this run.
fn open_cache(dir: &Path) -> Result<Connection, EngineError> {
    let path = dir.join(CACHE_FILE);
    let error = match prepare_cache(Connection::open(&path)) {
        Ok(conn) => return Ok(conn),
        Err(error) => error,
    };
    tracing::warn!(%error, "usage insights cache unreadable; rebuilding it");
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(dir.join(format!("{CACHE_FILE}{suffix}")));
    }
    prepare_cache(Connection::open(&path)).or_else(|error| {
        tracing::warn!(%error, "usage insights cache unwritable; keeping it in memory");
        prepare_cache(Connection::open_in_memory())
    })
}

fn prepare_cache(conn: rusqlite::Result<Connection>) -> Result<Connection, EngineError> {
    let conn = conn.map_err(sql)?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(sql)?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(sql)?;
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(sql)?;
    if version != SCHEMA_VERSION {
        conn.execute_batch(&format!(
            "DROP TABLE IF EXISTS chats;
             CREATE TABLE chats (
                chat_id  TEXT PRIMARY KEY,
                saved_at INTEGER NOT NULL,
                harness  TEXT,
                usage    TEXT NOT NULL
             ) STRICT;
             PRAGMA user_version = {SCHEMA_VERSION};"
        ))
        .map_err(sql)?;
    }
    Ok(conn)
}

/// The chats the report covers: every session the person ran, archived ones
/// included, but not the hidden voice orchestrators.
fn counted(chat: &Chat) -> bool {
    !zeron_proto::voice::is_orchestrator_chat(&chat.id)
}

/// The harness that runs `chat` now: whichever last held its native
/// session, else the one it is configured for.
fn latest_harness(chat: &Chat) -> Option<String> {
    chat.harness_session_harness
        .clone()
        .filter(|slug| zeron_proto::HarnessId::from_slug(slug).is_some())
        .or_else(|| chat.config.as_ref().map(|c| c.harness.as_str().to_owned()))
}

impl Inner {
    /// Re-read chats whose snapshot or harness attribution changed, newest
    /// first, within `budget`; drop rows for chats that are gone. Returns
    /// whether any are left. A refresh already in flight makes this a no-op
    /// that reports indexing.
    fn refresh(&self, budget: Duration) -> Result<bool, EngineError> {
        let Ok(_refreshing) = self.refreshing.try_lock() else {
            return Ok(true);
        };
        let started = Instant::now();
        let chats: HashMap<String, Chat> = self
            .workspace
            .read_chats()?
            .into_iter()
            .filter(counted)
            .map(|chat| (chat.id.clone(), chat))
            .collect();
        let mut wanted: Vec<(String, i64)> = self
            .store
            .snapshot_saved_at()?
            .into_iter()
            .filter(|(id, _)| chats.contains_key(id))
            .collect();
        let cached: HashMap<String, (i64, Option<String>)> = {
            let conn = self.conn();
            let mut stmt = conn
                .prepare("SELECT chat_id, saved_at, harness FROM chats")
                .map_err(sql)?;
            stmt.query_map([], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?))))
                .map_err(sql)?
                .collect::<Result<_, _>>()
                .map_err(sql)?
        };
        let wanted_ids: HashSet<&str> = wanted.iter().map(|(id, _)| id.as_str()).collect();
        let gone: Vec<&String> = cached
            .keys()
            .filter(|id| !wanted_ids.contains(id.as_str()))
            .collect();
        if !gone.is_empty() {
            let mut conn = self.conn();
            let tx = conn.transaction().map_err(sql)?;
            for id in gone {
                tx.execute("DELETE FROM chats WHERE chat_id = ?1", params![id])
                    .map_err(sql)?;
            }
            tx.commit().map_err(sql)?;
        }
        // A row is current while both inputs of `read_usage` are: the
        // snapshot, and the chat's harness (metadata that can change alone).
        wanted.retain(|(id, saved_at)| {
            let harness = chats.get(id).and_then(latest_harness);
            cached.get(id) != Some(&(*saved_at, harness))
        });
        let unloadable = self
            .unloadable
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        wanted.sort_by_key(|(id, saved_at)| {
            (
                unloadable.contains(id),
                cached.contains_key(id),
                std::cmp::Reverse(*saved_at),
            )
        });
        let mut processed = 0;
        for (chat_id, saved_at) in wanted {
            // Always settle one chat before yielding, or a slow store could
            // report indexing forever without progress.
            if processed > 0 && started.elapsed() > budget {
                return Ok(true);
            }
            let Some(chat) = chats.get(&chat_id) else {
                continue;
            };
            let usage = match self.store.load_snapshot(&chat_id) {
                Ok(Some(bytes)) => read_usage(&bytes, chat).unwrap_or_else(|error| {
                    tracing::warn!(chat = %chat_id, %error, "transcript unreadable; no usage counted");
                    empty_usage()
                }),
                Ok(None) => continue,
                // Left uncounted and tried again on a later request, after
                // the rest: one bad row must not fail the whole report.
                Err(error) => {
                    tracing::warn!(chat = %chat_id, %error, "transcript snapshot unavailable; skipped");
                    self.unloadable
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(chat_id);
                    continue;
                }
            };
            self.unloadable
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&chat_id);
            let json =
                serde_json::to_string(&usage).map_err(|e| EngineError::Other(e.to_string()))?;
            self.conn()
                .execute(
                    "INSERT INTO chats (chat_id, saved_at, harness, usage) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(chat_id) DO UPDATE SET saved_at = excluded.saved_at,
                        harness = excluded.harness, usage = excluded.usage",
                    params![chat_id, saved_at, latest_harness(chat), json],
                )
                .map_err(sql)?;
            processed += 1;
        }
        Ok(false)
    }

    /// Every cached chat's usage. A row this build cannot read counts as
    /// nothing rather than failing the report.
    fn cached(&self) -> Result<Vec<ChatUsage>, EngineError> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT usage FROM chats").map_err(sql)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        Ok(rows
            .into_iter()
            .filter_map(|json| serde_json::from_str(&json).ok())
            .collect())
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn empty_usage() -> ChatUsage {
    ChatUsage {
        category: zeron_proto::usage_insights::TaskCategory::Other,
        turns: Vec::new(),
    }
}

fn read_usage(snapshot: &[u8], chat: &Chat) -> Result<ChatUsage, EngineError> {
    let Some(harness) = latest_harness(chat) else {
        return Ok(empty_usage());
    };
    let raw = loro::LoroDoc::new();
    raw.import(snapshot)
        .map_err(|e| EngineError::Other(e.to_string()))?;
    let entries = SessionDoc::from_doc(raw).read_entries()?;
    Ok(zeron_doc::usage::chat_usage(entries, &harness))
}

fn sql(error: rusqlite::Error) -> EngineError {
    EngineError::Other(format!("usage insights: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeron_doc::{MessagePart, MessageRole, MessageStatus, SessionMessageEntry};
    use zeron_proto::HarnessId;

    fn chat(id: &str, harness: HarnessId) -> Chat {
        let mut chat: Chat = serde_json::from_value(serde_json::json!({
            "id": id,
            "deviceId": "dev",
            "title": null,
            "archived": false,
            "cwd": null,
            "branch": null,
            "checkoutId": null,
            "config": null,
            "lastMessagePreview": null,
            "lastMessageAt": null,
            "createdAt": "2026-10-01T00:00:00Z",
        }))
        .unwrap();
        chat.config = Some(zeron_proto::ChatConfig {
            harness,
            model: None,
            reasoning: None,
            model_options: Default::default(),
            sandbox: zeron_proto::SandboxLevel::ReadOnly,
            access: Default::default(),
            plan: false,
        });
        chat
    }

    fn message(id: &str, role: MessageRole, text: &str) -> SessionMessageEntry {
        SessionMessageEntry {
            id: id.into(),
            role,
            parts: vec![MessagePart::Text {
                id: format!("{id}-t"),
                text: text.into(),
            }],
            created_at: 1_000,
            device_id: "dev".into(),
            status: Some(MessageStatus::Complete),
            continuation_of: None,
            duration_ms: (role == MessageRole::Assistant).then_some(1_500),
        }
    }

    fn snapshot(chat_id: &str, entries: &[SessionMessageEntry]) -> Vec<u8> {
        let doc = SessionDoc::init(chat_id).unwrap();
        for entry in entries {
            doc.push_message(entry).unwrap();
        }
        doc.export_snapshot().unwrap()
    }

    #[test]
    fn an_unreadable_cache_is_rebuilt_not_left_off() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(CACHE_FILE), vec![0xABu8; 4096]).unwrap();
        let conn = open_cache(dir.path()).unwrap();
        conn.execute(
            "INSERT INTO chats (chat_id, saved_at, harness, usage) VALUES ('c', 1, 'codex', '{}')",
            [],
        )
        .unwrap();
        drop(conn);
        let rebuilt = std::fs::read(dir.path().join(CACHE_FILE)).unwrap();
        assert!(rebuilt.starts_with(b"SQLite format 3\0"));
        let reopened = open_cache(dir.path()).unwrap();
        let rows: i64 = reopened
            .query_row("SELECT count(*) FROM chats", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1, "a healthy cache is kept across opens");
    }

    #[test]
    fn an_unwritable_cache_keeps_working_in_memory() {
        let dir = tempfile::tempdir().unwrap();
        let blocked = dir.path().join(CACHE_FILE);
        std::fs::create_dir(&blocked).unwrap();
        std::fs::write(blocked.join("keep"), "untouched").unwrap();

        let conn = open_cache(dir.path()).unwrap();
        let filename: String = conn
            .query_row("PRAGMA database_list", [], |row| row.get(2))
            .unwrap();
        assert!(filename.is_empty(), "the fallback is an in-memory database");
        conn.execute(
            "INSERT INTO chats (chat_id, saved_at, harness, usage) VALUES ('c', 1, 'codex', '{}')",
            [],
        )
        .unwrap();
        let rows: i64 = conn
            .query_row("SELECT count(*) FROM chats", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1);
        assert_eq!(
            std::fs::read_to_string(blocked.join("keep")).unwrap(),
            "untouched"
        );
    }

    #[test]
    fn the_latest_native_session_owner_beats_the_configured_harness() {
        let mut row = chat("c", HarnessId::Codex);
        assert_eq!(latest_harness(&row).as_deref(), Some("codex"));
        row.harness_session_harness = Some("claude-code".into());
        assert_eq!(latest_harness(&row).as_deref(), Some("claude-code"));
        row.harness_session_harness = Some("some-future-agent".into());
        assert_eq!(latest_harness(&row).as_deref(), Some("codex"));
        row.config = None;
        assert_eq!(latest_harness(&row), None);
    }

    #[test]
    fn a_snapshot_reads_as_the_chats_usage() {
        let bytes = snapshot(
            "c",
            &[
                message("u1", MessageRole::User, "Fix the login crash"),
                message("a1", MessageRole::Assistant, "Fixed."),
                message("u2", MessageRole::User, "还是不行"),
                message("a2", MessageRole::Assistant, "Fixed for real."),
            ],
        );
        let usage = read_usage(&bytes, &chat("c", HarnessId::Opencode)).unwrap();
        assert_eq!(
            usage.category,
            zeron_proto::usage_insights::TaskCategory::Bugfix
        );
        assert_eq!(usage.turns.len(), 2);
        assert!(usage.turns.iter().all(|t| t.harness == "opencode"));
        assert!(usage.turns[0].reworked);
        assert!(!usage.turns[1].reworked);
        assert_eq!(usage.turns[0].duration_ms, Some(1_500));
    }
}
