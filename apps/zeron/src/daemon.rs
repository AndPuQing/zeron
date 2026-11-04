//! `zerun daemon …` — install/manage `zerun headless` as a background service:
//! a systemd **user** unit on Linux (the VPS deployment target), a launchd
//! LaunchAgent on macOS. The unit runs the current executable with the
//! `ZERON_*` environment captured at install time, so
//! `ZERON_EDGE_URL=… zerun daemon install` bakes that override in.
//!
//! Auth is decoupled: without a saved session the service remains up on the
//! local-only profile. `zerun login` and a service restart opt into sync.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use zeron_engine::InstanceLock;

const LAUNCHD_LABEL: &str = "work.puqing.zerun";
/// `zerun daemon install` exits with this, without touching launchd, when an
/// engine other than the LaunchAgent's (usually the desktop app's) already
/// holds the data dir; `edge/src/install.sh` reads it.
pub const ENGINE_ALREADY_RUNNING_EXIT: i32 = 3;
/// `bootout` returns before the old job has fully gone, and a `bootstrap` in
/// that window fails with "5: Input/output error".
const LAUNCHD_BOOTSTRAP_ATTEMPTS: u32 = 5;
/// How long `zerun daemon install` waits for the service's engine to take the
/// data dir before reporting that it did not start.
const ENGINE_START_TIMEOUT: Duration = Duration::from_secs(20);
/// Same unit name the curl|sh installer (`edge/src/install.sh`) writes, so
/// `zerun daemon …` manages that installation rather than a competing copy.
const SYSTEMD_UNIT: &str = "zerun.service";

/// Environment captured into the unit file. `PATH` is always included (the
/// engine spawns harness CLIs like `claude`, which service managers' minimal
/// default PATH won't find); the `ZERON_*`/logging vars only when set.
const CAPTURED_ENV: &[&str] = &[
    "PATH",
    "ZERUN_DATA_DIR",
    "ZERON_EDGE_URL",
    "ZERON_EDGE_TOKEN",
    "ZERON_ORG_ID",
    "ZERON_WORKOS_CLIENT_ID",
    "ZERON_WORKOS_API_BASE",
    "ZERUN_IPC_PORT",
    "ZERUN_CALLBACK_PORT",
    "ZERON_HARNESS",
    "ZERON_DEVICE_NAME",
    "RUST_LOG",
];

pub fn install(data_dir: &Path) -> anyhow::Result<()> {
    let exe = std::env::current_exe().context("resolving the zerun executable path")?;
    let env = captured_env();
    if cfg!(target_os = "macos") {
        require_launchd_login_session()?;
        let plist = launchd_plist_path()?;
        std::fs::create_dir_all(plist.parent().expect("LaunchAgents parent"))?;
        std::fs::create_dir_all(data_dir)?;
        let target = launchd_service_target()?;
        let job = launchd_print(&target);
        let previous_pid = job.as_deref().and_then(launchd_print_pid);
        // The desktop app's engine and the service share the data dir and the
        // IPC port: a second engine would only fail the lock and be restarted
        // by KeepAlive forever.
        if let Some(pid) = foreign_engine(InstanceLock::holder(data_dir).as_deref(), previous_pid) {
            eprintln!(
                "Another Zerun engine (pid {pid}, usually the desktop app) is already running on {}; {LAUNCHD_LABEL} was not installed or started.",
                data_dir.display()
            );
            eprintln!("Quit it, then run `zerun daemon install` again.");
            std::process::exit(ENGINE_ALREADY_RUNNING_EXIT);
        }
        let rendered = render_launchd_plist(
            &launchd_exec_path(&exe, std::env::var_os("HOME").map(PathBuf::from).as_deref()),
            &env,
            &data_dir.join("daemon.log"),
        );
        if job.is_some()
            && std::fs::read_to_string(&plist).is_ok_and(|installed| installed == rendered)
        {
            // Same definition (it runs `current/zerun`, so a new version too):
            // restart the loaded job in place.
            run("launchctl", &["kickstart", "-k", &target])?;
        } else {
            // Reinstall-friendly: unload any previous incarnation before rewriting.
            if job.is_some() {
                let _ = run_quiet("launchctl", &["bootout", &target]);
                wait_for_launchd_unload(&target);
            }
            std::fs::write(&plist, rendered)?;
            bootstrap_launchd(&launchd_domain()?, &plist)?;
        }
        let pid = wait_for_launchd_engine(data_dir, &target, previous_pid)?;
        println!(
            "Installed and started {LAUNCHD_LABEL} (pid {pid}, {}).",
            plist.display()
        );
    } else if cfg!(target_os = "linux") {
        let unit = systemd_unit_path()?;
        std::fs::create_dir_all(unit.parent().expect("systemd user dir"))?;
        std::fs::write(&unit, render_systemd_unit(&exe, &env))?;
        run("systemctl", &["--user", "daemon-reload"])?;
        run("systemctl", &["--user", "enable", "--now", SYSTEMD_UNIT])?;
        println!("Installed and started {SYSTEMD_UNIT} ({}).", unit.display());
        println!(
            "For start-at-boot without an active login session (VPS): loginctl enable-linger $USER"
        );
    } else {
        bail!("zerun daemon is only supported on macOS (launchd) and Linux (systemd)");
    }
    println!(
        "Without a saved account the engine stays local-only; sign-in and restart are optional for sync."
    );
    println!(
        "Logs: {}",
        if cfg!(target_os = "macos") {
            format!("{}", data_dir.join("daemon.log").display())
        } else {
            format!("journalctl --user -u {SYSTEMD_UNIT}")
        }
    );
    Ok(())
}

