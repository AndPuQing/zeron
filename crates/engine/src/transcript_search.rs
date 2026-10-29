//! Full-text search over transcripts stored on this engine.
//!
//! The index is derived data. `transcript-search.sqlite3` holds one FTS5 row
//! per chat and the snapshot `saved_at` it was built from. Refresh diffs the
//! index against `DocsStore` snapshots of the workspace's live chats, so
//! anything that writes a snapshot gets indexed with no hook. That covers live
//! persistence, chat2 seeding, and profile import. Deleting the file only
//! costs a rebuild.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use rusqlite::{Connection, params};
use zeron_doc::{MessagePart, MessageRole, SessionDoc};
use zeron_proto::TranscriptSearchHit;
use zeron_sync::DocsStore;

use crate::EngineError;
use crate::workspace_host::WorkspaceHost;

const SCHEMA_VERSION: i64 = 3;
/// A message's FTS rowid is `chat row * MESSAGE_SLOTS + message index`, so a
/// chat's messages are one rowid range.
// ponytail: messages past a million in one chat go unindexed.
const MESSAGE_SLOTS: i64 = 1_000_000;
const REFRESH_BUDGET: Duration = Duration::from_secs(1);
const SNIPPET_CHARS: usize = 180;

#[derive(Clone)]
pub struct TranscriptSearch {
    inner: Arc<Inner>,
}

struct Inner {
    index: Mutex<Connection>,
    refreshing: Mutex<()>,
    store: Arc<DocsStore>,
    workspace: WorkspaceHost,
}

impl TranscriptSearch {
    pub fn open(
        dir: &Path,
        store: Arc<DocsStore>,
        workspace: WorkspaceHost,
    ) -> Result<Self, EngineError> {
        let conn = Connection::open(dir.join("transcript-search.sqlite3")).map_err(sql)?;
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
                 DROP TABLE IF EXISTS transcripts;
                 CREATE TABLE chats (
                    id       INTEGER PRIMARY KEY,
                    chat_id  TEXT NOT NULL UNIQUE,
                    saved_at INTEGER NOT NULL
                 ) STRICT;
                 CREATE VIRTUAL TABLE transcripts USING fts5(
                    indexed_text, body UNINDEXED, from_user UNINDEXED, message_id UNINDEXED,
                    tokenize = 'unicode61 remove_diacritics 2'
                 );
                 PRAGMA user_version = {SCHEMA_VERSION};"
            ))
            .map_err(sql)?;
        }
        Ok(Self {
            inner: Arc::new(Inner {
                index: Mutex::new(conn),
                refreshing: Mutex::new(()),
                store,
                workspace,
            }),
        })
    }

    /// An empty query only refreshes, so callers can warm the index.
    pub async fn search(
        &self,
        query: String,
        limit: u16,
    ) -> Result<Vec<TranscriptSearchHit>, EngineError> {
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            if let Err(error) = inner.refresh() {
                tracing::warn!(%error, "transcript index refresh failed");
            }
            match match_expression(&query) {
                Some(expr) => inner.query(&expr, &query, limit.clamp(1, 100)),
                None => Ok(Vec::new()),
            }
        })
        .await
        .map_err(|e| EngineError::Other(e.to_string()))?
    }
}

