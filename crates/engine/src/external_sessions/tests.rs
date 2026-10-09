use super::*;
use crate::EngineCore;
use async_trait::async_trait;
use futures::StreamExt;
use futures::stream::BoxStream;
use std::sync::atomic::AtomicUsize;
use zeron_harness::saved_sessions::{SavedHistoryMessage, SavedSessionPage};
use zeron_harness::{Harness, HarnessError, RunControls};

struct Provider {
    cwd: String,
    copies: AtomicUsize,
    fail_read: AtomicBool,
    store_changed: AtomicBool,
    requests: Mutex<Vec<RunRequest>>,
}

impl Provider {
    fn session(&self) -> SavedSession {
        SavedSession {
            harness: HarnessId::Codex,
            native_id: "original".into(),
            store_id: "canonical-store".into(),
            title: "Imported conversation".into(),
            cwd: self.cwd.clone(),
            created_at_ms: 1000,
            updated_at_ms: 2000,
            model: None,
            locator: None,
            running: false,
        }
    }
}

#[async_trait]
impl Harness for Provider {
    fn id(&self) -> HarnessId {
        HarnessId::Codex
    }
    fn display_name(&self) -> &str {
        "Copy fixture"
    }
    fn supports_steering(&self) -> bool {
        true
    }
    fn steering_mode(&self) -> SteeringMode {
        SteeringMode::StepBoundary
    }
    fn reasoning_levels(&self) -> &[ReasoningLevel] {
        &[]
    }
    async fn models(&self) -> Result<Vec<Model>, HarnessError> {
        Ok(vec![])
    }
    fn saved_session_store_id(&self) -> Result<String, HarnessError> {
        Ok(if self.store_changed.load(Ordering::Relaxed) {
            "other-store"
        } else {
            "canonical-store"
        }
        .into())
    }
    async fn saved_sessions(
        &self,
        _cursor: Option<&str>,
    ) -> Result<SavedSessionPage, HarnessError> {
        Ok(SavedSessionPage {
            sessions: vec![self.session()],
            next_cursor: None,
        })
    }
    async fn copy_saved_session(
        &self,
        source: &SavedSession,
    ) -> Result<SavedSession, HarnessError> {
        let count = self.copies.fetch_add(1, Ordering::Relaxed) + 1;
        Ok(SavedSession {
            native_id: format!("native-copy-{count}"),
            ..source.clone()
        })
    }
    async fn saved_session_history(
        &self,
        session: &SavedSession,
    ) -> Result<SavedSessionHistory, HarnessError> {
        if self.fail_read.swap(false, Ordering::Relaxed) && session.native_id != "original" {
            return Err(HarnessError::Protocol("transient history failure".into()));
        }
        Ok(SavedSessionHistory {
            messages: vec![
                SavedHistoryMessage {
                    id: "historical-user".into(),
                    role: SavedMessageRole::User,
                    timestamp_ms: 1000,
                    parts: vec![SavedHistoryPart::Text("remember imported context".into())],
                },
                SavedHistoryMessage {
                    id: "historical-assistant".into(),
                    role: SavedMessageRole::Assistant,
                    timestamp_ms: 2000,
                    parts: vec![SavedHistoryPart::Text("remembered".into())],
                },
            ],
            notice: None,
        })
    }
    async fn run(
        &self,
        request: RunRequest,
        _controls: RunControls,
    ) -> Result<BoxStream<'static, Result<AgentEvent, HarnessError>>, HarnessError> {
        self.requests.lock().unwrap().push(request.clone());
        assert!(request.require_native_resume);
        let session = request.resume.clone().expect("native copy");
        assert!(session.starts_with("native-copy-"));
        Ok(futures::stream::iter(vec![
            Ok(AgentEvent::SessionStarted {
                harness: HarnessId::Codex,
                model: "test".into(),
                cwd: request.cwd,
                session_id: session.clone(),
                tools: vec![],
                assistant_message_id: "reply".into(),
            }),
            Ok(AgentEvent::TextDelta {
                text: "continued native copy".into(),
            }),
            Ok(AgentEvent::Done {
                status: DoneStatus::Completed,
                result: None,
                error: None,
                session_id: Some(session),
            }),
        ])
        .boxed())
    }
}

