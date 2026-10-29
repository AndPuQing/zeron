//! Phone/desktop controls use runtime-owned ids, not a client-supplied PID/path.
use std::sync::{Arc, Mutex};
use async_trait::async_trait;
use futures::{StreamExt, stream::BoxStream};
use zeron_engine::{EngineCore, HarnessRegistry};
use zeron_harness::{Harness, HarnessError, RunControls, TurnControl};
use zeron_proto::{AgentEvent, BackgroundTask, HarnessId, Model, RunRequest, SandboxLevel};
use zeron_rpc::methods;

struct Fixture(Arc<Mutex<Option<TurnControl>>>);
#[async_trait]
impl Harness for Fixture {
    fn id(&self) -> HarnessId { HarnessId::Mock }
    async fn models(&self) -> Result<Vec<Model>, HarnessError> { Ok(vec![]) }
    async fn run(&self, _: RunRequest, controls: RunControls) -> Result<BoxStream<'static, Result<AgentEvent, HarnessError>>, HarnessError> {
        *self.0.lock().unwrap() = Some(controls.turn);
        Ok(futures::stream::pending().boxed())
    }
}

#[tokio::test]
async fn controls_list_read_and_stop_only_the_addressed_runtime_task() {
    let directory = tempfile::tempdir().unwrap();
    let published = Arc::new(Mutex::new(None));
    let registry = HarnessRegistry::new();
    registry.register(Arc::new(Fixture(published.clone())));
    let core = EngineCore::assemble(directory.path(), Arc::new(registry), HarnessId::Mock, None).unwrap();
    let client = zeron_rpc::memory_client(core.rpc_service());
    core.workspace.create_chat("background", None, Some(&core.device_id), None, None).unwrap();
    core.sessions.dispatch("background", HarnessId::Mock, RunRequest {
        prompt: "start".into(), harness: Some(HarnessId::Mock), model: None, reasoning: None,
        model_options: Default::default(), cwd: directory.path().to_string_lossy().into(),
        sandbox: SandboxLevel::WorkspaceWrite, auto_approve: false, access: Default::default(), plan: false,
        resume: None, require_native_resume: false, attachments: vec![], worktree: None, mcp: None, env: Default::default(),
    }, None).await.unwrap();
    let turn = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(turn) = published.lock().unwrap().clone() { break turn; }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let task = |id: &str| BackgroundTask {
        can_stop: true,
        id: id.into(),
        command: format!("watch {id}"),
        description: None,
        started_at: chrono::Utc::now(),
    };
    turn.set_background_processes(vec![task("one"), task("two")]);
    turn.append_background_output("one", "output from one\n");
    let params = serde_json::json!({"chatId":"background", "taskId":"one"});
    let listed = client.call(methods::LIST_BACKGROUND_TASKS, params.clone()).await.unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 2);
    let output = client.call(methods::READ_BACKGROUND_TASK_OUTPUT, params.clone()).await.unwrap();
    assert_eq!(output["text"], "output from one");
    let stopped = client.call(methods::STOP_BACKGROUND_TASK, params.clone()).await.unwrap();
    assert_eq!(stopped["stopping"], true);
    assert_eq!(turn.background_stop_requested().await, "one");
    // A request is not a completion: the driver must confirm it disappeared.
    assert_eq!(client.call(methods::LIST_BACKGROUND_TASKS, params.clone()).await.unwrap().as_array().unwrap().len(), 2);
    turn.set_background_processes(vec![task("two")]);
    assert!(client.call(methods::READ_BACKGROUND_TASK_OUTPUT, params.clone()).await.unwrap().is_null());
    assert_eq!(client.call(methods::STOP_BACKGROUND_TASK, params).await.unwrap()["stopping"], false);
    let listed = client.call(methods::LIST_BACKGROUND_TASKS, serde_json::json!({"chatId":"background"})).await.unwrap();
    assert_eq!(listed[0]["id"], "two");
    core.shutdown().await;
    assert!(core.sessions.background_tasks("background").is_empty());
}