pub fn uninstall() -> anyhow::Result<()> {
    if cfg!(target_os = "macos") {
        let _ = run_quiet("launchctl", &["bootout", &launchd_service_target()?]);
        let plist = launchd_plist_path()?;
        match std::fs::remove_file(&plist) {
            Ok(()) => println!("Removed {}.", plist.display()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                println!("Not installed.")
            }
            Err(err) => return Err(err.into()),
        }
    } else if cfg!(target_os = "linux") {
        let _ = run_quiet("systemctl", &["--user", "disable", "--now", SYSTEMD_UNIT]);
        let unit = systemd_unit_path()?;
        match std::fs::remove_file(&unit) {
            Ok(()) => {
                run("systemctl", &["--user", "daemon-reload"])?;
                println!("Removed {}.", unit.display());
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                println!("Not installed.")
            }
            Err(err) => return Err(err.into()),
        }
    } else {
        bail!("zerun daemon is only supported on macOS (launchd) and Linux (systemd)");
    }
    Ok(())
}

pub fn start() -> anyhow::Result<()> {
    if cfg!(target_os = "macos") {
        require_launchd_login_session()?;
        let plist = launchd_plist_path()?;
        if !plist.exists() {
            bail!("not installed — run `zerun daemon install` first");
        }
        // `stop` boots the job out of the domain, so start = bootstrap; already
        // loaded is fine, then kickstart guarantees a running process either way.
        let _ = run_quiet(
            "launchctl",
            &["bootstrap", &launchd_domain()?, &plist.to_string_lossy()],
        );
        run("launchctl", &["kickstart", &launchd_service_target()?])?;
    } else if cfg!(target_os = "linux") {
        run("systemctl", &["--user", "start", SYSTEMD_UNIT])?;
    } else {
        bail!("zerun daemon is only supported on macOS (launchd) and Linux (systemd)");
    }
    println!("Started.");
    Ok(())
}

pub fn stop() -> anyhow::Result<()> {
    if cfg!(target_os = "macos") {
        // bootout (not `kill`): with KeepAlive the job would otherwise respawn.
        run("launchctl", &["bootout", &launchd_service_target()?])?;
    } else if cfg!(target_os = "linux") {
        run("systemctl", &["--user", "stop", SYSTEMD_UNIT])?;
    } else {
        bail!("zerun daemon is only supported on macOS (launchd) and Linux (systemd)");
    }
    println!("Stopped.");
    Ok(())
}

pub fn restart() -> anyhow::Result<()> {
    if cfg!(target_os = "macos") {
        if run_quiet(
            "launchctl",
            &["kickstart", "-k", &launchd_service_target()?],
        )
        .is_err()
        {
            // Not loaded (e.g. after `stop`) — fall through to a plain start.
            return start();
        }
        println!("Restarted.");
        Ok(())
    } else if cfg!(target_os = "linux") {
        run("systemctl", &["--user", "restart", SYSTEMD_UNIT])?;
        println!("Restarted.");
        Ok(())
    } else {
        bail!("zerun daemon is only supported on macOS (launchd) and Linux (systemd)");
    }
}