fn setup() -> (tempfile::TempDir, Arc<Provider>, EngineCore) {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("project worktree");
    std::fs::create_dir(&cwd).unwrap();
    let provider = Arc::new(Provider {
        cwd: cwd.to_string_lossy().into(),
        copies: AtomicUsize::new(0),
        fail_read: AtomicBool::new(false),
        store_changed: AtomicBool::new(false),
        requests: Mutex::new(vec![]),
    });
    let registry = Arc::new(HarnessRegistry::new());
    registry.register(provider.clone());
    let core =
        EngineCore::assemble(&dir.path().join("data"), registry, HarnessId::Codex, None).unwrap();
    (dir, provider, core)
}

async fn scan(core: &EngineCore) -> String {
    let result = core
        .external_sessions
        .list(Default::default())
        .await
        .unwrap();
    assert_eq!(result.sessions.len(), 1);
    result.sessions[0].source_ref.clone()
}
async fn batch(core: &EngineCore, reference: &str) -> Vec<ExternalSessionImportEvent> {
    let mut events = core
        .external_sessions
        .start(ImportExternalSessionsParams {
            operation_id: crate::new_id(),
            source_refs: vec![reference.into()],
        })
        .unwrap();
    let mut result = vec![];
    while let Some(event) = events.recv().await {
        result.push(event);
    }
    result
}
fn imported(events: &[ExternalSessionImportEvent]) -> String {
    events
        .iter()
        .find_map(|event| {
            if let ExternalSessionImportEvent::Imported { chat_id, .. } = event {
                Some(chat_id.clone())
            } else {
                None
            }
        })
        .unwrap()
}
fn request(cwd: &str) -> RunRequest {
    RunRequest {
        prompt: "continue".into(),
        cwd: cwd.into(),
        harness: None,
        model: None,
        reasoning: None,
        model_options: Default::default(),
        sandbox: SandboxLevel::WorkspaceWrite,
        auto_approve: true,
        attachments: vec![],
        resume: None,
        require_native_resume: false,
        worktree: None,
        mcp: None,
    }
}

