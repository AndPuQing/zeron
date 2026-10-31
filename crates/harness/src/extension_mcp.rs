//! Load the existing Zeron delegation tools into one Pi-style extension host
//! process only. Pi and OMP share the extension `registerTool`/`session_start`
//! API, so one bridge serves both under each host's own env key and client name.
use crate::{HarnessError, process::Command, scratch::ScratchDir};

/// The bridge as written, already in Pi's names.
const SOURCE: &str = include_str!("extension_mcp.mjs");
const PI_ENV_KEY: &str = "ZERON_PI_MCP";
const PI_CLIENT: &str = "\"zeron-pi\"";

/// The names one extension host sees the bridge under.
pub(crate) struct ExtensionHost {
    pub scratch_prefix: &'static str,
    pub env_key: &'static str,
    pub client_name: &'static str,
}

pub(crate) const PI: ExtensionHost = ExtensionHost {
    scratch_prefix: "pi-mcp",
    env_key: PI_ENV_KEY,
    client_name: "zeron-pi",
};

pub(crate) const OMP: ExtensionHost = ExtensionHost {
    scratch_prefix: "omp-mcp",
    env_key: "ZERON_OMP_MCP",
    client_name: "zeron-omp",
};

pub(crate) fn configure(
    host: &ExtensionHost,
    cmd: &mut Command,
    config: &zeron_proto::McpServer,
) -> Result<ScratchDir, HarnessError> {
    let scratch = ScratchDir::new(host.scratch_prefix)?;
    let extension = scratch.path().join("zeron-mcp.mjs");
    std::fs::write(&extension, source(host))?;
    cmd.arg("--extension").arg(extension).env(
        host.env_key,
        serde_json::to_string(config).expect("serializable MCP config"),
    );
    Ok(scratch)
}

fn source(host: &ExtensionHost) -> String {
    SOURCE
        .replace(PI_ENV_KEY, host.env_key)
        .replace(PI_CLIENT, &format!("{:?}", host.client_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_host_reads_its_own_env_key_and_names_itself() {
        assert_eq!(SOURCE.matches(PI_ENV_KEY).count(), 1);
        assert_eq!(SOURCE.matches(PI_CLIENT).count(), 1);
        assert_eq!(source(&PI), SOURCE);
        let omp = source(&OMP);
        assert!(omp.contains("process.env.ZERON_OMP_MCP"));
        assert!(omp.contains("name: \"zeron-omp\""));
        assert!(!omp.contains("PI_MCP") && !omp.contains("zeron-pi"));
    }
}
