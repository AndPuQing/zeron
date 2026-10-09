use serde_json::{Value, json};
use zeron_harness::saved_sessions::{SavedHistoryPart, SavedMessageRole};
use zeron_harness::{ClaudeHarness, Harness};

const SOURCE: &str = "77777777-1111-2222-3333-444444444444";
const USER: &str = "11111111-1111-2222-3333-444444444444";
const ASSISTANT: &str = "22222222-1111-2222-3333-444444444444";

fn claude_fixture(records: Vec<Value>) -> (tempfile::TempDir, ClaudeHarness, std::path::PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("projects").join("encoded-unicode-project");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.join(format!("{SOURCE}.jsonl"));
    let data = records
        .into_iter()
        .map(|record| format!("{record}\n"))
        .collect::<String>();
    std::fs::write(&path, data).unwrap();
    let harness = ClaudeHarness::new().with_saved_session_home(root.path());
    (root, harness, path)
}

fn user(parent: Option<&str>, id: &str, content: Value) -> Value {
    json!({"type":"user","uuid":id,"parentUuid":parent,"sessionId":SOURCE,"cwd":"/项目/a worktree",
        "timestamp":"2026-10-01T12:00:00Z","message":{"content":content}})
}
fn assistant(parent: &str, id: &str, content: Value, stop: &str) -> Value {
    json!({"type":"assistant","uuid":id,"parentUuid":parent,"sessionId":SOURCE,"cwd":"/项目/a worktree",
        "timestamp":"2026-10-01T12:00:01Z","message":{"model":"claude-sonnet-4-6","content":content,"stop_reason":stop}})
}

#[tokio::test]
async fn claude_offline_copy_preserves_context_and_remaps_native_identity_without_writing_source() {
    let progress = "33333333-1111-2222-3333-444444444444";
    let (root, harness, path) = claude_fixture(vec![
        user(None, USER, json!("remember turquoise")),
        json!({"type":"progress","uuid":progress,"parentUuid":USER,"sessionId":SOURCE}),
        assistant(
            progress,
            ASSISTANT,
            json!([{"type":"thinking","thinking":"reasoning"},{"type":"text","text":"remembered"}]),
            "end_turn",
        ),
        json!({"type":"content-replacement","sessionId":SOURCE,"replacements":[{"original":"old","replacement":"new"}]}),
        json!({"type":"atis-latch","sessionId":SOURCE,"atis":"native-state"}),
    ]);
    let original = std::fs::read(&path).unwrap();
    let page = harness.saved_sessions(None).await.unwrap();
    assert_eq!(page.sessions.len(), 1);
    let source = &page.sessions[0];
    assert_eq!(source.cwd, "/项目/a worktree");
    let copy = harness.copy_saved_session(source).await.unwrap();
    assert_ne!(copy.native_id, SOURCE);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(
        std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
        2
    );
    let records = std::fs::read_to_string(copy.locator.as_ref().unwrap())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(!records.iter().any(|r| r["type"] == "progress"));
    assert!(records.iter().all(|r| r["sessionId"] == copy.native_id));
    let u = records.iter().find(|r| r["type"] == "user").unwrap();
    let a = records.iter().find(|r| r["type"] == "assistant").unwrap();
    assert_ne!(u["uuid"], USER);
    assert_ne!(a["uuid"], ASSISTANT);
    assert_eq!(a["parentUuid"], u["uuid"]);
    assert_eq!(a["forkedFrom"]["sessionId"], SOURCE);
    assert!(records.iter().any(|r| r["atis"] == "native-state"));
    assert!(records.iter().any(|r| r["type"] == "content-replacement"));
    let history = harness.saved_session_history(&copy).await.unwrap();
    assert_eq!(history.messages.len(), 2);
    assert!(
        matches!(&history.messages[0].parts[0], SavedHistoryPart::Text(s) if s == "remember turquoise")
    );
    assert!(
        matches!(&history.messages[1].parts[0], SavedHistoryPart::Reasoning(s) if s == "reasoning")
    );
    drop(root);
}