#[tokio::test]
async fn import_persists_archived_history_with_new_native_id_and_deduplicates_concurrent_batches() {
    let (_dir, provider, core) = setup();
    let reference = scan(&core).await;
    let (first, second) = tokio::join!(batch(&core, &reference), batch(&core, &reference));
    let chat_id = first
        .iter()
        .chain(&second)
        .find_map(|event| {
            if let ExternalSessionImportEvent::Imported { chat_id, .. } = event {
                Some(chat_id.clone())
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(provider.copies.load(Ordering::Relaxed), 1);
    assert!(
        first
            .iter()
            .chain(&second)
            .any(|event| matches!(event, ExternalSessionImportEvent::AlreadyManaged { .. }))
    );
    let chat = core.workspace.chat(&chat_id).unwrap().unwrap();
    assert!(chat.archived);
    assert!(!chat.unseen());
    assert_eq!(chat.harness_session_id.as_deref(), Some("native-copy-1"));
    assert_eq!(
        chat.import_source.as_ref().unwrap().original_native_id,
        "original"
    );
    let handle = core.doc_host.open(&chat_id).unwrap();
    let messages = handle.doc().read_entries().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].created_at, 1000);
    assert!(
        messages
            .iter()
            .all(|m| m.status == Some(MessageStatus::Complete))
    );
    assert!(
        provider.requests.lock().unwrap().is_empty(),
        "import must not run a model"
    );
    let result = core
        .external_sessions
        .list(Default::default())
        .await
        .unwrap();
    assert_eq!(
        result.sessions[0].already_managed_chat_id.as_deref(),
        Some(chat_id.as_str())
    );
    core.shutdown().await;
}

#[tokio::test]
async fn history_retry_reuses_recorded_copy_even_after_discovery_is_cleared() {
    let (_dir, provider, core) = setup();
    provider.fail_read.store(true, Ordering::Relaxed);
    let reference = scan(&core).await;
    let first = batch(&core, &reference).await;
    assert!(
        first
            .iter()
            .any(|event| matches!(event, ExternalSessionImportEvent::Failed { .. }))
    );
    core.external_sessions.inner.sources.lock().unwrap().clear();
    let second = batch(&core, &reference).await;
    let chat_id = imported(&second);
    assert_eq!(provider.copies.load(Ordering::Relaxed), 1);
    assert_eq!(
        core.workspace
            .chat(&chat_id)
            .unwrap()
            .unwrap()
            .harness_session_id
            .as_deref(),
        Some("native-copy-1")
    );
    core.shutdown().await;
}

#[tokio::test]
async fn imported_history_and_binding_survive_a_full_engine_restart() {
    let (dir, provider, core) = setup();
    let reference = scan(&core).await;
    let chat_id = imported(&batch(&core, &reference).await);
    core.shutdown().await;
    drop(core);
    let registry = Arc::new(HarnessRegistry::new());
    registry.register(provider.clone());
    let restarted =
        EngineCore::assemble(&dir.path().join("data"), registry, HarnessId::Codex, None).unwrap();
    let chat = restarted.workspace.chat(&chat_id).unwrap().unwrap();
    assert!(chat.archived);
    assert_eq!(
        chat.import_source.as_ref().unwrap().copied_native_id,
        "native-copy-1"
    );
    assert_eq!(
        restarted
            .doc_host
            .open(&chat_id)
            .unwrap()
            .doc()
            .read_entries()
            .unwrap()
            .len(),
        2
    );
    let list = restarted
        .external_sessions
        .list(Default::default())
        .await
        .unwrap();
    assert_eq!(
        list.sessions[0].already_managed_chat_id.as_deref(),
        Some(chat_id.as_str())
    );
    assert!(
        batch(&restarted, &reference)
            .await
            .iter()
            .any(|e| matches!(e, ExternalSessionImportEvent::AlreadyManaged { .. }))
    );
    assert_eq!(provider.copies.load(Ordering::Relaxed), 1);
    let mut next = request(&provider.cwd);
    restarted
        .sessions
        .prepare_imported_run(&chat_id, HarnessId::Codex, &mut next)
        .unwrap();
    assert_eq!(next.resume.as_deref(), Some("native-copy-1"));
    restarted.shutdown().await;
}

#[tokio::test]
async fn recovery_publishes_saved_snapshot_and_space_without_replacing_edited_lineage_or_resurrecting_deletions()
 {
    let (_dir, provider, core) = setup();
    let source = provider.session();
    let reference = key(&source);
    let copy = provider.copy_saved_session(&source).await.unwrap();
    let history = provider.saved_session_history(&copy).await.unwrap();
    let receipt = Receipt {
        source,
        chat_id: crate::new_id(),
        copy: Some(copy),
        row: None,
        space: None,
        complete: false,
    };
    let receipt = core
        .external_sessions
        .save_history(&reference, receipt, history)
        .unwrap();
    assert!(core.workspace.chat(&receipt.chat_id).unwrap().is_none());
    assert!(core.workspace.read_spaces().unwrap().is_empty());
    core.external_sessions.recover_publications().unwrap();
    assert!(core.workspace.chat(&receipt.chat_id).unwrap().is_some());
    assert_eq!(core.workspace.read_spaces().unwrap().len(), 1);
    let handle = core.doc_host.open(&receipt.chat_id).unwrap();
    handle
        .write_user_message("local-edit", "keep this activity", 5000)
        .unwrap();
    let snapshot = handle.doc().export_snapshot().unwrap();
    core.external_sessions.save(&reference, &receipt).unwrap(); // crash after publishing but before completion
    core.external_sessions.recover_publications().unwrap();
    assert_eq!(handle.doc().export_snapshot().unwrap(), snapshot);
    assert_eq!(handle.doc().read_entries().unwrap().len(), 3);
    assert!(core.workspace.delete_chat(&receipt.chat_id).unwrap());
    core.external_sessions.save(&reference, &receipt).unwrap();
    core.external_sessions.recover_publications().unwrap();
    assert!(core.workspace.chat(&receipt.chat_id).unwrap().is_none());
    assert!(
        core.external_sessions
            .receipt(&reference)
            .unwrap()
            .unwrap()
            .complete
    );
    core.shutdown().await;
}

#[tokio::test]
async fn imported_binding_is_checked_before_message_write_and_resume_is_owned_by_engine() {
    let (_dir, provider, core) = setup();
    let reference = scan(&core).await;
    let chat_id = imported(&batch(&core, &reference).await);
    let handle = core.doc_host.open(&chat_id).unwrap();
    let mut wrong = request(&provider.cwd);
    wrong.resume = Some("original".into());
    assert!(
        core.sessions
            .dispatch(&chat_id, HarnessId::Codex, wrong, None)
            .await
            .is_err()
    );
    assert!(
        core.sessions
            .dispatch(
                &chat_id,
                HarnessId::ClaudeCode,
                request(&provider.cwd),
                None
            )
            .await
            .is_err()
    );
    assert!(
        core.sessions
            .dispatch(
                &chat_id,
                HarnessId::Codex,
                request("/missing-import-directory"),
                None
            )
            .await
            .is_err()
    );
    provider.store_changed.store(true, Ordering::Relaxed);
    assert!(
        core.sessions
            .dispatch(&chat_id, HarnessId::Codex, request(&provider.cwd), None)
            .await
            .is_err()
    );
    assert!(
        core.sessions
            .steer(&chat_id, "invalid steer", None)
            .await
            .is_err()
    );
    assert_eq!(
        handle.doc().read_entries().unwrap().len(),
        2,
        "invalid sends must not become history"
    );
    provider.store_changed.store(false, Ordering::Relaxed);
    core.sessions
        .dispatch(
            &chat_id,
            HarnessId::Codex,
            request(&provider.cwd),
            Some("new-user".into()),
        )
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while provider.requests.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let request = provider.requests.lock().unwrap()[0].clone();
    assert_eq!(request.resume.as_deref(), Some("native-copy-1"));
    assert!(request.require_native_resume);
    assert_eq!(
        core.workspace
            .chat(&chat_id)
            .unwrap()
            .unwrap()
            .harness_session_id
            .as_deref(),
        Some("native-copy-1")
    );
    core.shutdown().await;
}

#[tokio::test]
async fn cancelled_waiting_batch_copies_nothing_and_engine_shutdown_rejects_new_imports() {
    let (_dir, provider, core) = setup();
    let reference = scan(&core).await;
    let serial = core.external_sessions.inner.serial.lock().await;
    let operation_id = crate::new_id();
    let mut rx = core
        .external_sessions
        .start(ImportExternalSessionsParams {
            operation_id: operation_id.clone(),
            source_refs: vec![reference],
        })
        .unwrap();
    core.external_sessions.cancel(&operation_id);
    drop(serial);
    assert!(matches!(
        rx.recv().await,
        Some(ExternalSessionImportEvent::Finished { cancelled: true })
    ));
    assert_eq!(provider.copies.load(Ordering::Relaxed), 0);
    core.shutdown().await;
    assert!(
        core.external_sessions
            .start(ImportExternalSessionsParams {
                operation_id: crate::new_id(),
                source_refs: vec!["anything".into()]
            })
            .is_err()
    );
}