pub fn status() -> anyhow::Result<()> {
    if cfg!(target_os = "macos") {
        let output = Command::new("launchctl")
            .args(["print", &launchd_service_target()?])
            .output()
            .context("running launchctl")?;
        if !output.status.success() {
            println!(
                "{LAUNCHD_LABEL}: not loaded{}",
                if launchd_plist_path()?.exists() {
                    " (installed — `zerun daemon start`)"
                } else {
                    " (not installed — `zerun daemon install`)"
                }
            );
            return Ok(());
        }
        // `launchctl print` is pages long; surface just the liveness lines.
        let text = String::from_utf8_lossy(&output.stdout);
        println!("{LAUNCHD_LABEL}: loaded");
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("state = ")
                || trimmed.starts_with("pid = ")
                || trimmed.starts_with("last exit code = ")
            {
                println!("  {trimmed}");
            }
        }
        Ok(())
    } else if cfg!(target_os = "linux") {
        // Passthrough; `status` exits nonzero for inactive units, which is not an
        // error for us to report — the output already says it.
        let _ = Command::new("systemctl")
            .args(["--user", "--no-pager", "status", SYSTEMD_UNIT])
            .status()
            .context("running systemctl")?;
        Ok(())
    } else {
        bail!("zerun daemon is only supported on macOS (launchd) and Linux (systemd)");
    }
}

// ---------------------------------------------------------------------------
// Unit rendering (pure — unit-tested below)
// ---------------------------------------------------------------------------

fn captured_env() -> Vec<(String, String)> {
    CAPTURED_ENV
        .iter()
        .filter_map(|key| std::env::var(key).ok().map(|v| (key.to_string(), v)))
        .collect()
}

fn render_systemd_unit(exe: &Path, env: &[(String, String)]) -> String {
    let mut unit = String::from(
        "[Unit]\nDescription=Zerun headless engine\nAfter=network-online.target\nStartLimitIntervalSec=60\nStartLimitBurst=5\n\n[Service]\n",
    );
    for (key, value) in env {
        // systemd unquotes the value; escape the characters it treats specially.
        let value = value.replace('\\', "\\\\").replace('"', "\\\"");
        unit.push_str(&format!("Environment=\"{key}={value}\"\n"));
    }
    unit.push_str(&format!(
        "ExecStart={} headless\nRestart=on-failure\nRestartSec=5\nEnvironmentFile=-%h/.zerun/env\n\n[Install]\nWantedBy=default.target\n",
        systemd_exec_path(exe)
    ));
    unit
}

/// The ExecStart binary path. An exe under `~/.zerun/app/` came from the
/// curl|sh installer, whose upgrades relink `app/current` — point the unit at
/// the symlink (as the installer's own unit does) so it never pins one version.
/// (`current_exe` resolves symlinks, so the versioned dir is what we see here.)
fn systemd_exec_path(exe: &Path) -> String {
    exec_path_for(exe, std::env::var_os("HOME").map(PathBuf::from).as_deref())
}

fn exec_path_for(exe: &Path, home: Option<&Path>) -> String {
    let installed = home
        .map(|home| home.join(".zerun/app"))
        .is_some_and(|app_root| exe.starts_with(app_root));
    if installed {
        "%h/.zerun/app/current/zerun".to_string()
    } else {
        format!("{}", exe.display())
    }
}

/// A managed CLI update swaps `current`; launchd must not keep starting the
/// resolved old version after `zerun update` restarts the service. macOS's
/// `current_exe` keeps the path the binary was started through (for example
/// `~/.local/bin/zerun`), so compare with symlinks resolved on both sides.
fn launchd_exec_path(exe: &Path, home: Option<&Path>) -> PathBuf {
    let resolved = |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    if let Some(root) = home.map(|home| home.join(".zerun/app")) {
        let exe = resolved(exe);
        if exe.starts_with(&root) || exe.starts_with(resolved(&root)) {
            return root.join("current/zerun");
        }
    }
    exe.to_owned()
}

