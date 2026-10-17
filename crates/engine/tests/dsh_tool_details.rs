//! DSH ACP → shared transcript → desktop/mobile tool summaries, without a provider.
#![cfg(unix)]
use std::{collections::BTreeMap, os::unix::fs::PermissionsExt, sync::Arc, time::Duration};
use zeron_doc::MessagePart;
use zeron_engine::{EngineCore, HarnessRegistry};
use zeron_harness::{
    AcpHarness,
    environment::{EnvironmentEntry, EnvironmentSnapshot},
};
use zeron_proto::{HarnessId, RunRequest, SandboxLevel, ToolCall};

#[tokio::test]
async fn dsh_tool_details_survive_transcript_sanitization() {
    let temp = tempfile::tempdir().unwrap();
    let script = temp.path().join("dsh-tools.py");
    std::fs::write(
        &script,
        include_str!("../../harness/tests/fixtures/dsh-tools.py"),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let fixture = include_str!("../../harness/tests/fixtures/dsh-tool-calls.json");
    std::fs::write(temp.path().join("dsh-tool-calls.json"), fixture).unwrap();
    let home = temp.path().join("dsh-home");
    std::fs::create_dir(&home).unwrap();
    let env = EnvironmentSnapshot {
        entries: BTreeMap::from([(
            "DSH_HOME".into(),
            EnvironmentEntry::Set {
                value: home.to_string_lossy().into_owned(),
            },
        )]),
        ..Default::default()
    };
    let registry = HarnessRegistry::new();
    registry.register(Arc::new(
        AcpHarness::dsh()
            .with_executable(&script)
            .with_environment(Arc::new(env)),
    ));
    let core = EngineCore::assemble(
        &temp.path().join("data"),
        Arc::new(registry),
        HarnessId::Dsh,
        None,
    )
    .unwrap();
    let rpc = zeron_rpc::memory_client(core.rpc_service());
    rpc.call(zeron_rpc::methods::MUTATE, serde_json::json!({"op":"createChat", "chatId":"dsh-details", "deviceId":core.device_id, "cwd":temp.path()})).await.unwrap();
    core.workspace
        .rename_chat("dsh-details", "Tool detail fixture")
        .unwrap();
    core.sessions
        .dispatch(
            "dsh-details",
            HarnessId::Dsh,
            RunRequest {
                prompt: "fixture".into(),
                cwd: temp.path().to_string_lossy().into_owned(),
                harness: None,
                model: None,
                reasoning: None,
                model_options: Default::default(),
                sandbox: SandboxLevel::WorkspaceWrite,
                auto_approve: true,
                resume: None,
                require_native_resume: false,
                attachments: vec![],
                worktree: None,
                mcp: None,
            },
            None,
        )
        .await
        .unwrap();
    let cases: Vec<serde_json::Value> = serde_json::from_str(fixture).unwrap();
    let handle = core.doc_host.open("dsh-details").unwrap();
    let tools = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let entries = handle.doc().read_entries().unwrap();
            let tools: Vec<_> = entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter_map(|part| {
                    if let MessagePart::Tool {
                        id,
                        call,
                        resolved,
                        output,
                        ..
                    } = part
                    {
                        Some((id.clone(), call.clone(), *resolved, output.clone()))
                    } else {
                        None
                    }
                })
                .collect();
            if tools.len() == cases.len()
                && tools.iter().all(|(_, _, resolved, _)| *resolved)
                && !core.sessions.turn_in_flight("dsh-details")
            {
                break tools;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("all fixture calls should reach the shared transcript");
    for (id, call, _, output) in tools {
        let expected: ToolCall =
            serde_json::from_value(cases[id.parse::<usize>().unwrap()]["expected"].clone())
                .unwrap();
        assert_eq!(call, zeron_doc::sanitize_tool_call(&expected), "tool {id}");
        let persisted = serde_json::to_string(&call).unwrap();
        assert!(!persisted.contains("fixture-secret"), "{persisted}");
        assert!(output.is_none(), "output stays journal-only");
        let (label, detail) = zeron_proto::view::tool_chip_content(&call);
        if !matches!(expected, ToolCall::Unknown { .. }) {
            assert_ne!(label, "Tool", "tool {id} still lacks a typed summary");
            assert!(!detail.is_empty(), "tool {id} has no visible detail");
        }
    }
    core.sessions.terminate("dsh-details").await.unwrap();
}
