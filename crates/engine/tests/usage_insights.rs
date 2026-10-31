//! `GetUsageInsights` end to end. Two chats run mock-harness turns queued
//! over the RPC transport, then the report counts them per harness and task
//! type. Covers the time filter, chat deletion, a restart that serves the
//! cached rows, the advertised capability, and null params.

use std::sync::Arc;
use std::time::Duration;

use zeron_doc::{MessageRole, MessageStatus, SessionCommandPayload};
use zeron_engine::{EngineCore, HarnessRegistry};
use zeron_harness::mock::MockHarness;
use zeron_proto::usage_insights::{TaskCategory, UsageInsightsReport};
use zeron_proto::{AgentEvent, DoneStatus, HarnessId, RunRequest, SandboxLevel};
use zeron_rpc::{RpcClient, methods};

const BUG: &str = "chat-bug";
const DOCS: &str = "chat-docs";

fn assemble(dir: &std::path::Path) -> EngineCore {
    let registry = HarnessRegistry::new();
    registry.register(Arc::new(MockHarness {
        script: vec![
            AgentEvent::TextDelta {
                text: "Done.".into(),
            },
            AgentEvent::Done {
                status: DoneStatus::Completed,
                result: None,
                error: None,
                session_id: None,
            },
        ],
    }));
    EngineCore::assemble(dir, Arc::new(registry), HarnessId::Mock, None).expect("engine assembles")
}

async fn create_chat(client: &RpcClient, core: &EngineCore, chat: &str) {
    client
        .call(
            methods::MUTATE,
            serde_json::json!({ "op": "createChat", "chatId": chat, "deviceId": core.device_id }),
        )
        .await
        .expect("createChat");
    core.workspace
        .rename_chat(chat, "Untitled")
        .expect("pre-title keeps the titler out");
}

async fn run_turn(client: &RpcClient, chat: &str, prompt: &str) {
    let command = serde_json::to_value(SessionCommandPayload::Run {
        request: RunRequest {
            access: Default::default(),
            plan: false,
            require_native_resume: false,
            prompt: prompt.into(),
            harness: None,
            model: None,
            reasoning: None,
            model_options: Default::default(),
            cwd: "/tmp".into(),
            sandbox: SandboxLevel::WorkspaceWrite,
            auto_approve: true,
            attachments: Vec::new(),
            worktree: None,
            resume: None,
            mcp: None,
            env: Default::default(),
        },
        message_id: format!("{chat}-m1"),
    })
    .unwrap();
    client
        .call(
            methods::QUEUE_COMMAND,
            serde_json::json!({ "chatId": chat, "command": command }),
        )
        .await
        .expect("QueueCommand");
}

fn turn_complete(core: &EngineCore, chat: &str) -> bool {
    core.doc_host
        .open(chat)
        .ok()
        .and_then(|h| h.doc().read_entries().ok())
        .unwrap_or_default()
        .iter()
        .any(|e| e.role == MessageRole::Assistant && e.status == Some(MessageStatus::Complete))
}

async fn report(client: &RpcClient, params: serde_json::Value) -> UsageInsightsReport {
    let reply = client
        .call(methods::GET_USAGE_INSIGHTS, params)
        .await
        .expect("GetUsageInsights");
    serde_json::from_value(reply).expect("report shape")
}

