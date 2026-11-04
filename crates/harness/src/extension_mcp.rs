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

/// Configure one isolated bridge per run-scoped MCP server for either host.
pub(crate) fn configure_all(
    host: &ExtensionHost,
    cmd: &mut Command,
    mcp: Option<&zeron_proto::McpServer>,
    extras: &[zeron_proto::McpServer],
) -> Result<Option<ScratchDir>, HarnessError> {
    let mut servers = mcp.into_iter().chain(extras);
    let Some(first) = servers.next() else { return Ok(None); };
    let scratch = configure(host, cmd, first)?;
    for (index, config) in servers.enumerate().map(|(i, c)| (i + 1, c)) {
        let var = format!("{}_{index}", host.env_key);
        let source = source(host).replace(
            &format!("process.env.{}", host.env_key),
            &format!("process.env.{var}"),
        );
        let extension = scratch.path().join(format!("zeron-mcp-{index}.mjs"));
        std::fs::write(&extension, source)?;
        cmd.arg("--extension").arg(extension).env(var, serde_json::to_string(config).expect("serializable MCP config"));
    }
    Ok(Some(scratch))
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
    fn plugin_servers_keep_separate_configs_for_both_extension_hosts() {
        let app = zeron_proto::McpServer {
            name: "zeron".into(), command: "zerun".into(),
            args: vec!["mcp".into()], env: Default::default(),
        };
        let plugin = zeron_proto::McpServer {
            name: "plugin-focus".into(), command: "focus-server".into(),
            args: vec![], env: Default::default(),
        };
        for host in [&PI, &OMP] {
            let mut cmd = Command::new("fixture-agent");
            let scratch = configure_all(host, &mut cmd, Some(&app), std::slice::from_ref(&plugin)).unwrap().unwrap();
            let envs: std::collections::HashMap<_, _> = cmd.as_std().get_envs()
                .filter_map(|(k,v)| Some((k.to_string_lossy().into_owned(),v?.to_string_lossy().into_owned()))).collect();
            assert_eq!(serde_json::from_str::<zeron_proto::McpServer>(&envs[host.env_key]).unwrap(), app);
            assert_eq!(serde_json::from_str::<zeron_proto::McpServer>(&envs[&format!("{}_1",host.env_key)]).unwrap(), plugin);
            assert_eq!(cmd.as_std().get_args().filter(|a| *a == "--extension").count(), 2);
            let bridge = std::fs::read_to_string(scratch.path().join("zeron-mcp-1.mjs")).unwrap();
            assert!(bridge.contains(&format!("process.env.{}_1)",host.env_key)));
            assert!(!bridge.contains(&format!("process.env.{})",host.env_key)));
            let mut bare = Command::new("fixture-agent");
            assert!(configure_all(host, &mut bare, None, &[]).unwrap().is_none());
            assert_eq!(bare.as_std().get_args().count(),0);
        }
    }

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