#[tokio::test]
async fn claude_effective_chain_drops_rewound_and_subagent_messages_and_pairs_tools() {
    let abandoned = "33333333-1111-2222-3333-444444444444";
    let result = "44444444-1111-2222-3333-444444444444";
    let final_id = "55555555-1111-2222-3333-444444444444";
    let mut child = assistant(USER, abandoned, json!("subagent"), "end_turn");
    child["isSidechain"] = true.into();
    let (_root, harness, _path) = claude_fixture(vec![
        user(None, USER, json!("request")),
        assistant(USER, abandoned, json!("abandoned answer"), "end_turn"),
        assistant(
            USER,
            ASSISTANT,
            json!([{"type":"tool_use","id":"tool-1","name":"Bash","input":{"command":"pwd"}}]),
            "tool_use",
        ),
        child,
        user(
            Some(ASSISTANT),
            result,
            json!([{"type":"tool_result","tool_use_id":"tool-1","content":"/project"}]),
        ),
        assistant(
            result,
            final_id,
            json!([{"type":"text","text":"done"}]),
            "end_turn",
        ),
    ]);
    let source = harness
        .saved_sessions(None)
        .await
        .unwrap()
        .sessions
        .remove(0);
    let history = harness.saved_session_history(&source).await.unwrap();
    assert_eq!(history.messages.len(), 3);
    assert!(
        matches!(&history.messages[1].parts[0],SavedHistoryPart::Tool {output:Some(text),..} if text == "/project")
    );
    assert!(!format!("{history:?}").contains("abandoned"));
    assert!(!format!("{history:?}").contains("subagent"));
}

#[tokio::test]
async fn claude_compaction_keeps_summary_and_reports_history_boundary() {
    let boundary = "33333333-1111-2222-3333-444444444444";
    let mut summary = user(Some(boundary), USER, json!("compact summary"));
    summary["isCompactSummary"] = true.into();
    let (_root, harness, _path) = claude_fixture(vec![
        json!({"type":"system","subtype":"compact_boundary","uuid":boundary,"parentUuid":null,"sessionId":SOURCE,"cwd":"/项目/a worktree"}),
        summary,
        assistant(
            USER,
            ASSISTANT,
            json!([{"type":"image","source":{"data":"secret-base64"}},{"type":"text","text":"continued"}]),
            "end_turn",
        ),
    ]);
    let source = harness
        .saved_sessions(None)
        .await
        .unwrap()
        .sessions
        .remove(0);
    let history = harness.saved_session_history(&source).await.unwrap();
    assert!(history.notice.as_deref().unwrap().contains("compacted"));
    assert!(history.notice.as_deref().unwrap().contains("attachments"));
    assert!(matches!(history.messages[1].role, SavedMessageRole::System));
    assert!(!format!("{history:?}").contains("secret-base64"));
}

#[tokio::test]
async fn claude_large_transcript_metadata_keeps_complete_tail_records() {
    let (_root, harness, _path) = claude_fixture(vec![
        user(None, USER, json!("initial prompt")),
        json!({"type":"native-extra","data":"z".repeat(400_000)}),
        assistant(USER, ASSISTANT, json!("latest answer"), "end_turn"),
    ]);
    let page = harness.saved_sessions(None).await.unwrap();
    assert_eq!(page.sessions.len(), 1);
    let source = &page.sessions[0];
    assert_eq!(source.model.as_deref(), Some("claude-sonnet-4-6"));
    assert_eq!(source.updated_at_ms - source.created_at_ms, 1_000);
}

#[tokio::test]
async fn claude_rejects_busy_torn_records_changed_root_and_oversized_sources() {
    let (_root, harness, path) = claude_fixture(vec![user(None, USER, json!("unfinished prompt"))]);
    let source = harness
        .saved_sessions(None)
        .await
        .unwrap()
        .sessions
        .remove(0);
    assert!(
        harness
            .copy_saved_session(&source)
            .await
            .unwrap_err()
            .to_string()
            .contains("Finish or stop")
    );
    assert_eq!(
        std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
        1
    );
    let mut forged = source.clone();
    forged.store_id = "other-root".into();
    assert!(harness.copy_saved_session(&forged).await.is_err());
    std::fs::write(&path, b"{\"type\":\"user\"").unwrap();
    assert!(
        harness
            .saved_session_history(&source)
            .await
            .unwrap_err()
            .to_string()
            .contains("incomplete")
    );
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_len(zeron_harness::saved_sessions::MAX_SOURCE_BYTES + 1)
        .unwrap();
    assert!(
        harness
            .saved_session_history(&source)
            .await
            .unwrap_err()
            .to_string()
            .contains("limit")
    );
}

