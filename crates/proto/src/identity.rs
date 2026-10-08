//! Desktop installation identity, shared by GUI, CLI, MCP and managed tools.

use std::{ffi::OsString, path::PathBuf};

pub const BUNDLE_ID: &str = "work.puqing.zerun";
pub const DEFAULT_IPC_PORT: u16 = 27655;
pub const DEFAULT_CALLBACK_PORT: u16 = 27642;

pub fn data_dir() -> Option<PathBuf> {
    resolve_data_dir(std::env::consts::OS, |name| std::env::var_os(name))
}

/// Resolve without touching disk. Existing directories belonging to another
/// application are never adopted or renamed.
pub fn resolve_data_dir(
    os: &str,
    mut env: impl FnMut(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    let mut value = |name: &str| env(name).filter(|value| !value.is_empty());
    if let Some(dir) = value("ZERUN_DATA_DIR") {
        return Some(PathBuf::from(dir));
    }
    if os == "windows" {
        value("LOCALAPPDATA")
            .map(PathBuf::from)
            .or_else(|| {
                value("USERPROFILE").map(|home| PathBuf::from(home).join("AppData").join("Local"))
            })
            .map(|local| local.join("Zerun"))
    } else {
        value("HOME").map(|home| PathBuf::from(home).join(".zerun"))
    }
}

pub fn ipc_port() -> u16 {
    port("ZERUN_IPC_PORT", DEFAULT_IPC_PORT)
}

pub fn callback_port() -> u16 {
    port("ZERUN_CALLBACK_PORT", DEFAULT_CALLBACK_PORT)
}

fn port(name: &str, fallback: u16) -> u16 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(os: &str, vars: &[(&str, &str)]) -> Option<PathBuf> {
        resolve_data_dir(os, |name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.into())
        })
    }

    #[test]
    fn unix_instances_ignore_other_app_overrides() {
        for os in ["linux", "macos"] {
            assert_eq!(
                resolve(os, &[("HOME", "/home/test"), ("ZERON_DATA_DIR", "/shared")]),
                Some(PathBuf::from("/home/test/.zerun"))
            );
        }
    }

    #[test]
    fn explicit_root_is_shared_by_all_desktop_entry_points() {
        for os in ["linux", "macos", "windows"] {
            assert_eq!(
                resolve(os, &[("ZERUN_DATA_DIR", "custom root")]),
                Some("custom root".into())
            );
        }
    }

    #[test]
    fn windows_uses_native_user_storage_even_with_a_shell_home() {
        assert_eq!(
            resolve(
                "windows",
                &[("LOCALAPPDATA", "C:/Local"), ("HOME", "D:/shell")]
            ),
            Some(PathBuf::from("C:/Local").join("Zerun"))
        );
        assert_eq!(
            resolve("windows", &[("USERPROFILE", "C:/Users/Test User")]),
            Some(PathBuf::from("C:/Users/Test User").join("AppData/Local/Zerun"))
        );
        assert_eq!(resolve("windows", &[("HOME", "D:/shell")]), None);
    }
}
