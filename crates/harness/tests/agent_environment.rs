//! Actual ACP subprocesses and separate Devin model probes observe the same binding.
#![cfg(feature = "native-fixture")]
use futures::StreamExt;
use std::{sync::Arc, time::Duration};
use zeron_harness::environment::EnvironmentSnapshot;
use zeron_harness::{AcpHarness, CancellationToken, Harness, RunControls};
use zeron_proto::{AgentEvent, EnvironmentChange, RunRequest, SandboxLevel};

fn environment(value: &str) -> Arc<EnvironmentSnapshot> {
    Arc::new(
        EnvironmentSnapshot::default()
            .patched(&[
                EnvironmentChange::Set {
                    name: "AGENT_ENV_TEST".into(),
                    value: value.into(),
                },
                EnvironmentChange::Set {
                    name: "AGENT_ENV_EMPTY".into(),
                    value: String::new(),
                },
                EnvironmentChange::Set {
                    name: "PATH".into(),
                    value: "literal-path".into(),
                },
            ])
            .unwrap(),
    )
}
async fn observe(harness: AcpHarness, value: &str) {
    let root = tempfile::tempdir().unwrap();
    let harness = harness
        .with_executable(env!("CARGO_BIN_EXE_harness-native-fixture"))
        .with_sessions_root(root.path())
        .with_environment(environment(value));
    let models = harness.models().await.unwrap();
    assert!(
        models.iter().any(|model| model.label.contains(value)),
        "{models:?}"
    );
    let (steer, steering) = tokio::sync::mpsc::channel(4);
    let controls = RunControls {
        realtime: None,
        execution_lease: None,
        steering,
        interrupt: CancellationToken::new(),
        request_input: Box::new(|_| tokio::sync::oneshot::channel().1),
    };
    let req = RunRequest {
        prompt: "observe".into(),
        harness: None,
        model: None,
        reasoning: None,
        model_options: Default::default(),
        cwd: root.path().display().to_string(),
        sandbox: SandboxLevel::WorkspaceWrite,
        auto_approve: true,
        attachments: vec![],
        worktree: None,
        resume: None,
        require_native_resume: false,
        mcp: None,
    };
    let mut stream = harness.run(req, controls).await.unwrap();
    let report = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(event) = stream.next().await {
            if let AgentEvent::TextDelta { text } = event.unwrap() {
                return serde_json::from_str::<serde_json::Value>(&text).unwrap();
            }
        }
        panic!("missing child observation")
    })
    .await
    .unwrap();
    assert_eq!(report["environment"]["value"], value);
    assert_eq!(report["environment"]["empty"], "");
    assert_eq!(report["environment"]["path"], "literal-path");
    drop(steer);
}

#[tokio::test]
async fn acp_providers_and_devin_model_probe_receive_isolated_literal_values() {
    let parent = std::env::var_os("AGENT_ENV_TEST");
    tokio::join!(
        observe(AcpHarness::grok(), "Grok 日本語 $HOME; `literal`\nline"),
        observe(AcpHarness::devin(), "Devin é $() & |"),
        observe(AcpHarness::hermes(), "Hermes 😀"),
        observe(AcpHarness::antigravity(), "Antigravity value")
    );
    assert_eq!(std::env::var_os("AGENT_ENV_TEST"), parent);
}

