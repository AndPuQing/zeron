//! Start the matching fork engine. Packaged Betas share the installed profile;
//! source previews use an isolated experimental profile.
//! `ZERUN_ZGX_ENGINE_*` overrides apply only to source previews, which never
//! adopt the installed desktop's ports or default profile directories.
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

#[derive(Clone)]
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
    /// Packaged Betas use the installed profile; development previews stay isolated.
    pub fn from_env(edge: &str, client: &str, callback_port: u16) -> Result<Self, Refusal> {
        let var = |key: &str| std::env::var(key).ok().filter(|v: &String| !v.trim().is_empty());
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from).ok_or_else(|| "A user home directory is required for the experimental profile".to_string())?;
        let exe = std::env::current_exe().ok();
        if cfg!(feature = "beta") {
            Self::installed(var, &home, exe.as_deref(), edge, client, callback_port)
        } else {
            Self::from_vars(var, &home, exe.as_deref(), edge, client, callback_port)
        }
    }

    fn installed(
        var: impl Fn(&str) -> Option<String>, home: &Path, exe: Option<&Path>,
        edge: &str, client: &str, callback_port: u16,
    ) -> Result<Self, Refusal> {
        let production = crate::auth::production();
        if (edge, client, callback_port) != (production.edge.as_str(), production.client.as_str(), production.port) {
            return Err("The packaged Beta needs the Zerun account service".into());
        }
        let port = zeron_cli::ipc_port_from(var("ZERUN_IPC_PORT").as_deref());
        let data_dir = zeron_cli::paths::resolve_data_dir(|key| match key {
            "HOME" => Some(home.as_os_str().to_owned()),
            key => var(key).map(Into::into),
        });
        if port == STAGING_PORT || same_dir(&data_dir, &home.join(STAGING_DATA)) {
            return Err("The packaged Beta cannot use the development preview's profile".into());
        }
        Ok(Self {
            port, data_dir,
            bin: exe.and_then(zeron_launch::engine_executable),
            home: None, spawn: true,
            edge: edge.into(), client: client.into(), callback_port, managed: true,
        })
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
    said_held: bool,
}

impl Spawner {
    pub fn new() -> Self {
        Self { last: None, said_missing: false, said_held: false }
    }

    /// Whether the last look found another engine holding the data dir.
    pub fn held(&self) -> bool {
        self.said_held
    }

    /// Whether an engine this app started is likely still coming up: started
    /// less than [`SPAWN_SPACING`] ago, so it's dialled again soon rather
    /// than less and less often.
    pub fn starting(&self) -> bool {
        self.last.is_some_and(|at| at.elapsed() < SPAWN_SPACING)
    }

