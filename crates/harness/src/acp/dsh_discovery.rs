//! Keep ACP discovery sessions and their indexes out of dsh's shared history.

use crate::HarnessError;
use crate::process::Command;
use crate::scratch::ScratchDir;
use serde_json::json;

/// Discovery needs `session/new` for models and commands. dsh persists that
/// session before replying, including its workspace and projection records.
/// Apply a final, process-local overlay to the standard base-profile storage
/// entries. Keep DSH_HOME intact so profiles, credentials and model settings
/// resolve exactly as they do for real runs.
pub(super) fn configure(command: &mut Command, scratch: &ScratchDir) -> Result<(), HarnessError> {
    let patches = json!([
        {
            "id": "session-persistence-jsonl",
            "config": { "root": scratch.path().join("sessions") }
        },
        {
            "id": "storage-json",
            "config": { "root": scratch.path().join("storages") }
        },
        {
            "id": "storage-domain",
            "config": { "backend": "json" }
        },
        {
            "id": "session-query-sqlite",
            "config": { "path": ":memory:", "openAt": "never" }
        }
    ]);
    let overlay = scratch.path().join("discovery.patch.yml");
    // JSON is valid YAML; the serializer also quotes Windows paths safely.
    let bytes = serde_json::to_vec(&patches)
        .map_err(|error| HarnessError::Protocol(format!("dsh discovery overlay: {error}")))?;
    std::fs::write(&overlay, bytes)?;
    command.arg("--patch").arg(overlay);
    Ok(())
}
