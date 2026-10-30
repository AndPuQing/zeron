use std::sync::Arc;
use zeron_engine::{
    EngineCore,
    registry::{HarnessDescriptor, HarnessRegistry},
};
use zeron_proto::{HarnessConfigFiles, HarnessId, SteeringMode};
use zeron_rpc::methods;

#[tokio::test]
async fn configuration_discovery_is_metadata_only_and_does_not_launch_the_provider() {
    let directory = tempfile::tempdir().unwrap();
    let registry = HarnessRegistry::new();
    registry.register_lazy(
        HarnessDescriptor {
            id: HarnessId::Codex,
            name: "Codex".into(),
            supports_steering: false,
            steering_mode: SteeringMode::TurnBoundary,
            reasoning_levels: vec![],
            installed: false,
            can_install: false,
            enabled: Some(false),
            access_modes: vec![],
            supports_plan: false,
        },
        Box::new(|| false),
        Box::new(|| panic!("listing paths must not resolve/start an agent")),
    );
    let core =
        EngineCore::assemble(directory.path(), Arc::new(registry), HarnessId::Mock, None).unwrap();
    let client = zeron_rpc::memory_client(core.rpc_service());
    let value = client
        .call(
            methods::LIST_HARNESS_CONFIG_FILES,
            serde_json::json!({"harness":"codex"}),
        )
        .await
        .unwrap();
    let files: HarnessConfigFiles = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(files.files.len(), 1);
    assert!(files.files[0].path.ends_with("config.toml"));
    let fields = value["files"][0].as_object().unwrap();
    assert_eq!(fields.len(), 3);
    assert!(
        fields.contains_key("label")
            && fields.contains_key("path")
            && fields.contains_key("exists")
    );
    for harness in ["mock", "muse", "not-a-provider"] {
        assert!(
            client
                .call(
                    methods::LIST_HARNESS_CONFIG_FILES,
                    serde_json::json!({"harness":harness})
                )
                .await
                .is_err()
        );
    }
    core.doc_host.shutdown_workers().await;
}
