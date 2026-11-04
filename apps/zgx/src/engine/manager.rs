//! Start the matching fork engine on an isolated experimental profile.
//! `ZERUN_ZGX_ENGINE_*` overrides apply only to this entry point. Stable
//! desktop ports and default profile directories are never adopted.
//! The engine survives closing the window and owns session/auth persistence.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The production engine's data dirs, relative to home: today's and the one
/// it was renamed from in 0.2.0.
const PRODUCTION_DATA: &[&str] = &[".zerun", ".zeron", ".comet-native"];

const STAGING_PORT: u16 = 27872;
const STAGING_DATA: &str = ".zerun-experimental";


/// The soonest a second engine is started after the first.
const SPAWN_SPACING: Duration = Duration::from_secs(10);

pub struct Config {
    pub port: u16,
    pub data_dir: PathBuf,
    bin: Option<PathBuf>,
    home: Option<PathBuf>,
    spawn: bool,
    /// What the engine signs in against: `src/auth.rs`'s.
    edge: String,
    client: String,
    callback_port: u16,
    /// Zerun.app's engine, managed as Zerun.app manages it (the beta): its
    /// bundle's, started through crates/launch; it restarts into a newer
    /// bundle by itself.
    managed: bool,
}

/// Why no engine can be used.
pub type Refusal = String;

/// The file in an engine's data dir holding the build of the engine this
/// app started there last (`zerun-engine --build-id`): an engine reporting
/// that build is one this app started.
const STARTED: &str = "zgx-started-build";

impl Config {
    /// The matching executable, including when automatic spawning is disabled.
    pub fn executable(&self) -> Option<&Path> {
        self.bin.as_deref()
    }

    /// The engine this app would start now, if it may start one.
    pub fn startable(&self) -> Option<&Path> {
        if self.spawn { self.bin.as_deref() } else { None }
    }

    /// Whether this is Zerun.app's engine, which follows its bundle by
    /// itself (no "older than the one on disk" of this app's to judge).
    pub fn follows_bundle(&self) -> bool {
        self.managed
    }
}

/// Which build `bin` is (`zerun-engine --build-id`), if it says; an engine
/// from before build ids doesn't.
pub fn build_of(bin: &Path) -> Option<String> {
    let out = std::process::Command::new(bin)
        .arg("--build-id")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let id = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    (out.status.success() && !id.is_empty() && !id.contains(' ')).then_some(id)
}

/// The build of the engine this app last started with `config`'s data.
pub fn started_build(config: &Config) -> Option<String> {
    let id = std::fs::read_to_string(config.data_dir.join(STARTED)).ok()?;
    let id = id.trim();
    (!id.is_empty()).then(|| id.to_owned())
}

impl Config {
    /// This client always uses an isolated profile, including packaged builds.
    pub fn from_env(edge: &str, client: &str, callback_port: u16) -> Result<Self, Refusal> {
        let var = |key: &str| std::env::var(key).ok().filter(|v: &String| !v.trim().is_empty());
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from).ok_or_else(|| "A user home directory is required for the experimental profile".to_string())?;
        let exe = std::env::current_exe().ok();
        Self::from_vars(var, &home, exe.as_deref(), edge, client, callback_port)
    }

    fn from_vars(
        var: impl Fn(&str) -> Option<String>, home: &Path, exe: Option<&Path>,
        edge: &str, client: &str, callback_port: u16,
    ) -> Result<Self, Refusal> {
        let port = match var("ZERUN_ZGX_ENGINE_PORT") {
            Some(raw) => raw.trim().parse().ok().filter(|p| *p != 0)
                .ok_or_else(|| format!("ZERUN_ZGX_ENGINE_PORT `{raw}` isn't a port"))?,
            None => STAGING_PORT,
        };
        let data_dir = var("ZERUN_ZGX_ENGINE_DATA")
            .map(|dir| expand(&dir, home))
            .unwrap_or_else(|| {
                if cfg!(windows) {
                    var("LOCALAPPDATA").map(PathBuf::from)
                        .unwrap_or_else(|| home.join("AppData/Local")).join("Zerun Experimental")
                } else { home.join(STAGING_DATA) }
            });
        if matches!(port, 27654 | 27655) || var("ZERUN_IPC_PORT").and_then(|p| p.parse::<u16>().ok()) == Some(port) {
            return Err(format!("port {port} belongs to a stable desktop"));
        }
        if let Some(dir) = PRODUCTION_DATA.iter().find(|dir| same_dir(&data_dir, &home.join(dir))) {
            return Err(format!("~/{dir} belongs to a stable desktop"));
        }
        if cfg!(windows) && var("LOCALAPPDATA").is_some_and(|local| same_dir(&data_dir,&PathBuf::from(local).join("Zerun"))) {
            return Err("that data directory belongs to the stable desktop".into());
        }
        if var("ZERUN_DATA_DIR").is_some_and(|stable| same_dir(&data_dir, &expand(&stable,home))) {
            return Err("that data directory belongs to the stable desktop".into());
        }
        let bin = var("ZERUN_ZGX_ENGINE_BIN").map(|p| expand(&p, home))
            .or_else(|| exe.and_then(zeron_launch::engine_executable))
            .or_else(|| {
                let built = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join(if cfg!(windows) { "../../target/debug/zerun-engine.exe" } else { "../../target/debug/zerun-engine" });
                built.exists().then_some(built)
            });
        Ok(Self {
            port, data_dir, bin,
            home: var("ZERUN_ZGX_ENGINE_HOME").map(|dir| expand(&dir, home)),
            spawn: var("ZERUN_ZGX_ENGINE_SPAWN").as_deref() != Some("0"),
            edge: edge.into(), client: client.into(), callback_port, managed: false,
        })
    }

    pub fn describe(&self) -> String {
        format!("port {}, data {}", self.port, self.data_dir.display())
    }
}