/// The first finished report that counts `sessions` sessions.
async fn eventually(client: &RpcClient, sessions: u32) -> UsageInsightsReport {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let report = report(client, serde_json::json!({})).await;
        if !report.indexing && report.total.sessions == sessions {
            return report;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "wanted {sessions} sessions, still {report:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn usage_insights_remain_available_when_the_disk_cache_cannot_open() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let profile = zeron_engine::profile::EngineProfile::local(&data).unwrap();
    std::fs::create_dir_all(profile.store_root().join("usage-insights.sqlite3")).unwrap();

    let core = assemble(&data);
    let client = zeron_rpc::memory_client(core.rpc_service());
    let info: zeron_proto::EngineInfo = serde_json::from_value(
        client
            .call(methods::ENGINE_INFO, serde_json::Value::Null)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(info.supports(zeron_proto::capabilities::USAGE_INSIGHTS_V1));
    create_chat(&client, &core, BUG).await;
    run_turn(&client, BUG, "Fix the login crash").await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while !turn_complete(&core, BUG) {
        assert!(tokio::time::Instant::now() < deadline, "turn completes");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let usage = eventually(&client, 1).await;
    assert_eq!(usage.total.turns, 1);
    assert_eq!(usage.total.completed, 1);
    core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn usage_insights_count_turns_per_harness_and_task_type() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");

    let core = assemble(&data);
    let client = zeron_rpc::memory_client(core.rpc_service());
    let info: zeron_proto::EngineInfo = serde_json::from_value(
        client
            .call(methods::ENGINE_INFO, serde_json::json!({}))
            .await
            .expect("EngineInfo"),
    )
    .unwrap();
    assert!(
        info.supports(zeron_proto::capabilities::USAGE_INSIGHTS_V1),
        "the engine advertises the report"
    );
    create_chat(&client, &core, BUG).await;
    create_chat(&client, &core, DOCS).await;
    run_turn(&client, BUG, "Fix the crash when the queue is empty").await;
    run_turn(
        &client,
        DOCS,
        "Update the README for the new installer docs",
    )
    .await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while !(turn_complete(&core, BUG) && turn_complete(&core, DOCS)) {
        assert!(tokio::time::Instant::now() < deadline, "turns complete");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let both = eventually(&client, 2).await;
    assert_eq!(both.total.turns, 2);
    assert_eq!(both.total.completed, 2);
    assert_eq!(both.harnesses.len(), 1);
    assert_eq!(both.harnesses[0].harness, HarnessId::Mock.as_str());
    let mut categories: Vec<TaskCategory> = both.categories.iter().map(|c| c.category).collect();
    categories.sort_by_key(|c| TaskCategory::ALL.iter().position(|x| x == c));
    assert_eq!(categories, [TaskCategory::Bugfix, TaskCategory::Docs]);
    assert!(both.generated_at > 0);

    let future = both.generated_at + 60_000;
    let none = report(&client, serde_json::json!({ "sinceMs": future })).await;
    assert_eq!(none.total.turns, 0);
    assert_eq!(none.since_ms, Some(future));
    assert!(none.categories.is_empty());
    let null_params = report(&client, serde_json::Value::Null).await;
    assert_eq!(null_params.total.sessions, 2);

    client
        .call(
            methods::MUTATE,
            serde_json::json!({ "op": "deleteChat", "chatId": DOCS }),
        )
        .await
        .expect("deleteChat");
    let after_delete = eventually(&client, 1).await;
    assert_eq!(after_delete.categories.len(), 1);
    assert_eq!(after_delete.categories[0].category, TaskCategory::Bugfix);

    core.shutdown().await;
    drop(client);
    drop(core);

    // Mark the cached row (its one turn, twice) so the restarted engine can
    // only report it by serving the cache, not by re-reading the transcript.
    let cache =
        find(&data, "usage-insights.sqlite3").expect("the cache lives in the profile store");
    {
        let conn = rusqlite::Connection::open(&cache).unwrap();
        let usage: String = conn
            .query_row("SELECT usage FROM chats WHERE chat_id = ?1", [BUG], |row| {
                row.get(0)
            })
            .unwrap();
        let mut usage: serde_json::Value = serde_json::from_str(&usage).unwrap();
        let turn = usage["turns"][0].clone();
        usage["turns"].as_array_mut().unwrap().push(turn);
        conn.execute(
            "UPDATE chats SET usage = ?1 WHERE chat_id = ?2",
            rusqlite::params![usage.to_string(), BUG],
        )
        .unwrap();
    }

    let core = assemble(&data);
    let client = zeron_rpc::memory_client(core.rpc_service());
    let restarted = eventually(&client, 1).await;
    assert_eq!(
        restarted.total.turns, 2,
        "the restarted engine serves the cached row instead of re-reading the transcript"
    );
    core.shutdown().await;
}

fn find(dir: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .find_map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                find(&path, name)
            } else {
                path.file_name().is_some_and(|n| n == name).then_some(path)
            }
        })
}
