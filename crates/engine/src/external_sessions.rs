//! Native copies and ordinary chat documents, with durable per-source recovery.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use zeron_doc::{MessagePart, MessageRole, MessageStatus, SessionDoc, SessionMessageEntry};
use zeron_harness::saved_sessions::{
    SavedHistoryPart, SavedMessageRole, SavedSession, SavedSessionHistory,
};
use zeron_proto::*;
use zeron_sync::DocsStore;

use crate::{DocHost, EngineError, HarnessRegistry, WorkspaceHost};

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub struct ExternalSessionImporter {
    inner: Arc<Inner>,
}

struct Inner {
    store: Arc<DocsStore>,
    registry: Arc<HarnessRegistry>,
    workspace: WorkspaceHost,
    docs: DocHost,
    sources: Mutex<HashMap<String, SavedSession>>,
    serial: tokio::sync::Mutex<()>,
    operations: Mutex<HashMap<String, Arc<AtomicBool>>>,
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    stopped: AtomicBool,
}

#[derive(Clone, Serialize, Deserialize)]
struct Receipt {
    source: SavedSession,
    chat_id: String,
    copy: Option<SavedSession>,
    row: Option<Chat>,
    #[serde(default)]
    space: Option<Space>,
    complete: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct Cursor {
    codex: Option<String>,
    claude: Option<String>,
    codex_done: bool,
    claude_done: bool,
}

fn error(message: impl Into<String>) -> EngineError {
    EngineError::Other(message.into())
}

fn key(session: &SavedSession) -> String {
    let mut hash = Sha256::new();
    hash.update(format!(
        "{:?}\0{}\0{}",
        session.harness, session.store_id, session.native_id
    ));
    format!("{:x}", hash.finalize())
}

fn decode_receipt(bytes: &str) -> Result<Receipt, EngineError> {
    serde_json::from_str(bytes).map_err(|e| error(format!("Import receipt could not be read: {e}")))
}

impl ExternalSessionImporter {
    pub fn new(
        store: Arc<DocsStore>,
        registry: Arc<HarnessRegistry>,
        workspace: WorkspaceHost,
        docs: DocHost,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                registry,
                workspace,
                docs,
                sources: Mutex::new(HashMap::new()),
                serial: tokio::sync::Mutex::new(()),
                operations: Mutex::new(HashMap::new()),
                tasks: Mutex::new(Vec::new()),
                stopped: AtomicBool::new(false),
            }),
        }
    }

    pub async fn shutdown(&self) {
        self.inner.stopped.store(true, Ordering::Release);
        for cancel in self
            .inner
            .operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
        {
            cancel.store(true, Ordering::Release);
        }
        let tasks = std::mem::take(
            &mut *self
                .inner
                .tasks
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        for task in tasks {
            let _ = task.await;
        }
    }

    fn receipt(&self, source: &str) -> Result<Option<Receipt>, EngineError> {
        self.inner
            .store
            .external_session_import(source)?
            .as_deref()
            .map(decode_receipt)
            .transpose()
    }

    fn save(&self, source: &str, receipt: &Receipt) -> Result<(), EngineError> {
        let bytes = serde_json::to_string(receipt).map_err(|e| error(e.to_string()))?;
        self.inner
            .store
            .save_external_session_import(source, &bytes)?;
        Ok(())
    }

    /// Finish publication after a crash without replacing an existing document lineage.
    pub fn recover_publications(&self) -> Result<(), EngineError> {
        for bytes in self.inner.store.external_session_imports()? {
            let mut receipt = match decode_receipt(&bytes) {
                Ok(receipt) => receipt,
                Err(cause) => {
                    tracing::error!(error = %cause, "invalid session import receipt");
                    continue;
                }
            };
            if !receipt.complete && receipt.row.is_some() {
                let source_key = key(&receipt.source);
                if self.inner.workspace.chat_deleted(&receipt.chat_id) {
                    receipt.complete = true;
                    self.save(&source_key, &receipt)?;
                    continue;
                }
                if let Err(cause) = self.publish(&source_key, &mut receipt) {
                    tracing::error!(chat = %receipt.chat_id, error = %cause, "session import recovery deferred");
                }
            }
        }
        Ok(())
    }

    fn managed(&self, session: &SavedSession) -> Result<Option<String>, EngineError> {
        let device = self.inner.docs.device_id();
        Ok(self
            .inner
            .workspace
            .read_chats()?
            .into_iter()
            .find(|chat| {
                chat.device_id == device
                    && ((chat
                        .config
                        .as_ref()
                        .is_some_and(|config| config.harness == session.harness)
                        && chat.harness_session_id.as_deref() == Some(session.native_id.as_str()))
                        || chat.import_source.as_ref().is_some_and(|source| {
                            source.harness == session.harness
                                && source.store_id == session.store_id
                                && source.original_native_id == session.native_id
                        }))
            })
            .map(|chat| chat.id))
    }

    fn source(&self, reference: &str) -> Result<SavedSession, EngineError> {
        self.inner
            .sources
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(reference)
            .cloned()
            .ok_or_else(|| error("This session is no longer in the discovery list. Scan again."))
    }

    pub async fn list(
        &self,
        params: ListExternalSessionsParams,
    ) -> Result<ExternalSessionList, EngineError> {
        if params.cursor.is_none() {
            self.inner
                .sources
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clear();
        }
        if params
            .cursor
            .as_ref()
            .is_some_and(|cursor| cursor.len() > 8192)
        {
            return Err(error("Invalid discovery cursor."));
        }
        let mut cursor: Cursor = params
            .cursor
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| error("Invalid discovery cursor."))?
            .unwrap_or_default();
        let mut result = ExternalSessionList::default();
        for id in [HarnessId::Codex, HarnessId::ClaudeCode] {
            let (next, done) = if id == HarnessId::Codex {
                (&mut cursor.codex, &mut cursor.codex_done)
            } else {
                (&mut cursor.claude, &mut cursor.claude_done)
            };
            if *done {
                continue;
            }
            let available = self
                .inner
                .registry
                .descriptors()
                .into_iter()
                .any(|item| item.id == id && item.installed && item.enabled != Some(false));
            if !available {
                *done = true;
                result.source_errors.push(format!(
                    "{id:?} is not installed or enabled on this device."
                ));
                continue;
            }
            let lease = self.inner.registry.execution_lease(id).await;
            let provider = self.inner.registry.resolve(id)?;
            let provider_cursor = next.clone();
            let page = tokio::spawn(async move {
                let _lease = lease;
                provider.saved_sessions(provider_cursor.as_deref()).await
            })
            .await
            .map_err(|e| error(e.to_string()))?;
            match page {
                Ok(page) => {
                    *next = page.next_cursor;
                    *done = next.is_none();
                    for session in page.sessions {
                        let source_ref = key(&session);
                        let already_managed_chat_id = self.managed(&session)?;
                        let unavailable_reason = (!Path::new(&session.cwd).is_absolute()
                            || !Path::new(&session.cwd).is_dir())
                        .then(|| "The original working directory is unavailable.".into());
                        result.sessions.push(ExternalSessionEntry {
                            source_ref: source_ref.clone(),
                            harness: id,
                            title: session.title.clone(),
                            cwd: session.cwd.clone(),
                            updated_at_ms: session.updated_at_ms,
                            already_managed_chat_id,
                            unavailable_reason,
                            running: session.running,
                        });
                        let mut sources = self
                            .inner
                            .sources
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        if sources.len() >= 5000 && !sources.contains_key(&source_ref) {
                            return Err(error(
                                "Too many sessions have been loaded. Reopen the importer to scan again.",
                            ));
                        }
                        sources.insert(source_ref, session);
                    }
                }
                Err(cause) => {
                    *done = true;
                    result.source_errors.push(format!("{id:?}: {cause}"));
                }
            }
        }
        result
            .sessions
            .sort_by_key(|session| std::cmp::Reverse(session.updated_at_ms));
        if !cursor.codex_done || !cursor.claude_done {
            result.next_cursor =
                Some(serde_json::to_string(&cursor).map_err(|e| error(e.to_string()))?);
        }
        Ok(result)
    }

    pub fn cancel(&self, operation_id: &str) {
        if let Some(cancelled) = self
            .inner
            .operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(operation_id)
        {
            cancelled.store(true, Ordering::Release);
        }
    }

    pub fn start(
        &self,
        params: ImportExternalSessionsParams,
    ) -> Result<mpsc::UnboundedReceiver<ExternalSessionImportEvent>, EngineError> {
        let mut tasks = self
            .inner
            .tasks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.inner.stopped.load(Ordering::Acquire) {
            return Err(error("This engine is shutting down."));
        }
        tasks.retain(|task| !task.is_finished());
        if uuid::Uuid::parse_str(&params.operation_id).is_err()
            || params.source_refs.is_empty()
            || params.source_refs.len() > MAX_EXTERNAL_SESSION_IMPORT_BATCH
        {
            return Err(error(
                "Choose between 1 and 200 sessions with a valid import operation ID.",
            ));
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        // A rescan may clear discovery while this batch is queued. Capture
        // the selected metadata now; receipts govern resumed attempts.
        let selected = params
            .source_refs
            .iter()
            .map(|reference| {
                self.receipt(reference).and_then(|receipt| {
                    receipt
                        .filter(|r| !r.complete && !self.inner.workspace.chat_deleted(&r.chat_id))
                        .map(|r| r.source)
                        .map(Ok)
                        .unwrap_or_else(|| self.source(reference))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        {
            let mut operations = self
                .inner
                .operations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if operations.contains_key(&params.operation_id) {
                return Err(error("This import operation is already running."));
            }
            operations.insert(params.operation_id.clone(), cancelled.clone());
        }
        let (sender, receiver) = mpsc::unbounded_channel();
        let importer = self.clone();
        tasks.push(tokio::spawn(async move {
            let _serial = importer.inner.serial.lock().await;
            for (source_ref, source) in params.source_refs.iter().zip(selected) {
                if sender.is_closed() {
                    cancelled.store(true, Ordering::Release);
                }
                if importer.inner.stopped.load(Ordering::Acquire) {
                    cancelled.store(true, Ordering::Release);
                }
                if cancelled.load(Ordering::Acquire) {
                    break;
                }
                let _ = sender.send(ExternalSessionImportEvent::Importing {
                    source_ref: source_ref.clone(),
                });
                let event = match importer.import_one(source_ref, source).await {
                    Ok(event) => event,
                    Err(cause) => ExternalSessionImportEvent::Failed {
                        source_ref: source_ref.clone(),
                        reason: cause.to_string(),
                        retryable: true,
                    },
                };
                let _ = sender.send(event);
            }
            let _ = sender.send(ExternalSessionImportEvent::Finished {
                cancelled: cancelled.load(Ordering::Acquire),
            });
            importer
                .inner
                .operations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&params.operation_id);
        }));
        Ok(receiver)
    }

    fn require_provider(&self, harness: HarnessId) -> Result<(), EngineError> {
        if !self
            .inner
            .registry
            .descriptors()
            .iter()
            .any(|item| item.id == harness && item.installed && item.enabled != Some(false))
        {
            return Err(error(
                "The provider is not installed or enabled on this device.",
            ));
        }
        Ok(())
    }

    async fn import_one(
        &self,
        source_ref: &str,
        source: SavedSession,
    ) -> Result<ExternalSessionImportEvent, EngineError> {
        let pending = self
            .receipt(source_ref)?
            .filter(|r| !r.complete && !self.inner.workspace.chat_deleted(&r.chat_id));
        if let Some(mut receipt) = pending.as_ref().filter(|r| r.row.is_some()).cloned() {
            self.publish(source_ref, &mut receipt)?;
            return Ok(ExternalSessionImportEvent::Imported {
                source_ref: source_ref.into(),
                chat_id: receipt.chat_id,
            });
        }
        if let Some(chat_id) = self.managed(&source)? {
            return Ok(ExternalSessionImportEvent::AlreadyManaged {
                source_ref: source_ref.into(),
                chat_id,
            });
        }
        if !Path::new(&source.cwd).is_absolute() || !Path::new(&source.cwd).is_dir() {
            return Err(error("The original working directory is unavailable."));
        }
        let mut receipt = match pending {
            Some(receipt) => receipt,
            None => Receipt {
                source: source.clone(),
                chat_id: crate::new_id(),
                copy: None,
                row: None,
                space: None,
                complete: false,
            },
        };
        self.save(source_ref, &receipt)?;
        let _lease = self.inner.registry.execution_lease(source.harness).await;
        self.require_provider(source.harness)?;
        let provider = self.inner.registry.resolve(source.harness)?;
        if provider.saved_session_store_id()? != source.store_id {
            return Err(error("The provider history location changed. Scan again."));
        }
        if receipt.copy.is_none() {
            let copy = provider.copy_saved_session(&source).await?;
            if copy.native_id.is_empty()
                || copy.native_id == source.native_id
                || copy.store_id != source.store_id
                || copy.cwd != source.cwd
            {
                return Err(error(
                    "The provider did not create an independent native copy in the original workspace.",
                ));
            }
            receipt.copy = Some(copy);
            self.save(source_ref, &receipt)?;
        }
        if receipt.row.is_none() {
            let copy = receipt
                .copy
                .as_ref()
                .ok_or_else(|| error("The native copy is unavailable."))?;
            let history = provider.saved_session_history(copy).await?;
            history.check_budget()?;
            let importer = self.clone();
            let source_ref = source_ref.to_owned();
            receipt = tokio::task::spawn_blocking(move || {
                importer.save_history(&source_ref, receipt, history)
            })
            .await
            .map_err(|e| error(e.to_string()))??;
        }
        let importer = self.clone();
        let reference = source_ref.to_owned();
        let chat_id = receipt.chat_id.clone();
        tokio::task::spawn_blocking(move || importer.publish(&reference, &mut receipt))
            .await
            .map_err(|e| error(e.to_string()))??;
        Ok(ExternalSessionImportEvent::Imported {
            source_ref: source_ref.into(),
            chat_id,
        })
    }

    fn save_history(
        &self,
        source_ref: &str,
        mut receipt: Receipt,
        history: SavedSessionHistory,
    ) -> Result<Receipt, EngineError> {
        let source = &receipt.source;
        let copy = receipt
            .copy
            .as_ref()
            .ok_or_else(|| error("The native copy is unavailable."))?;
        let doc = SessionDoc::init(&receipt.chat_id)?;
        for message in &history.messages {
            let id = format!("import:{}", message.id);
            let parts = message
                .parts
                .iter()
                .enumerate()
                .map(|(index, part)| {
                    let id = format!("{id}:{index}");
                    match part {
                        SavedHistoryPart::Text(text) => MessagePart::Text {
                            id,
                            text: text.clone(),
                        },
                        SavedHistoryPart::Reasoning(text) => MessagePart::Reasoning {
                            id,
                            text: text.clone(),
                        },
                        SavedHistoryPart::Tool {
                            call,
                            output,
                            is_error,
                        } => MessagePart::Tool {
                            id,
                            call: zeron_doc::sanitize_tool_call(call),
                            is_error: *is_error,
                            resolved: true,
                            output: output.as_deref().and_then(zeron_doc::summarize_tool_output),
                            diff: None,
                            output_ref: None,
                            output_bytes: None,
                            diff_ref: None,
                            diff_stats: None,
                            subagent_ref: None,
                            subagent_status: None,
                            subagent_tail: None,
                        },
                    }
                })
                .collect::<Vec<_>>();
            for (index, parts) in zeron_doc::split_parts(&parts).into_iter().enumerate() {
                doc.push_message(&SessionMessageEntry {
                    id: if index == 0 {
                        id.clone()
                    } else {
                        zeron_doc::continuation_id(&id, index)
                    },
                    role: match message.role {
                        SavedMessageRole::User => MessageRole::User,
                        SavedMessageRole::Assistant => MessageRole::Assistant,
                        SavedMessageRole::System => MessageRole::System,
                    },
                    parts,
                    created_at: message.timestamp_ms,
                    device_id: self.inner.docs.device_id().into(),
                    status: Some(MessageStatus::Complete),
                    continuation_of: (index > 0).then(|| id.clone()),
                    duration_ms: None,
                })?;
            }
        }
        let spaces = self.inner.workspace.read_spaces()?;
        let cwd = std::fs::canonicalize(&source.cwd)?;
        let space = spaces
            .into_iter()
            .filter(|space| space.device_id == self.inner.docs.device_id())
            .filter(|space| {
                std::fs::canonicalize(&space.path).is_ok_and(|path| cwd.starts_with(path))
            })
            .max_by_key(|space| space.path.len());
        let space = match space {
            Some(space) => space,
            None => {
                let space = Space {
                    id: crate::new_id(),
                    device_id: self.inner.docs.device_id().into(),
                    path: source.cwd.clone(),
                    name: None,
                    git_detected: false,
                    git_checked_at: None,
                    checkout_id: None,
                    repository_id: None,
                    created_at: Utc::now(),
                };
                receipt.space = Some(space.clone());
                space
            }
        };
        let last_at = DateTime::from_timestamp_millis(source.updated_at_ms);
        let preview = history
            .messages
            .iter()
            .rev()
            .flat_map(|message| &message.parts)
            .find_map(|part| {
                if let SavedHistoryPart::Text(text) = part {
                    Some(text.chars().take(160).collect::<String>())
                } else {
                    None
                }
            });
        receipt.row = Some(Chat {
            id: receipt.chat_id.clone(),
            device_id: self.inner.docs.device_id().into(),
            title: Some(source.title.clone()),
            archived: true,
            cwd: Some(source.cwd.clone()),
            branch: None,
            checkout_id: None,
            source_context: None,
            config: Some(ChatConfig {
                harness: source.harness,
                model: copy.model.clone().or_else(|| source.model.clone()),
                reasoning: None,
                model_options: Default::default(),
                sandbox: SandboxLevel::WorkspaceWrite,
            }),
            last_message_preview: preview,
            last_message_at: last_at,
            created_at: DateTime::from_timestamp_millis(source.created_at_ms)
                .unwrap_or_else(Utc::now),
            harness_session_id: Some(copy.native_id.clone()),
            harness_session_cwd: Some(copy.cwd.clone()),
            space_id: Some(space.id),
            last_seen_at: last_at,
            room_gen: Some(crate::chat2_host::CHAT2_DOC_EPOCH),
            parent_chat_id: None,
            import_source: Some(SessionImportSource {
                harness: source.harness,
                device_id: self.inner.docs.device_id().into(),
                store_id: source.store_id.clone(),
                original_native_id: source.native_id.clone(),
                copied_native_id: copy.native_id.clone(),
                imported_at_ms: crate::now_ms(),
                history_notice: history.notice,
            }),
        });
        let bytes = doc.export_snapshot()?;
        self.inner.store.save_external_session_import_snapshot(
            source_ref,
            &serde_json::to_string(&receipt).map_err(|e| error(e.to_string()))?,
            &receipt.chat_id,
            &bytes,
            crate::chat2_host::CHAT2_DOC_EPOCH,
        )?;
        Ok(receipt)
    }

    fn publish(&self, source_ref: &str, receipt: &mut Receipt) -> Result<(), EngineError> {
        if !self.inner.store.has_snapshot(&receipt.chat_id)? {
            return Err(error("The saved import snapshot is unavailable."));
        }
        if self.inner.workspace.chat(&receipt.chat_id)?.is_none() {
            if self.inner.workspace.chat_deleted(&receipt.chat_id) {
                return Err(error("The import destination was deleted. Scan again."));
            }
            if let Some(space) = &receipt.space
                && !self
                    .inner
                    .workspace
                    .read_spaces()?
                    .iter()
                    .any(|row| row.id == space.id)
            {
                self.inner.workspace.import_space_row(space)?;
            }
            self.inner.workspace.import_chat_row(
                receipt
                    .row
                    .as_ref()
                    .ok_or_else(|| error("The saved import binding is unavailable."))?,
            )?;
        }
        self.inner.workspace.flush_checked()?;
        self.inner.docs.enqueue_wakeup(&receipt.chat_id)?;
        receipt.complete = true;
        self.save(source_ref, receipt)
    }
}