#[tokio::test]
#[ignore = "set ZERON_IMPORT_CLAUDE_SDK to an installed SDK module; offline, isolated synthetic store"]
async fn rust_claude_copy_is_readable_by_the_official_sdk() {
    let module = std::env::var("ZERON_IMPORT_CLAUDE_SDK").expect("explicit SDK module path");
    let (root, harness, path) = claude_fixture(vec![
        user(None, USER, json!("remember turquoise")),
        assistant(
            USER,
            ASSISTANT,
            json!([{"type":"text","text":"remembered turquoise"}]),
            "end_turn",
        ),
    ]);
    let original = std::fs::read(&path).unwrap();
    let source = harness
        .saved_sessions(None)
        .await
        .unwrap()
        .sessions
        .remove(0);
    let copy = harness.copy_saved_session(&source).await.unwrap();
    let script = "const sdk=await import(process.argv[1]); const messages=await sdk.getSessionMessages(process.argv[2]); console.log(JSON.stringify(messages));";
    let result = std::process::Command::new("node")
        .args([
            "--input-type=module",
            "-e",
            script,
            &module,
            &copy.native_id,
        ])
        .env("CLAUDE_CONFIG_DIR", root.path())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let messages: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(messages.as_array().unwrap().len(), 2);
    assert!(messages.to_string().contains("remembered turquoise"));
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[cfg(unix)]
#[tokio::test]
async fn claude_strict_resume_rejects_missing_copies_and_a_different_native_id() {
    use futures::StreamExt;
    use zeron_harness::{CancellationToken, RunControls};
    use zeron_proto::{RunRequest, SandboxLevel};

    let (root, harness, path) = claude_fixture(vec![
        user(None, USER, json!("remember turquoise")),
        assistant(USER, ASSISTANT, json!("remembered"), "end_turn"),
    ]);
    let cwd = root.path().to_string_lossy().into_owned();
    let records = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| {
            let mut record: Value = serde_json::from_str(line).unwrap();
            record["cwd"] = cwd.clone().into();
            format!("{record}\n")
        })
        .collect::<String>();
    std::fs::write(&path, records).unwrap();
    let original = std::fs::read(&path).unwrap();
    let harness = harness.with_executable(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh"),
    );
    let source = harness
        .saved_sessions(None)
        .await
        .unwrap()
        .sessions
        .remove(0);
    let copy = harness.copy_saved_session(&source).await.unwrap();
    let controls = || {
        let (steer, rx) = tokio::sync::mpsc::channel(1);
        (
            steer,
            RunControls {
                turn: Default::default(),
                realtime: None,
                execution_lease: None,
                request_input: Box::new(|_| tokio::sync::oneshot::channel().1),
                steering: rx,
                interrupt: CancellationToken::new(),
            },
        )
    };
    let mut request = RunRequest {
        prompt: "scenario:happy".into(),
        cwd,
        harness: None,
        model: None,
        reasoning: None,
        model_options: Default::default(),
        sandbox: SandboxLevel::WorkspaceWrite,
        auto_approve: true,
        attachments: Vec::new(),
        resume: Some(uuid::Uuid::new_v4().to_string()),
        require_native_resume: true,
        worktree: None,
        mcp: None,
    };
    let (_missing_mailbox, missing_controls) = controls();
    match harness.run(request.clone(), missing_controls).await {
        Err(error) => assert!(error.to_string().contains("missing")),
        Ok(_) => panic!("missing copy must fail before starting the CLI"),
    }
    request.resume = Some(copy.native_id.clone());
    // Keep the live mailbox open while checking identity, as the engine does.
    let (_live_mailbox, run_controls) = controls();
    let mut events = harness.run(request, run_controls).await.unwrap();
    let first = tokio::time::timeout(std::time::Duration::from_secs(10), events.next())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(first, Err(error) if error.to_string().contains("different session")));
    assert!(copy.locator.unwrap().exists());
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[cfg(unix)]
mod codex {
    use super::*;
    use futures::StreamExt;
    use std::os::unix::fs::PermissionsExt;
    use zeron_harness::{CancellationToken, CodexHarness, RunControls};
    use zeron_proto::{RunRequest, SandboxLevel};

