//! Load the existing Zeron delegation tools into this OMP process only.
//! OMP shares Pi's extension `registerTool`/`session_start` API, so the bridge
//! is the same shape under OMP's own env key and extension flag.
use crate::{HarnessError, process::Command, scratch::ScratchDir};

pub(super) fn configure(
    cmd: &mut Command,
    config: &zeron_proto::McpServer,
) -> Result<ScratchDir, HarnessError> {
    let scratch = ScratchDir::new("omp-mcp")?;
    let extension = scratch.path().join("zeron-mcp.mjs");
    std::fs::write(&extension, include_str!("mcp.mjs"))?;
    cmd.arg("--extension").arg(extension).env(
        "ZERON_OMP_MCP",
        serde_json::to_string(config).expect("serializable MCP config"),
    );
    Ok(scratch)
}
