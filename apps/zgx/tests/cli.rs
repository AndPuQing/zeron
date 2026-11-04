//! This app's `zeron` is the Zerun app's command line (crates/cli), as
//! production's `zeron` is (apps/zeron/tests/cli.rs runs the same checks
//! against that binary): every subcommand's help as the library renders it,
//! the version, and the engine's subcommands handed to the engine helper
//! beside the binary with the same arguments, exit status passed through.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A scratch folder of this test's own.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zgx-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The binary as `zeron` in `dir` (a hard link: it runs from there, so the
/// helper beside it is the one it finds, and clap names it `zeron`).
fn zeron_in(dir: &Path) -> PathBuf {
    let link = dir.join("zerun");
    std::fs::hard_link(env!("CARGO_BIN_EXE_zerun-experimental"), &link)
        .or_else(|_| std::fs::copy(env!("CARGO_BIN_EXE_zerun-experimental"), &link).map(|_| ()))
        .unwrap();
    link
}

#[test]
fn every_subcommand_s_help_is_the_shared_command_line_s() {
    let dir = scratch("help");
    let zeron = zeron_in(&dir);
    let version = env!("ZERON_APP_VERSION");
    for path in zeron_cli::subcommand_paths(version) {
        let out = Command::new(&zeron)
            .args(&path)
            .arg("--help")
            .env("ZERUN_DATA_DIR", &dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "{path:?}: {out:?}");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim_end(),
            zeron_cli::help_for(version, &path).trim_end(),
            "zeron {} --help",
            path.join(" ")
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn version_is_zeron_and_this_build_s() {
    let out = Command::new(env!("CARGO_BIN_EXE_zerun-experimental")).arg("--version").output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("zerun {}", env!("ZERON_APP_VERSION"))
    );
}

/// `zeron plugin list` runs in the helper: the same arguments, and its exit
/// status is the command's (exec: the helper is this process).
#[cfg(unix)]
#[test]
fn the_engine_s_subcommands_run_in_the_helper_beside_the_app() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = scratch("helper");
    let zeron = zeron_in(&dir);
    let heard = dir.join("heard");
    let helper = dir.join(if cfg!(target_os = "macos") { "Zerun Engine" } else { "zerun-engine" });
    std::fs::write(&helper, format!("#!/bin/sh\necho \"$@\" > '{}'\nexit 7\n", heard.display())).unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let status = Command::new(&zeron).args(["plugin", "list"]).env("ZERUN_DATA_DIR", &dir).status().unwrap();
    assert_eq!(status.code(), Some(7));
    assert_eq!(std::fs::read_to_string(&heard).unwrap().trim(), "plugin list");
    // Without a helper, it says so (and opens no window).
    std::fs::remove_file(&helper).unwrap();
    let out = Command::new(&zeron).arg("status").env("ZERUN_DATA_DIR", &dir).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("couldn't start the engine"), "{out:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn an_explicit_matching_engine_gets_only_the_experimental_profile() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = scratch("override");
    let app = zeron_in(&dir);
    let helper = dir.join("matching-engine");
    std::fs::write(&helper, "#!/bin/sh\nprintf '%s\\n' \"$*\" \"$ZERUN_DATA_DIR\" \"$ZERUN_IPC_PORT\" \"$ZERON_EDGE_URL\" \"$ZERUN_CALLBACK_PORT\" \"${ZERUN_MANAGED_ENGINE-unset}\" \"${ZERUN_APP-unset}\" \"${ZERON_IPC_TOKEN-unset}\" \"$ZERON_AUTO_UPDATE\"\nexit 7\n").unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let profile = dir.join("experimental");
    let out = Command::new(&app).arg("status")
        .env("ZERUN_ZGX_ENGINE_BIN", &helper)
        .env("ZERUN_ZGX_ENGINE_DATA", &profile)
        .env("ZERUN_ZGX_ENGINE_PORT", "27972")
        .env("ZERUN_ZGX_CALLBACK_PORT", "27973")
        .env("ZERUN_ZGX_EDGE", "https://edge.example.test")
        .env("ZERUN_DATA_DIR", dir.join("stable"))
        .env("ZERUN_IPC_PORT", "27971")
        .env("ZERUN_MANAGED_ENGINE", "1")
        .env("ZERUN_APP", "stable-app")
        .env("ZERON_IPC_TOKEN", "stable-token")
        .env("ZERON_AUTO_UPDATE", "1")
        .output().unwrap();
    assert_eq!(out.status.code(), Some(7), "{out:?}");
    let said = String::from_utf8(out.stdout).unwrap();
    assert_eq!(said.lines().collect::<Vec<_>>(), vec![
        "status", profile.to_str().unwrap(), "27972", "https://edge.example.test",
        "27973", "unset", "unset", "unset", "0",
    ]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_experimental_entry_refuses_stable_updates_and_service_installation() {
    for args in [vec!["update"], vec!["daemon", "install"]] {
        let out = Command::new(env!("CARGO_BIN_EXE_zerun-experimental")).args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&out.stderr).contains("updated by rebuilding it"), "{out:?}");
    }
}