    #[tokio::test]
    #[ignore = "requires an installed Codex CLI; uses only an isolated synthetic store, no model request"]
    async fn installed_codex_can_copy_and_reopen_synthetic_native_history() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project worktree");
        std::fs::create_dir(&project).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let turn = uuid::Uuid::new_v4().to_string();
        let records = vec![
            json!({"type":"session_meta","payload":{"id":id,"timestamp":"2026-10-01T12:00:00Z","cwd":project,"originator":"codex_cli_rs","cli_version":"0.160.1","source":"cli","model_provider":"openai"}}),
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":turn,"model_context_window":258400}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"remember turquoise","images":[],"local_images":[],"text_elements":[]}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"remember turquoise"}]}}),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"remembered turquoise"}]}}),
            json!({"type":"event_msg","payload":{"type":"agent_message","message":"remembered turquoise"}}),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":turn,"last_agent_message":"remembered turquoise"}}),
        ];
        let directory = root.path().join("sessions/2026/10/01");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("rollout-2026-10-01T12-00-00-{id}.jsonl"));
        let original = records
            .into_iter()
            .map(|mut r| {
                r["timestamp"] = "2026-10-01T12:00:00Z".into();
                format!("{r}\n")
            })
            .collect::<String>();
        std::fs::write(&path, &original).unwrap();
        let harness = CodexHarness::new().with_saved_session_home(root.path());
        let list = harness.saved_sessions(None).await.unwrap();
        let source = list.sessions.iter().find(|s| s.native_id == id).unwrap();
        let copy = harness.copy_saved_session(source).await.unwrap();
        assert_ne!(copy.native_id, id);
        let history = harness.saved_session_history(&copy).await.unwrap();
        assert_eq!(history.messages.len(), 2);
        assert!(format!("{history:?}").contains("remembered turquoise"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }

    fn fixture(options: Value) -> (tempfile::TempDir, CodexHarness) {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("saved-codex.py");
        std::fs::write(&executable, include_bytes!("fixtures/saved-codex.py")).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut state = json!({"thread":{"id":"original-native-id","cwd":root.path(),"name":"Original","createdAt":1,"updatedAt":6},
            "turns":(0..6).map(|i| json!({"id":format!("turn-{i}"),"status":"completed","startedAt":i+1,"itemsView":"full",
                "items":[{"type":"userMessage","id":format!("user-{i}"),"content":[{"type":"text","text":format!("prompt-{i}")}]},
                {"type":"agentMessage","id":format!("answer-{i}"),"text":format!("answer-{i}")}]})).collect::<Vec<_>>()});
        for (key, value) in options.as_object().unwrap() {
            state[key] = value.clone();
        }
        std::fs::write(root.path().join("fixture.json"), state.to_string()).unwrap();
        let harness = CodexHarness::new()
            .with_executable(executable)
            .with_saved_session_home(root.path())
            .with_graces(
                std::time::Duration::from_millis(20),
                std::time::Duration::from_millis(100),
            );
        (root, harness)
    }

    #[tokio::test]
    async fn codex_forks_at_completed_boundary_and_reads_every_history_page() {
        let (root, harness) = fixture(json!({}));
        let original = std::fs::read(root.path().join("fixture.json")).unwrap();
        let page = harness.saved_sessions(None).await.unwrap();
        assert!(page.next_cursor.is_some());
        assert!(
            harness
                .saved_sessions(page.next_cursor.as_deref())
                .await
                .unwrap()
                .next_cursor
                .is_none()
        );
        let source = &page.sessions[0];
        let copy = harness.copy_saved_session(source).await.unwrap();
        assert_eq!(copy.native_id, "copy-native-id");
        assert_ne!(copy.native_id, source.native_id);
        let history = harness.saved_session_history(&copy).await.unwrap();
        assert_eq!(history.messages.len(), 12);
        assert_eq!(history.messages[10].timestamp_ms, 6000);
        assert_eq!(
            std::fs::read(root.path().join("fixture.json")).unwrap(),
            original
        );
        let calls = std::fs::read_to_string(root.path().join("calls.jsonl")).unwrap();
        assert!(calls.contains("lastTurnId"));
        assert!(!calls.contains("turn/start"));
    }

    #[tokio::test]
    async fn codex_cursor_loops_unsupported_versions_and_oversized_frames_fail_explicitly() {
        for (options, expected) in [
            (json!({"loop":true}), "cursor"),
            (json!({"unsupported":true}), "Update Codex"),
            (json!({"oversized":true}), "limit"),
        ] {
            let (_root, harness) = fixture(options);
            let result = harness.saved_sessions(None).await;
            let error = if let Ok(page) = result {
                harness
                    .saved_session_history(&page.sessions[0])
                    .await
                    .unwrap_err()
                    .to_string()
            } else {
                result.unwrap_err().to_string()
            };
            assert!(error.contains(expected), "{error}");
        }
    }

    #[tokio::test]
    async fn codex_strict_resume_never_starts_fresh_or_accepts_a_different_id() {
        for options in [json!({"resumeError":true}), json!({"wrongResume":true})] {
            let (root, harness) = fixture(options);
            let (_steer, rx) = tokio::sync::mpsc::channel(1);
            let controls = RunControls {
                turn: Default::default(),
                realtime: None,
                execution_lease: None,
                request_input: Box::new(|_| tokio::sync::oneshot::channel().1),
                steering: rx,
                interrupt: CancellationToken::new(),
            };
            let request = RunRequest {
                prompt: "continue".into(),
                cwd: root.path().to_string_lossy().into_owned(),
                harness: None,
                model: None,
                reasoning: None,
                model_options: Default::default(),
                sandbox: SandboxLevel::WorkspaceWrite,
                auto_approve: true,
                attachments: vec![],
                resume: Some("copy-native-id".into()),
                require_native_resume: true,
                worktree: None,
                mcp: None,
            };
            let mut events = harness.run(request, controls).await.unwrap();
            let first = tokio::time::timeout(std::time::Duration::from_secs(5), events.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert!(matches!(
                first,
                zeron_proto::AgentEvent::Done {
                    status: zeron_proto::DoneStatus::Errored,
                    ..
                }
            ));
            let calls = std::fs::read_to_string(root.path().join("calls.jsonl")).unwrap();
            assert!(!calls.contains("thread/start"));
            assert!(!calls.contains("turn/start"));
        }
    }
}