/// The pid of an engine holding the data dir that is not the LaunchAgent's
/// own job (a reinstall restarts that one in place).
fn foreign_engine<'a>(holder: Option<&'a str>, job_pid: Option<&str>) -> Option<&'a str> {
    holder.filter(|holder| Some(*holder) != job_pid)
}

/// `launchctl print <service>` output, `None` when the job is not loaded.
fn launchd_print(target: &str) -> Option<String> {
    let output = Command::new("launchctl")
        .args(["print", target])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The running process of a loaded job (`pid = 123`), absent while it is
/// waiting to be (re)started.
fn launchd_print_pid(print: &str) -> Option<&str> {
    print
        .lines()
        .find_map(|line| line.trim().strip_prefix("pid = "))
        .map(str::trim)
        .filter(|pid| !pid.is_empty() && pid.bytes().all(|byte| byte.is_ascii_digit()))
}

fn wait_for_launchd_unload(target: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while launchd_print(target).is_some() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn bootstrap_launchd(domain: &str, plist: &Path) -> anyhow::Result<()> {
    let plist = plist.to_string_lossy();
    let mut attempt = 1;
    loop {
        match run("launchctl", &["bootstrap", domain, &plist]) {
            Ok(()) => return Ok(()),
            Err(error) if attempt < LAUNCHD_BOOTSTRAP_ATTEMPTS => {
                eprintln!("{error:#}; retrying");
                attempt += 1;
                std::thread::sleep(Duration::from_secs(1));
            }
            Err(error) => return Err(error),
        }
    }
}

/// Wait until a new process of the job (not `previous`, the one replaced)
/// holds the data dir: launchd accepting the job only means it will try to
/// start it.
fn wait_for_launchd_engine(
    data_dir: &Path,
    target: &str,
    previous: Option<&str>,
) -> anyhow::Result<String> {
    let deadline = Instant::now() + ENGINE_START_TIMEOUT;
    loop {
        let job_pid = launchd_print(target)
            .as_deref()
            .and_then(launchd_print_pid)
            .map(str::to_owned);
        if let Some(pid) = job_pid
            && Some(pid.as_str()) != previous
            && InstanceLock::holder(data_dir).as_deref() == Some(pid.as_str())
        {
            return Ok(pid);
        }
        if Instant::now() >= deadline {
            bail!(
                "{LAUNCHD_LABEL} is installed, but its engine did not start within {}s; see {}",
                ENGINE_START_TIMEOUT.as_secs(),
                data_dir.join("daemon.log").display()
            );
        }
        // `holder` briefly takes the lock itself; leave the engine room to.
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn require_launchd_login_session() -> anyhow::Result<()> {
    let domain = launchd_domain()?;
    if !Command::new("launchctl")
        .args(["print", &domain])
        .output()
        .is_ok_and(|output| output.status.success())
    {
        bail!(
            "No macOS GUI login session ({domain}). Log in at the Mac, then run `zerun daemon install`. For an SSH-only session, run `zerun headless` directly, for example inside tmux."
        );
    }
    Ok(())
}

fn render_launchd_plist(exe: &Path, env: &[(String, String)], log: &Path) -> String {
    let mut env_dict = String::new();
    for (key, value) in env {
        env_dict.push_str(&format!(
            "      <key>{}</key><string>{}</string>\n",
            xml_escape(key),
            xml_escape(value)
        ));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
  <dict>
    <key>Label</key><string>{label}</string>
    <key>ProgramArguments</key>
    <array>
      <string>{exe}</string>
      <string>headless</string>
    </array>
    <key>EnvironmentVariables</key>
    <dict>
{env_dict}    </dict>
    <key>RunAtLoad</key><true/>
    <key>KeepAlive</key>
    <dict>
      <key>SuccessfulExit</key><false/>
    </dict>
    <key>ThrottleInterval</key><integer>30</integer>
    <key>StandardOutPath</key><string>{log}</string>
    <key>StandardErrorPath</key><string>{log}</string>
  </dict>
</plist>
"#,
        label = LAUNCHD_LABEL,
        exe = xml_escape(&exe.to_string_lossy()),
        env_dict = env_dict,
        log = xml_escape(&log.to_string_lossy()),
    )
}

fn xml_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// ---------------------------------------------------------------------------
// Paths + process helpers
// ---------------------------------------------------------------------------

fn home_dir() -> anyhow::Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME not set")
}

fn launchd_plist_path() -> anyhow::Result<PathBuf> {
    Ok(home_dir()?
        .join("Library/LaunchAgents")
        .join(format!("{LAUNCHD_LABEL}.plist")))
}

fn systemd_unit_path() -> anyhow::Result<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or(home_dir()?.join(".config"));
    Ok(config.join("systemd/user").join(SYSTEMD_UNIT))
}

fn launchd_domain() -> anyhow::Result<String> {
    let output = Command::new("id").arg("-u").output().context("id -u")?;
    let uid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if uid.is_empty() {
        bail!("could not determine the current uid");
    }
    Ok(format!("gui/{uid}"))
}

fn launchd_service_target() -> anyhow::Result<String> {
    Ok(format!("{}/{LAUNCHD_LABEL}", launchd_domain()?))
}

/// Run a command echoing it first; error (with stderr) on nonzero exit.
fn run(program: &str, args: &[&str]) -> anyhow::Result<()> {
    println!("$ {program} {}", args.join(" "));
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("running {program}"))?;
    if !output.status.success() {
        bail!(
            "{program} {} failed ({}): {}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// Run without echoing; used where failure is an expected branch.
fn run_quiet(program: &str, args: &[&str]) -> anyhow::Result<()> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("running {program}"))?;
    if !output.status.success() {
        bail!("{program} failed ({})", output.status);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systemd_unit_shape() {
        let unit = render_systemd_unit(
            Path::new("/usr/local/bin/zerun"),
            &[
                ("PATH".into(), "/usr/bin:/bin".into()),
                ("ZERON_EDGE_URL".into(), "https://edge.example".into()),
                ("RUST_LOG".into(), "info,zerun=\"debug\"".into()),
            ],
        );
        assert!(unit.contains("ExecStart=/usr/local/bin/zerun headless\n"));
        assert!(unit.contains("Environment=\"PATH=/usr/bin:/bin\"\n"));
        assert!(unit.contains("Environment=\"ZERON_EDGE_URL=https://edge.example\"\n"));
        // Inner quotes escaped so systemd re-parses the value verbatim.
        assert!(unit.contains("Environment=\"RUST_LOG=info,zerun=\\\"debug\\\"\"\n"));
        assert!(unit.contains("StartLimitIntervalSec=60\n"));
        assert!(unit.contains("StartLimitBurst=5\n"));
        assert!(unit.contains("Restart=on-failure"));
        assert!(!unit.contains("session.json"));
        assert!(!unit.contains("ConditionPathExists"));
        assert!(unit.contains("EnvironmentFile=-%h/.zerun/env"));
        assert!(unit.contains("WantedBy=default.target"));
    }

    #[test]
    fn curl_installer_always_starts_the_local_capable_service() {
        // Git for Windows can check out this source fixture with CRLF. These
        // assertions cover the installer directives, not checkout line endings.
        let installer = include_str!("../../../edge/src/install.sh").replace("\r\n", "\n");
        assert!(!installer.contains("session.json"));
        assert!(installer.contains("StartLimitIntervalSec=60\n"));
        assert!(installer.contains("StartLimitBurst=5\n"));
        assert!(installer.contains("systemctl --user enable zerun"));
        assert!(installer.contains("systemctl --user restart zerun"));
    }

    #[test]
    fn installed_exe_uses_the_current_symlink() {
        // Installer-managed binary (current_exe resolves the `current` symlink to
        // the versioned dir): the unit must point back at the symlink.
        assert_eq!(
            exec_path_for(
                Path::new("/home/u/.zerun/app/0.3.0/zerun"),
                Some(Path::new("/home/u")),
            ),
            "%h/.zerun/app/current/zerun"
        );
        // Source build: literal path.
        assert_eq!(
            exec_path_for(
                Path::new("/src/target/debug/zerun"),
                Some(Path::new("/home/u"))
            ),
            "/src/target/debug/zerun"
        );
    }

    #[test]
    fn launchd_uses_current_for_managed_cli_and_preserves_other_executables() {
        let home = Path::new("/home/test user");
        assert_eq!(
            launchd_exec_path(&home.join(".zerun/app/0.3.0/zerun"), Some(home)),
            home.join(".zerun/app/current/zerun"),
        );
        for path in [
            "/Applications/Zerun.app/Contents/MacOS/zerun",
            "/src/target/debug/zerun",
        ] {
            assert_eq!(
                launchd_exec_path(Path::new(path), Some(home)),
                Path::new(path)
            );
        }
        assert_eq!(
            launchd_exec_path(Path::new("/src/zerun"), None),
            Path::new("/src/zerun")
        );
    }

    #[cfg(unix)]
    #[test]
    fn launchd_follows_the_cli_symlink_to_the_managed_install() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let app_root = home.join(".zerun/app");
        std::fs::create_dir_all(app_root.join("0.3.0")).unwrap();
        std::fs::write(app_root.join("0.3.0/zerun"), "").unwrap();
        std::os::unix::fs::symlink(app_root.join("0.3.0"), app_root.join("current")).unwrap();
        let bin = home.join(".local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(app_root.join("current/zerun"), bin.join("zerun")).unwrap();
        assert_eq!(
            launchd_exec_path(&bin.join("zerun"), Some(&home)),
            app_root.join("current/zerun"),
        );
        // Another executable reached through a symlink keeps its own path.
        let source = tmp.path().join("target/debug/zerun");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, "").unwrap();
        std::os::unix::fs::symlink(&source, bin.join("zerun-dev")).unwrap();
        assert_eq!(
            launchd_exec_path(&bin.join("zerun-dev"), Some(&home)),
            bin.join("zerun-dev"),
        );
    }

    #[test]
    fn install_skips_only_an_engine_that_is_not_the_launch_agent() {
        // Nothing running, or the agent's own engine (a reinstall).
        assert_eq!(foreign_engine(None, None), None);
        assert_eq!(foreign_engine(None, Some("41")), None);
        assert_eq!(foreign_engine(Some("41"), Some("41")), None);
        // The desktop app's engine, with or without a loaded agent.
        assert_eq!(foreign_engine(Some("42"), None), Some("42"));
        assert_eq!(foreign_engine(Some("42"), Some("41")), Some("42"));
        assert_eq!(foreign_engine(Some("unknown"), Some("41")), Some("unknown"));
    }

    #[test]
    fn launchd_print_pid_reads_only_a_running_job() {
        let running = "gui/501/work.puqing.zerun = {\n\tactive count = 1\n\tstate = running\n\n\tpid = 4242\n\tlast exit code = 0\n}\n";
        assert_eq!(launchd_print_pid(running), Some("4242"));
        let waiting =
            "gui/501/work.puqing.zerun = {\n\tstate = not running\n\tlast exit code = 1\n}\n";
        assert_eq!(launchd_print_pid(waiting), None);
        assert_eq!(launchd_print_pid("\tpid = \n"), None);
    }

    #[test]
    fn curl_installer_honours_the_engine_already_running_exit() {
        let installer = include_str!("../../../edge/src/install.sh").replace("\r\n", "\n");
        assert!(installer.contains(&format!(
            "{ENGINE_ALREADY_RUNNING_EXIT}) service=engine_running"
        )));
    }

    #[test]
    fn launchd_plist_shape() {
        let plist = render_launchd_plist(
            Path::new("/Users/x/zerun & co/zerun"),
            &[("ZERON_EDGE_URL".into(), "https://e?a=1&b=2".into())],
            Path::new("/Users/x/.zerun/daemon.log"),
        );
        assert!(plist.contains("<key>Label</key><string>work.puqing.zerun</string>"));
        // XML-escaped exe path and env value.
        assert!(plist.contains("<string>/Users/x/zerun &amp; co/zerun</string>"));
        assert!(plist.contains("<string>https://e?a=1&amp;b=2</string>"));
        assert!(plist.contains("<string>headless</string>"));
        assert!(plist.contains("<key>SuccessfulExit</key><false/>"));
        assert!(
            plist.contains("<key>StandardOutPath</key><string>/Users/x/.zerun/daemon.log</string>")
        );
    }
}
