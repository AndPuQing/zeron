//! `zerun-engine` — the desktop app's background engine, as a process of its
//! own ("Zerun Engine" in Activity Monitor): the app starts it and attaches;
//! sessions keep running with the app closed. Without the UI linked in.
//!
//! It also answers what the engine runs its own executable for: `mcp` (the
//! `zeron` MCP server injected into agents, the tool name they expect) and
//! `--noop-browser` (browser suppression for agents' sign-in flows).

#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() -> anyhow::Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("--noop-browser") => Ok(()),
        Some("mcp") => {
            zeron_app::init_logging(false, "mcp", true);
            zeron_app::run_mcp()
        }
        Some("--version" | "-V") => {
            println!("zerun-engine {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => zeron_app::background::run(),
    }
}
