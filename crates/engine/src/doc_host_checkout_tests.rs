//! Deterministic races at the checkout switch's existing critical section.
use super::*;
use futures::{StreamExt, stream::BoxStream};
use std::time::Duration;
use zeron_harness::{Harness, HarnessError, RunControls};
use zeron_proto::{
    AgentEvent, DoneStatus, Model, ReasoningLevel, RunRequest, SteeringMode, UserInputQuestion,
};

struct Probe {
    requests: tokio::sync::mpsc::UnboundedSender<RunRequest>,
    ask: bool,
}

fn question() -> UserInputQuestion {
    UserInputQuestion {
        id: "q".into(),
        header: "Continue".into(),
        question: "Proceed?".into(),
        options: vec!["yes".into()],
        prefill: None,
        multiline: false,
        multi_select: false,
    }
}

#[async_trait::async_trait]
impl Harness for Probe {
    fn id(&self) -> HarnessId {
        HarnessId::Mock
    }
    fn display_name(&self) -> &str {
        "Worktree answer probe"
    }
    fn supports_steering(&self) -> bool {
        false
    }
    fn steering_mode(&self) -> SteeringMode {
        SteeringMode::TurnBoundary
    }
    fn reasoning_levels(&self) -> &[ReasoningLevel] {
        &[]
    }
    async fn models(&self) -> Result<Vec<Model>, HarnessError> {
        Ok(vec![])
    }
    async fn run(
        &self,
        request: RunRequest,
        controls: RunControls,
    ) -> Result<BoxStream<'static, Result<AgentEvent, HarnessError>>, HarnessError> {
        self.requests.send(request).unwrap();
        let ask = self.ask;
        Ok(futures::stream::once(async move {
            if ask {
                (controls.request_input)(vec![question()])
                    .await
                    .expect("live answer delivered");
            }
            Ok(AgentEvent::Done {
                status: DoneStatus::Completed,
                result: None,
                error: None,
                session_id: None,
            })
        })
        .boxed())
    }
}

fn answer(id: &str) -> SessionCommandEntry {
    SessionCommandEntry {
        id: "answer-command".into(),
        issued_at: now_ms(),
        issued_by: "viewer".into(),
        payload: SessionCommandPayload::RespondInput {
            request_id: id.into(),
            answers: vec![zeron_proto::UserInputAnswer {
                question_id: "q".into(),
                labels: vec!["yes".into()],
            }],
        },
        status: SessionCommandStatus::Pending,
        resolution: None,
        based_on: None,
        expires_at: None,
    }
}

fn pending_question(handle: &ChatDocHandle) -> Option<String> {
    handle
        .doc
        .read_entries()
        .unwrap()
        .iter()
        .flat_map(|entry| &entry.parts)
        .find_map(|part| match part {
            MessagePart::Input {
                request_id,
                resolved: false,
                ..
            } => Some(request_id.clone()),
            _ => None,
        })
}

fn fixture(
    ask: bool,
) -> (
    tempfile::TempDir,
    crate::EngineCore,
    tokio::sync::mpsc::UnboundedReceiver<RunRequest>,
) {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let registry = crate::HarnessRegistry::new();
    registry.register(Arc::new(Probe { requests: tx, ask }));
    let core =
        crate::EngineCore::assemble(dir.path(), Arc::new(registry), HarnessId::Mock, None).unwrap();
    core.workspace
        .claim_chat("chat", Some(dir.path().to_str().unwrap()))
        .unwrap();
    (dir, core, rx)
}

fn seed_orphan(core: &crate::EngineCore, handle: &ChatDocHandle) {
    handle
        .doc
        .push_message(&zeron_doc::SessionMessageEntry {
            id: "interrupted-question".into(),
            role: zeron_doc::MessageRole::Assistant,
            parts: vec![MessagePart::Input {
                id: "question-part".into(),
                request_id: "old-question".into(),
                questions: vec![question()],
                resolved: false,
            }],
            created_at: now_ms(),
            device_id: core.device_id.clone(),
            status: Some(MessageStatus::Aborted),
            continuation_of: None,
            duration_ms: None,
        })
        .unwrap();
}