impl Inner {
    /// Reindex chats whose snapshot changed, newest first, within
    /// [`REFRESH_BUDGET`]. A refresh already in flight makes this a no-op.
    fn refresh(&self) -> Result<(), EngineError> {
        let Ok(_refreshing) = self.refreshing.try_lock() else {
            return Ok(());
        };
        let started = Instant::now();
        let live: HashSet<String> = self
            .workspace
            .read_chats()?
            .into_iter()
            .map(|chat| chat.id)
            .collect();
        let mut wanted: Vec<(String, i64)> = self
            .store
            .snapshot_saved_at()?
            .into_iter()
            .filter(|(id, _)| live.contains(id))
            .collect();
        let indexed: HashMap<String, (i64, i64)> = {
            let conn = self.conn();
            let mut stmt = conn
                .prepare("SELECT chat_id, id, saved_at FROM chats")
                .map_err(sql)?;
            stmt.query_map([], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?))))
                .map_err(sql)?
                .collect::<Result<_, _>>()
                .map_err(sql)?
        };
        let wanted_ids: HashSet<&str> = wanted.iter().map(|(id, _)| id.as_str()).collect();
        let gone: Vec<i64> = indexed
            .iter()
            .filter(|(chat_id, _)| !wanted_ids.contains(chat_id.as_str()))
            .map(|(_, (row, _))| *row)
            .collect();
        if !gone.is_empty() {
            let mut conn = self.conn();
            let tx = conn.transaction().map_err(sql)?;
            for row in gone {
                delete_messages(&tx, row)?;
                tx.execute("DELETE FROM chats WHERE id = ?1", params![row])
                    .map_err(sql)?;
            }
            tx.commit().map_err(sql)?;
        }
        wanted.retain(|(id, saved_at)| indexed.get(id).map(|(_, at)| at) != Some(saved_at));
        wanted.sort_by_key(|(_, saved_at)| std::cmp::Reverse(*saved_at));
        for (chat_id, saved_at) in wanted {
            if started.elapsed() > REFRESH_BUDGET {
                break;
            }
            let messages = match self.store.load_snapshot(&chat_id) {
                Ok(Some(bytes)) => transcript_messages(&bytes).unwrap_or_else(|error| {
                    tracing::warn!(chat = %chat_id, %error, "transcript unreadable; indexed empty");
                    Vec::new()
                }),
                Ok(None) => continue,
                Err(error) => return Err(error.into()),
            };
            let mut conn = self.conn();
            let tx = conn.transaction().map_err(sql)?;
            let row: i64 = tx
                .query_row(
                    "INSERT INTO chats (chat_id, saved_at) VALUES (?1, ?2)
                     ON CONFLICT(chat_id) DO UPDATE SET saved_at = excluded.saved_at
                     RETURNING id",
                    params![chat_id, saved_at],
                    |row| row.get(0),
                )
                .map_err(sql)?;
            delete_messages(&tx, row)?;
            for (ix, (message_id, from_user, body)) in (0..MESSAGE_SLOTS).zip(messages) {
                tx.execute(
                    "INSERT INTO transcripts (rowid, indexed_text, body, from_user, message_id) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![row * MESSAGE_SLOTS + ix, searchable_text(&body), body, from_user, message_id],
                )
                .map_err(sql)?;
            }
            tx.commit().map_err(sql)?;
        }
        Ok(())
    }

    fn query(
        &self,
        expr: &str,
        query: &str,
        limit: u16,
    ) -> Result<Vec<TranscriptSearchHit>, EngineError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare_cached(
                "SELECT chats.chat_id, transcripts.body, transcripts.from_user, transcripts.message_id
                 FROM transcripts
                 JOIN chats ON chats.id = transcripts.rowid / ?3
                 WHERE transcripts MATCH ?1
                 ORDER BY rank
                 LIMIT ?2",
            )
            .map_err(sql)?;
        stmt.query_map(params![expr, limit, MESSAGE_SLOTS], |row| {
            Ok(TranscriptSearchHit {
                chat_id: row.get(0)?,
                snippet: snippet(&row.get::<_, String>(1)?, query),
                message_id: row.get(3)?,
                from_user: row.get(2)?,
            })
        })
        .map_err(sql)?
        .collect::<Result<_, _>>()
        .map_err(sql)
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.index.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn delete_messages(tx: &rusqlite::Transaction<'_>, chat_row: i64) -> Result<(), EngineError> {
    tx.execute(
        "DELETE FROM transcripts WHERE rowid >= ?1 AND rowid < ?2",
        params![chat_row * MESSAGE_SLOTS, (chat_row + 1) * MESSAGE_SLOTS],
    )
    .map_err(sql)?;
    Ok(())
}

/// `(from user, text)` per user or assistant message. Reasoning is hidden in
/// the UI, and tool output would drown real matches.
fn transcript_messages(snapshot: &[u8]) -> Result<Vec<(String, bool, String)>, EngineError> {
    let raw = loro::LoroDoc::new();
    raw.import(snapshot)
        .map_err(|e| EngineError::Other(e.to_string()))?;
    let entries = SessionDoc::from_doc(raw).read_entries()?;
    let mut messages = Vec::new();
    for entry in entries {
        if !matches!(entry.role, MessageRole::User | MessageRole::Assistant) {
            continue;
        }
        let mut body = String::new();
        for part in entry.parts {
            if let MessagePart::Text { text, .. } = part {
                body.push_str(&text);
                body.push('\n');
            }
        }
        if !body.trim().is_empty() {
            messages.push((entry.id, entry.role == MessageRole::User, body));
        }
    }
    Ok(messages)
}

/// Every word must appear, each as a quoted prefix so FTS5 operators in user
/// text stay literal. Words without a letter or digit have no tokens to match.
fn match_expression(query: &str) -> Option<String> {
    let terms: Vec<String> = query
        .split_whitespace()
        .filter(|word| word.chars().any(char::is_alphanumeric))
        .map(|word| format!("\"{}\"*", searchable_text(word).trim().replace('"', "\"\"")))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

fn sql(error: rusqlite::Error) -> EngineError {
    EngineError::Other(format!("transcript index: {error}"))
}

// FTS unicode61 treats a contiguous Chinese sentence as one word. Separate
// CJK characters only in the derived index, preserving original display text.
fn searchable_text(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch as u32, 0x3400..=0x9fff | 0xf900..=0xfaff | 0x20000..=0x323af | 0x3040..=0x30ff | 0xac00..=0xd7af)
        {
            result.push(' ');
            result.push(ch);
            result.push(' ');
        } else {
            result.push(ch);
        }
    }
    result
}

fn snippet(body: &str, query: &str) -> String {
    let chars: Vec<char> = body.chars().collect();
    let lower = body.to_lowercase();
    let matched = query
        .split_whitespace()
        .filter_map(|q| lower.find(&q.to_lowercase()))
        .min()
        .unwrap_or(0);
    // Lowercasing may expand characters; clamp the approximate offset safely.
    let position = lower
        .char_indices()
        .take_while(|(i, _)| *i < matched)
        .count()
        .min(chars.len());
    let start = position.saturating_sub(40);
    let end = (start + SNIPPET_CHARS).min(chars.len());
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        chars[start..end].iter().collect::<String>(),
        if end < chars.len() { "…" } else { "" }
    )
}