fn expand(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(path),
    }
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| {
        let absolute = if p.is_absolute() { p.to_path_buf() } else {
            std::env::current_dir().unwrap_or_default().join(p)
        };
        let mut clean = PathBuf::new();
        for component in absolute.components() {
            match component {
                std::path::Component::CurDir => {},
                std::path::Component::ParentDir => { clean.pop(); },
                part => clean.push(part.as_os_str()),
            }
        }
        clean
    });
    canon(a) == canon(b)
}

/// Starts engines for `config`, no more often than [`SPAWN_SPACING`].
pub struct Spawner {
    last: Option<Instant>,
    said_missing: bool,
}

impl Spawner {
    pub fn new() -> Self {
        Self { last: None, said_missing: false }
    }

    /// Nothing answers on the port: start an engine, if this app may and
    /// it's been long enough since the last. Whether one was started.
    pub fn start_if_due(&mut self, config: &Config) -> bool {
        if !config.spawn || self.last.is_some_and(|at| at.elapsed() < SPAWN_SPACING) {
            return false;
        }
        let Some(bin) = &config.bin else {
            if !self.said_missing {
                self.said_missing = true;
                eprintln!(
                    "zerun-experimental: engine: nothing answers on {} and there's no engine to start: {}",
                    config.describe(),
                    if config.managed {
                        "this app has no Zerun Engine beside it"
                    } else {
                        "build one (`cargo build -p zeron --bin zerun-engine` at the repository's root) \
                         or set ZERUN_ZGX_ENGINE_BIN"
                    }
                );
            }
            return false;
        };
        self.last = Some(Instant::now());
        let started = if config.managed { launch(config) } else { spawn(config, bin) };
        match started {
            Ok(()) => {
                eprintln!("zerun-experimental: engine: started {} ({})", bin.display(), config.describe());
                true
            }
            Err(error) => {
                eprintln!("zerun-experimental: engine: {} didn't start: {error}", bin.display());
                false
            }
        }
    }
}

/// Zerun.app's engine, started as Zerun.app starts it (crates/launch).
fn launch(config: &Config) -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let child = zeron_launch::launch(
        &zeron_launch::LaunchConfig {
            data_dir: config.data_dir.clone(),
            ipc_port: config.port,
            edge_url: config.edge.clone(),
            workos_client_id: Some(config.client.clone()),
            edge_token: None,
            org_id: None,
        },
        &exe,
    )?;
    zeron_launch::reap(child);
    Ok(())
}