#[tokio::test]
async fn orphaned_answer_waits_for_checkout_publication_without_relocking_commands() {
    let (dir, core, mut requests) = fixture(false);
    let handle = core.doc_host.open("chat").unwrap();
    seed_orphan(&core, &handle);
    // These are the exact locks held while the switch resolves Git and
    // publishes cwd. Poll the answer at that boundary, without timing sleeps.
    let commands = handle.command_drain_lock.lock().await;
    let transition = handle.drain_lock.lock().await;
    let mut reply = Box::pin(
        core.doc_host
            .ingest_relayed_command("chat", answer("old-question")),
    );
    assert!(futures::poll!(&mut reply).is_pending());
    assert_eq!(pending_question(&handle).as_deref(), Some("old-question"));
    assert!(requests.try_recv().is_err());
    let target = dir.path();
    core.workspace
        .set_chat_source_context(
            "chat",
            &ConversationSourceContext {
                checkout_id: "target-checkout".into(),
                repo_root: dir.path().to_string_lossy().into_owned(),
                cwd: target.to_string_lossy().into_owned(),
                branch: "feature".into(),
                head_sha: None,
                observed_at: chrono::Utc::now(),
            },
        )
        .unwrap();
    // A config update while waiting makes a pre-lock request snapshot observable.
    core.workspace
        .set_chat_config(
            "chat",
            &zeron_proto::ChatConfig {
                harness: HarnessId::Mock,
                model: Some("after-checkout".into()),
                reasoning: None,
                model_options: Default::default(),
                sandbox: zeron_proto::SandboxLevel::WorkspaceWrite,
            },
        )
        .unwrap();
    drop(transition);
    // The normal command drain may already own the command lock. The orphan
    // answer must finish even while that lock remains held.
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), reply)
            .await
            .unwrap()
            .unwrap(),
        "executed"
    );
    let request = tokio::time::timeout(Duration::from_secs(5), requests.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(request.cwd, target.to_string_lossy());
    assert_eq!(request.model.as_deref(), Some("after-checkout"));
    assert!(request.prompt.contains("yes"));
    assert!(pending_question(&handle).is_none());
    drop(commands);
    core.shutdown().await;
}

#[tokio::test]
async fn live_answer_bypasses_checkout_and_command_locks() {
    let (_dir, core, mut requests) = fixture(true);
    let handle = core.doc_host.open("chat").unwrap();
    let request = core.doc_host.request_from_chat_row("chat", "ask").unwrap();
    core.sessions
        .dispatch("chat", HarnessId::Mock, request, None)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), requests.recv())
        .await
        .unwrap()
        .unwrap();
    let id = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(id) = pending_question(&handle) {
                break id;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let commands = handle.command_drain_lock.lock().await;
    let transition = handle.drain_lock.lock().await;
    assert_eq!(
        tokio::time::timeout(
            Duration::from_secs(5),
            core.doc_host.ingest_relayed_command("chat", answer(&id))
        )
        .await
        .unwrap()
        .unwrap(),
        "executed"
    );
    drop(transition);
    drop(commands);
    core.shutdown().await;
}

#[tokio::test]
async fn failed_orphan_dispatch_keeps_question_unanswered() {
    let (_dir, core, mut requests) = fixture(false);
    let handle = core.doc_host.open("chat").unwrap();
    seed_orphan(&core, &handle);
    core.workspace
        .set_chat_config(
            "chat",
            &zeron_proto::ChatConfig {
                harness: HarnessId::ClaudeCode,
                model: None,
                reasoning: None,
                model_options: Default::default(),
                sandbox: zeron_proto::SandboxLevel::WorkspaceWrite,
            },
        )
        .unwrap();
    // Only Mock is installed in this fixture; dispatch must fail before a run.
    assert!(
        core.doc_host
            .ingest_relayed_command("chat", answer("old-question"))
            .await
            .is_err()
    );
    assert_eq!(pending_question(&handle).as_deref(), Some("old-question"));
    assert!(requests.try_recv().is_err());
    core.shutdown().await;
}