    /// Nothing answers on the port: start an engine, if this app may and
    /// it's been long enough since the last. Whether one was started. An
    /// engine that holds the data dir without serving yet is on its way in or
    /// out: none is started beside it, and one follows once it has let go.
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
        if zeron_launch::data_dir_held(&config.data_dir) {
            if !self.said_held {
                self.said_held = true;
                eprintln!("zerun-experimental: engine: another holds {} and isn't serving; waiting for it", config.data_dir.display());
            }
            return false;
        }
        self.said_held = false;
        self.last = Some(Instant::now());
        // Started on a thread of its own: creating the process waits for the
        // system to map (and Windows' antivirus to scan) the engine's
        // executable, a few hundred milliseconds the first time a new
        // version runs, which the window's first frame shouldn't wait for.
        let (config, bin) = (config.clone(), bin.clone());
        let started = std::thread::Builder::new().name("engine-start".into()).spawn(move || {
            let started = if config.managed { launch(&config) } else { spawn(&config, &bin) };
            match started {
                Ok(()) => eprintln!("zerun-experimental: engine: started {} ({})", bin.display(), config.describe()),
                Err(error) => eprintln!("zerun-experimental: engine: {} didn't start: {error}", bin.display()),
            }
        });
        if let Err(error) = started {
            eprintln!("zerun-experimental: engine: couldn't start a thread to start it: {error}");
            self.last = None;
            return false;
        }
        true
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
/// one it opened signed out, unless this device keeps its chats local; or
/// synced while it now keeps them local.
pub fn needs_restart(scope: Option<&str>, signed_in: bool, keep_local: bool) -> bool {
    signed_in
        && match scope {
            Some("local") => !keep_local,
            Some("synced") => keep_local,
            _ => false,
        }
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
    fn packaged_beta_keeps_the_installed_data_and_engine_identity() {
        let home = Path::new("/example/home");
        let prod = crate::auth::production();
        let build = |vars: &[(&str, &str)]| Config::installed(
            |key| vars.iter().find(|(k, _)| *k == key).map(|(_, v)| v.to_string()),
            home, None, &prod.edge, &prod.client, prod.port,
        ).unwrap();
        let config = build(&[("ZERUN_DATA_DIR", "/example/saved-data"), ("ZERUN_IPC_PORT", "28555"),
            ("ZERUN_ZGX_ENGINE_DATA", "/example/preview"), ("ZERUN_ZGX_ENGINE_PORT", "27872")]);
        assert_eq!(config.data_dir, Path::new("/example/saved-data"));
        assert_eq!(config.port, 28555);
        assert!(config.follows_bundle());
        let default = build(&[]);
        assert_eq!(default.port, zeron_cli::DEFAULT_IPC_PORT);
        assert_eq!(default.data_dir, zeron_cli::paths::resolve_data_dir(|key| {
            (key == "HOME").then(|| home.as_os_str().to_owned())
        }));
        assert!(Config::installed(|_| None, home, None, "https://example.test", &prod.client, prod.port).is_err());
    }

    #[test]
    fn a_signed_in_session_on_a_local_workspace_restarts() {
        assert!(needs_restart(Some("local"), true, false));
        assert!(!needs_restart(Some("local"), false, false));
        assert!(!needs_restart(Some("synced"), true, false));
        assert!(!needs_restart(Some("development"), true, false));
        assert!(!needs_restart(None, true, false));
    }

    #[test]
    fn keeping_chats_local_holds_a_signed_in_engine_local() {
        // Signed in and kept local: the local workspace stays.
        assert!(!needs_restart(Some("local"), true, true));
        // Kept local after it synced: back to the local workspace.
        assert!(needs_restart(Some("synced"), true, true));
        // Signed out, it changes nothing.
        assert!(!needs_restart(Some("local"), false, true));
        assert!(!needs_restart(Some("development"), true, true));
        assert!(!needs_restart(None, true, true));
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

    /// An engine holding the data dir is on its way in or out: no second one
    /// is started beside it (it would only queue for the lock), and one is
    /// started as soon as it lets go.
    #[cfg(unix)]
    #[test]
    fn no_engine_starts_while_one_holds_the_data_dir() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("engine");
        std::fs::write(&bin, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let config = Config {
            port: 1,
            data_dir: dir.path().join("data"),
            bin: Some(bin),
            home: None,
            spawn: true,
            edge: String::new(),
            client: String::new(),
            callback_port: 0,
            managed: false,
        };
        std::fs::create_dir_all(&config.data_dir).unwrap();
        let lock = std::fs::File::create(config.data_dir.join("engine.lock")).unwrap();
        lock.try_lock().unwrap();
        let mut spawner = Spawner::new();
        assert!(!spawner.start_if_due(&config), "held: nothing started");
        lock.unlock().unwrap();
        assert!(spawner.start_if_due(&config), "let go: started at once");
    }

    /// A start returns at once (the process is made on a thread of its own),
    /// and the engine it started counts as coming up until a second could
    /// be started: one start, not two, however soon it's asked again.
    #[test]
    fn a_start_returns_at_once_and_is_coming_up_until_the_next_is_due() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            port: 1,
            data_dir: dir.path().join("data"),
            bin: Some(dir.path().join("no-such-engine")),
            home: None,
            spawn: true,
            edge: String::new(),
            client: String::new(),
            callback_port: 0,
            managed: false,
        };
        let mut spawner = Spawner::new();
        assert!(!spawner.starting());
        let asked = Instant::now();
        assert!(spawner.start_if_due(&config));
        assert!(asked.elapsed() < Duration::from_secs(1), "not waiting for the process");
        assert!(spawner.starting());
        assert!(!spawner.start_if_due(&config), "one start per SPAWN_SPACING");
        let never = Config { spawn: false, ..config };
        assert!(!Spawner::new().start_if_due(&never));
    }

    #[test]
    fn home_relative_paths_expand() {
        let home = Path::new("/Users/someone");
        assert_eq!(expand("~/.zerun-experimental", home), home.join(".zerun-experimental"));
        assert_eq!(expand("/tmp/x", home), PathBuf::from("/tmp/x"));
    }
}
