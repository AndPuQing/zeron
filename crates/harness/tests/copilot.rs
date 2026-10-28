//! Copilot's native ACP contract; no account or network service required.
#![cfg(unix)]

use futures::{StreamExt, stream::BoxStream};
use std::{path::PathBuf, time::Duration};
use tokio::sync::{mpsc, oneshot};
use zeron_harness::{
    AcpHarness, CancellationToken, Harness, HarnessError, RunControls, SteerMessage,
};
use zeron_proto::{
    AgentEvent, DoneStatus, HarnessId, ReasoningLevel, RunRequest, SandboxLevel, SteeringMode,
    ToolCall,
};

fn harness() -> AcpHarness {
    AcpHarness::copilot().with_executable(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/copilot-acp.py"),
    )
}
fn request(cwd: &std::path::Path, prompt: &str) -> RunRequest {
    RunRequest {
        access: Default::default(),
        plan: false,
        prompt: prompt.into(),
        harness: Some(HarnessId::Copilot),
        model: Some("gpt-4.1".into()),
        reasoning: Some(ReasoningLevel::High),
        model_options: Default::default(),
        cwd: cwd.display().to_string(),
        sandbox: SandboxLevel::WorkspaceWrite,
        auto_approve: true,
        resume: None,
        require_native_resume: false,
        attachments: vec![],
        worktree: None,
        mcp: None,
        env: Default::default(),
    }
}
fn run_controls() -> (RunControls, mpsc::Sender<SteerMessage>, CancellationToken) {
    let (steer, steering) = mpsc::channel(8);
    let interrupt = CancellationToken::new();
    (
        RunControls {
            realtime: None,
            execution_lease: None,
            steering,
            interrupt: interrupt.clone(),
            turn: Default::default(),
            request_input: Box::new(|_| {
                let (tx, rx) = oneshot::channel();
                let _ = tx.send(vec![]);
                rx
            }),
        },
        steer,
        interrupt,
    )
}
async fn next(stream: &mut BoxStream<'static, Result<AgentEvent, HarnessError>>) -> AgentEvent {
    tokio::time::timeout(Duration::from_secs(10), stream.next())
        .await
        .expect("event timeout")
        .expect("stream ended early")
        .expect("event")
}
async fn until_done(
    stream: &mut BoxStream<'static, Result<AgentEvent, HarnessError>>,
) -> Vec<AgentEvent> {
    let mut events = vec![];
    loop {
        let event = next(stream).await;
        let done = matches!(event, AgentEvent::Done { .. });
        events.push(event);
        if done {
            return events;
        }
    }
}
fn text(events: &[AgentEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}
fn done(events: &[AgentEvent], expected: DoneStatus) -> String {
    match events.last().unwrap() {
        AgentEvent::Done {
            status,
            session_id,
            error,
            ..
        } => {
            assert_eq!(*status, expected, "{error:?}");
            session_id.clone().unwrap_or_default()
        }
        event => panic!("not done: {event:?}"),
    }
}

#[test]
fn copilot_descriptor_and_wire_identity() {
    let harness = AcpHarness::copilot();
    assert_eq!(harness.id(), HarnessId::Copilot);
    assert_eq!(harness.display_name(), "GitHub Copilot");
    assert_eq!(harness.steering_mode(), SteeringMode::TurnBoundary);
    assert!(harness.stops_turn_in_place());
    assert!(harness.reasoning_levels().is_empty());
    assert_eq!(serde_json::to_string(&harness.id()).unwrap(), "\"copilot\"");
}

#[tokio::test]
async fn copilot_live_models_deduplicate_auto_and_use_advertised_effort() {
    let models = harness().models().await.unwrap();
    assert_eq!(
        models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        ["auto", "gpt-4.1"]
    );
    assert_eq!(
        models[1].reasoning_levels,
        [ReasoningLevel::Low, ReasoningLevel::High]
    );
    assert!(
        models.iter().all(|m| m.options.is_empty()),
        "permissions must not appear as model traits"
    );
}

#[tokio::test]
async fn copilot_streams_tools_and_images_with_selected_model() {
    let workspace = tempfile::tempdir().unwrap();
    let image = workspace.path().join("pixel.png");
    use base64::Engine;
    std::fs::write(&image, base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jkXcAAAAASUVORK5CYII=").unwrap()).unwrap();
    let mut req = request(workspace.path(), "tool");
    req.attachments.push(image.display().to_string());
    let (controls, _steer, _) = run_controls();
    let mut stream = harness().run(req, controls).await.unwrap();
    let events = until_done(&mut stream).await;
    assert!(
        text(&events).contains("model=gpt-4.1;effort=high;images=1"),
        "{events:?}"
    );
    assert!(events.iter().any(|event| matches!(event, AgentEvent::ToolCall { call: ToolCall::Exec { command }, .. } if command == "printf 'copilot-ok'")), "{events:?}");
    assert!(events.iter().any(|event| matches!(event, AgentEvent::ToolResult { is_error: false, output: Some(output), .. } if output == "copilot-ok")), "{events:?}");
    assert_eq!(done(&events, DoneStatus::Completed), "copilot-session");
}

#[tokio::test]
async fn copilot_resumes_without_replaying_history() {
    let workspace = tempfile::tempdir().unwrap();
    let mut req = request(workspace.path(), "fresh reply");
    req.resume = Some("saved-copilot-session".into());
    req.require_native_resume = true;
    let (controls, _steer, _) = run_controls();
    let mut stream = harness().run(req, controls).await.unwrap();
    let events = until_done(&mut stream).await;
    assert!(!text(&events).contains("OLD HISTORY"));
    assert_eq!(
        done(&events, DoneStatus::Completed),
        "saved-copilot-session"
    );
}

#[tokio::test]
async fn copilot_failed_required_resume_and_auth_errors_are_actionable() {
    let workspace = tempfile::tempdir().unwrap();
    for (sid, expected) in [
        ("missing", "could not restore required session"),
        ("signed-out", "copilot login"),
    ] {
        let mut req = request(workspace.path(), "must not run");
        req.resume = Some(sid.into());
        req.require_native_resume = true;
        let (controls, _steer, _) = run_controls();
        let mut stream = harness().run(req, controls).await.unwrap();
        let events = until_done(&mut stream).await;
        assert!(text(&events).is_empty());
        assert!(
            matches!(events.last(), Some(AgentEvent::Done { status: DoneStatus::Errored, error: Some(error), .. }) if error.contains(expected)),
            "{events:?}"
        );
    }
}

#[tokio::test]
async fn copilot_queues_followups_at_turn_boundaries() {
    let workspace = tempfile::tempdir().unwrap();
    let (controls, steer, _) = run_controls();
    steer.send(SteerMessage::text("second")).await.unwrap();
    steer.send(SteerMessage::text("third")).await.unwrap();
    let mut stream = harness()
        .run(request(workspace.path(), "first"), controls)
        .await
        .unwrap();
    let mut replies = String::new();
    for _ in 0..3 {
        let events = until_done(&mut stream).await;
        replies.push_str(&text(&events));
        assert_eq!(done(&events, DoneStatus::Completed), "copilot-session");
        if replies.contains("third") {
            break;
        }
    }
    assert!(
        replies.contains("first") && replies.contains("second") && replies.contains("third"),
        "{replies}"
    );
    assert!(replies.find("first").unwrap() < replies.find("second").unwrap());
    assert!(replies.find("second").unwrap() < replies.find("third").unwrap());
    drop(steer);
    assert!(
        tokio::time::timeout(Duration::from_secs(10), stream.next())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn copilot_stop_keeps_session_for_next_prompt_and_interrupt_closes_it() {
    let workspace = tempfile::tempdir().unwrap();
    let (controls, steer, interrupt) = run_controls();
    let turn = controls.turn.clone();
    let mut stream = harness()
        .run(request(workspace.path(), "wait"), controls)
        .await
        .unwrap();
    loop {
        if matches!(next(&mut stream).await, AgentEvent::TextDelta { .. }) {
            break;
        }
    }
    turn.stop_turn();
    assert_eq!(
        done(&until_done(&mut stream).await, DoneStatus::Interrupted),
        "copilot-session"
    );
    steer.send(SteerMessage::text("after stop")).await.unwrap();
    let events = until_done(&mut stream).await;
    assert!(text(&events).starts_with("after stop"));
    assert_eq!(done(&events, DoneStatus::Completed), "copilot-session");
    steer.send(SteerMessage::text("wait")).await.unwrap();
    loop {
        if matches!(next(&mut stream).await, AgentEvent::TextDelta { .. }) {
            break;
        }
    }
    interrupt.cancel();
    assert_eq!(
        done(&until_done(&mut stream).await, DoneStatus::Interrupted),
        "copilot-session"
    );
    assert!(
        tokio::time::timeout(Duration::from_secs(10), stream.next())
            .await
            .unwrap()
            .is_none()
    );
}

/// Uses the installed CLI and a local model peer. No GitHub login, credentials
/// or paid model calls. Run explicitly with `--ignored` after installing Copilot.
#[tokio::test]
#[ignore = "requires an installed Copilot CLI with native ACP support and Python 3"]
async fn copilot_native_cli_local_model_lifecycle() {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;
    use tokio::io::{AsyncBufReadExt, BufReader};
    use zeron_harness::environment::{EnvironmentEntry, EnvironmentSnapshot};

    let cli = AcpHarness::copilot()
        .executable_path()
        .expect("install Copilot CLI first");
    let workspace = tempfile::tempdir().unwrap();
    let provider =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/copilot-model.py");
    let mut model = tokio::process::Command::new("python3")
        .arg(provider)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(model.stdout.take().unwrap()).lines();
    let port = stdout
        .next_line()
        .await
        .unwrap()
        .unwrap()
        .parse::<u16>()
        .unwrap();
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
    let wrapper = workspace.path().join("copilot-isolated");
    std::fs::write(&wrapper, format!(
        "#!/bin/sh\nexport COPILOT_HOME={}\nexec {} --no-auto-update --disable-builtin-mcps --log-level none \"$@\"\n",
        quote(workspace.path().join("state").to_str().unwrap()), quote(cli.to_str().unwrap()),
    )).unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut environment = EnvironmentSnapshot::default();
    for (name, _) in std::env::vars() {
        if name.starts_with("COPILOT_") || matches!(name.as_str(), "GH_TOKEN" | "GITHUB_TOKEN") {
            environment.entries.insert(name, EnvironmentEntry::Unset);
        }
    }
    for (name, value) in [
        (
            "COPILOT_PROVIDER_BASE_URL",
            format!("http://127.0.0.1:{port}/v1"),
        ),
        ("COPILOT_PROVIDER_TYPE", "openai".into()),
        ("COPILOT_OFFLINE", "true".into()),
        ("COPILOT_MODEL", "gpt-4.1".into()),
    ] {
        environment
            .entries
            .insert(name.into(), EnvironmentEntry::Set { value });
    }
    let harness = AcpHarness::copilot()
        .with_executable(wrapper)
        .with_environment(Arc::new(environment));
    let mut req = request(workspace.path(), "native-tool");
    req.model = None;
    req.reasoning = None;
    let image = workspace.path().join("native-image.png");
    use base64::Engine;
    std::fs::write(&image, base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jkXcAAAAASUVORK5CYII=").unwrap()).unwrap();
    req.attachments.push(image.display().to_string());
    let (controls, steer, _) = run_controls();
    let turn = controls.turn.clone();
    let mut stream = harness.run(req.clone(), controls).await.unwrap();
    let events = until_done(&mut stream).await;
    assert!(events.iter().any(|e| matches!(e, AgentEvent::ToolCall { call: ToolCall::Exec { command }, .. } if command == "printf 'copilot-ok'")), "{events:?}");
    assert!(events.iter().any(|e| matches!(e, AgentEvent::ToolResult { output: Some(output), is_error: false, .. } if output.contains("copilot-ok"))), "{events:?}");
    assert!(text(&events).contains("LOCAL-DONE"));
    assert!(
        text(&events).contains("IMAGE-OK"),
        "native model must receive the image: {events:?}"
    );
    let sid = done(&events, DoneStatus::Completed);
    assert!(!sid.is_empty());
    steer
        .send(SteerMessage::text("native-followup"))
        .await
        .unwrap();
    assert_eq!(
        done(&until_done(&mut stream).await, DoneStatus::Completed),
        sid
    );
    steer.send(SteerMessage::text("native-wait")).await.unwrap();
    loop {
        if matches!(next(&mut stream).await, AgentEvent::TextDelta { text } if text.contains("WAITING"))
        {
            break;
        }
    }
    turn.stop_turn();
    assert_eq!(
        done(&until_done(&mut stream).await, DoneStatus::Interrupted),
        sid
    );
    steer
        .send(SteerMessage::text("native-after-stop"))
        .await
        .unwrap();
    assert_eq!(
        done(&until_done(&mut stream).await, DoneStatus::Completed),
        sid
    );
    drop(steer);
    assert!(
        tokio::time::timeout(Duration::from_secs(10), stream.next())
            .await
            .unwrap()
            .is_none()
    );

    req.attachments.clear();
    req.prompt = "native-resume".into();
    req.resume = Some(sid.clone());
    req.require_native_resume = true;
    let (controls, _steer, _) = run_controls();
    let mut resumed = harness.run(req, controls).await.unwrap();
    let events = until_done(&mut resumed).await;
    assert_eq!(
        text(&events),
        "LOCAL-DONE",
        "native history must not replay"
    );
    assert_eq!(done(&events, DoneStatus::Completed), sid);
    drop(resumed);
    model.kill().await.unwrap();
}

#[tokio::test]
async fn copilot_rejects_models_not_available_to_the_account() {
    let workspace = tempfile::tempdir().unwrap();
    let mut req = request(workspace.path(), "must not run");
    req.model = Some("unavailable-model".into());
    let (controls, _steer, _) = run_controls();
    let mut stream = harness().run(req, controls).await.unwrap();
    let events = until_done(&mut stream).await;
    assert!(text(&events).is_empty());
    assert!(
        matches!(events.last(), Some(AgentEvent::Done { status: DoneStatus::Errored, error: Some(error), .. }) if error.contains("unavailable-model")),
        "{events:?}"
    );
}

#[tokio::test]
async fn copilot_followup_keeps_its_images_and_skips_unreadable_files() {
    let workspace = tempfile::tempdir().unwrap();
    let image = workspace.path().join("attachment.gif");
    std::fs::write(&image, b"GIF89a-local-fixture").unwrap();
    let (controls, steer, _) = run_controls();
    let mut stream = harness()
        .run(request(workspace.path(), "first"), controls)
        .await
        .unwrap();
    done(&until_done(&mut stream).await, DoneStatus::Completed);
    steer
        .send(SteerMessage {
            prompt: "image followup".into(),
            message_id: None,
            config: None,
            attachments: vec![
                image.display().to_string(),
                workspace.path().join("missing.png").display().to_string(),
            ],
        })
        .await
        .unwrap();
    let events = until_done(&mut stream).await;
    assert!(text(&events).contains("images=1"), "{events:?}");
    done(&events, DoneStatus::Completed);
}