#[cfg(unix)]
#[tokio::test]
async fn native_runs_and_model_probes_receive_their_provider_binding() {
    use std::{os::unix::fs::PermissionsExt, path::Path};
    use zeron_harness::{ClaudeHarness, CodexHarness, CursorHarness, PiHarness};
    fn wrapper(
        root: &Path,
        name: &str,
        executable: &Path,
        interpreter: Option<&str>,
    ) -> std::path::PathBuf {
        fn quote(value: &str) -> String {
            format!("'{}'", value.replace('\'', "'\\''"))
        }
        let script = root.join(name);
        let report = root.join("observations.jsonl");
        let log = "import os,json,sys; f=open(sys.argv[1],'a'); f.write(json.dumps({'args':sys.argv[2:], **{k:os.environ.get(k) for k in ['AGENT_ENV_TEST','AGENT_ENV_EMPTY']}})+'\\n')";
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n/usr/bin/python3 -c {} {} \"$@\"\nexec {} {} \"$@\"\n",
                quote(log),
                quote(report.to_str().unwrap()),
                interpreter.unwrap_or(""),
                quote(executable.to_str().unwrap())
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        script
    }
    async fn run(harness: &dyn Harness, root: &Path, prompt: &str) {
        let (steer, steering) = tokio::sync::mpsc::channel(4);
        let controls = RunControls {
            realtime: None,
            execution_lease: None,
            steering,
            interrupt: CancellationToken::new(),
            request_input: Box::new(|_| tokio::sync::oneshot::channel().1),
        };
        let request = RunRequest {
            prompt: prompt.into(),
            cwd: root.display().to_string(),
            harness: None,
            model: None,
            reasoning: None,
            model_options: Default::default(),
            sandbox: SandboxLevel::WorkspaceWrite,
            auto_approve: true,
            attachments: vec![],
            worktree: None,
            resume: None,
            require_native_resume: false,
            mcp: None,
        };
        let mut stream = harness.run(request, controls).await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(event) = stream.next().await {
                if let AgentEvent::Done { status, error, .. } = event.unwrap() {
                    assert_eq!(status, zeron_proto::DoneStatus::Completed, "{error:?}");
                    return;
                }
            }
            panic!("missing terminal event")
        })
        .await
        .unwrap();
        drop(steer);
    }
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for provider in ["codex", "cursor", "claude", "pi"] {
        let root = tempfile::tempdir().unwrap();
        let value = format!("{provider} 中文\n$HOME `literal` ; &");
        // Keep PATH available to the shell peers; ACP above verifies explicit PATH replacement.
        let mut snapshot = EnvironmentSnapshot::default()
            .patched(&[
                EnvironmentChange::Set {
                    name: "AGENT_ENV_TEST".into(),
                    value: value.clone(),
                },
                EnvironmentChange::Set {
                    name: "AGENT_ENV_EMPTY".into(),
                    value: String::new(),
                },
            ])
            .unwrap();
        snapshot.revision = format!("fixture-{provider}");
        let environment = Arc::new(snapshot);
        let harness: Box<dyn Harness> = match provider {
            "codex" => Box::new(
                CodexHarness::new()
                    .with_executable(wrapper(
                        root.path(),
                        "agent",
                        &fixtures.join("fake-codex.sh"),
                        Some("/bin/sh"),
                    ))
                    .with_environment(environment.clone()),
            ),
            "cursor" => Box::new(
                CursorHarness::new()
                    .with_executable(wrapper(
                        root.path(),
                        "agent",
                        &fixtures.join("fake-cursor-shim.sh"),
                        Some("/bin/sh"),
                    ))
                    .with_environment(environment.clone()),
            ),
            "claude" => {
                let probe = root.path().join("probe.py");
                std::fs::write(&probe, include_str!("fixtures/fake-claude-models.py")).unwrap();
                std::fs::write(root.path().join("response.json"), r#"{"subtype":"success","response":{"models":[{"value":"fixture-model","displayName":"Fixture"}],"commands":[]}}"#).unwrap();
                let probe = ClaudeHarness::new()
                    .with_executable(wrapper(
                        root.path(),
                        "probe",
                        &probe,
                        Some("/usr/bin/python3"),
                    ))
                    .with_environment(environment.clone());
                assert_eq!(probe.model_catalog(true).await.unwrap().source, "live");
                Box::new(
                    ClaudeHarness::new()
                        .with_executable(wrapper(
                            root.path(),
                            "agent",
                            &fixtures.join("fake-claude.sh"),
                            Some("/bin/sh"),
                        ))
                        .with_environment(environment.clone()),
                )
            }
            _ => Box::new(
                PiHarness::new()
                    .with_executable(wrapper(
                        root.path(),
                        "agent",
                        Path::new(env!("CARGO_BIN_EXE_harness-pi-fixture")),
                        None,
                    ))
                    .with_agent_dir(root.path().join("pi-config"))
                    .with_session_store(root.path().join("sessions"))
                    .with_environment(environment.clone()),
            ),
        };
        if provider != "claude" {
            assert_eq!(harness.model_catalog(true).await.unwrap().source, "live");
        }
        run(
            harness.as_ref(),
            root.path(),
            if provider == "pi" {
                "hello"
            } else if provider == "codex" {
                "scenario:resumed"
            } else {
                "scenario:happy"
            },
        )
        .await;
        let observations = std::fs::read_to_string(root.path().join("observations.jsonl")).unwrap();
        let mut agent_launches = 0;
        for line in observations.lines() {
            let report: serde_json::Value = serde_json::from_str(line).unwrap();
            // Executable/version detection deliberately uses the engine's
            // environment; only provider operations receive the binding.
            if report["args"] == serde_json::json!(["--version"]) {
                assert_eq!(report["AGENT_ENV_TEST"], serde_json::Value::Null);
                continue;
            }
            agent_launches += 1;
            assert_eq!(report["AGENT_ENV_TEST"], value);
            assert_eq!(report["AGENT_ENV_EMPTY"], "");
        }
        assert!(agent_launches >= 2, "probe and run must both launch");
    }
}