/// A development engine, detached (its own process group, no inherited
/// pipes), so this app closing leaves it running; it logs to its data dir.
fn spawn(config: &Config, bin: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(&config.data_dir)?;
    // Which build it is, kept (an engine that reports it is this app's), as
    // its reaper asks below: off this thread, since asking runs the binary.
    // Until then none is kept, so a check meanwhile reads it as not ours.
    let started = config.data_dir.join(STARTED);
    let _ = std::fs::remove_file(&started);
    let mut command = std::process::Command::new(bin);
    command
        .env("ZERUN_DATA_DIR", &config.data_dir)
        .env("ZERUN_IPC_PORT", config.port.to_string())
        .env("ZERON_EDGE_URL", &config.edge)
        .env("ZERON_WORKOS_CLIENT_ID", &config.client)
        .env("ZERUN_CALLBACK_PORT", config.callback_port.to_string())
        .env("ZERON_AUTO_UPDATE", "0")
        // Not the shipping app's: no app to open from a banner.
        .env_remove("ZERUN_APP")
        .env_remove("ZERUN_MANAGED_ENGINE")
        // Nor a chat's (this app run from a Zerun terminal): an engine that
        // carries a chat's origin marker lends it to everything it runs, so
        // the shipping engine's preview scan would probe its agents'
        // sign-in callbacks (Gemini CLI's OAuth ends on the first stray
        // request).
        .env_remove("ZERON_ORIGIN")
        .env_remove("ZERON_EDGE_TOKEN")
        .env_remove("ZERON_IPC_TOKEN")
        .env_remove("ZERON_IPC_TOKEN_FILE")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(home) = &config.home {
        let (claude, codex) = (home.join(".claude"), home.join(".codex"));
        std::fs::create_dir_all(&claude)?;
        std::fs::create_dir_all(&codex)?;
        command
            .env("HOME", home)
            .env("CLAUDE_CONFIG_DIR", claude)
            .env("CODEX_HOME", codex);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let child = command.spawn()?;
    // Its build kept, then it reaped when it exits, whenever that is.
    let bin = bin.to_path_buf();
    std::thread::spawn(move || {
        if let Some(id) = build_of(&bin) {
            let _ = std::fs::write(&started, id);
        }
        let mut child = child;
        let _ = child.wait();
    });
    Ok(())
}

/// Whether the engine has to start again to follow its session: signed in
/// (or choosing an organization) while its workspace is still the local
/// one it opened signed out.
pub fn needs_restart(scope: Option<&str>, signed_in: bool) -> bool {
    signed_in && scope == Some("local")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_of(vars: &[(&str, &str)], home: &Path, exe: Option<&Path>, edge: &str) -> Result<Config, Refusal> {
        let vars: Vec<(String, String)> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let prod = crate::auth::production();
        Config::from_vars(
            move |key| vars.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()),
            home,
            exe,
            edge,
            &prod.client,
            prod.port,
        )
    }

    #[test]
    fn a_signed_in_session_on_a_local_workspace_restarts() {
        assert!(needs_restart(Some("local"), true));
        assert!(!needs_restart(Some("local"), false));
        assert!(!needs_restart(Some("synced"), true));
        assert!(!needs_restart(Some("development"), true));
        assert!(!needs_restart(None, true));
    }

    #[test]
    fn experimental_profiles_refuse_both_stable_desktops() {
        let home = Path::new("/tmp/example-home");
        let edge = "https://edge.550w.host";
        for port in ["27654", "27655", "0"] {
            assert!(config_of(&[("ZERUN_ZGX_ENGINE_PORT",port)],home,None,edge).is_err());
        }
        for dir in ["~/.zerun", "~/.zeron", "~/.comet-native", "~/.zerun/cache/.."] {
            assert!(config_of(&[("ZERUN_ZGX_ENGINE_DATA",dir)],home,None,edge).is_err());
        }
        assert!(config_of(&[("ZERUN_IPC_PORT","28000"),("ZERUN_ZGX_ENGINE_PORT","28000")],home,None,edge).is_err());
        assert!(config_of(&[("ZERUN_DATA_DIR","~/stable-custom"),("ZERUN_ZGX_ENGINE_DATA","~/stable-custom")],home,None,edge).is_err());
        let profile = config_of(&[],home,None,edge).unwrap();
        assert_eq!(profile.port,27872);
        assert_eq!(profile.data_dir,home.join(".zerun-experimental"));
        assert!(!profile.follows_bundle());
        let dir = std::env::temp_dir().join(format!("zerun-experimental-helper-{}",std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let helper = dir.join(if cfg!(windows) {"zerun-engine.exe"} else {"zerun-engine"});
        std::fs::write(&helper,"").unwrap();
        let bundled = config_of(&[],home,Some(&dir.join("zerun-experimental")),edge).unwrap();
        assert_eq!(bundled.startable(),Some(helper.as_path()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn home_relative_paths_expand() {
        let home = Path::new("/Users/someone");
        assert_eq!(expand("~/.zerun-experimental", home), home.join(".zerun-experimental"));
        assert_eq!(expand("/tmp/x", home), PathBuf::from("/tmp/x"));
    }
}
