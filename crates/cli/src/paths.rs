//! Application storage paths, shared with the GUI and managed engine.

use std::{ffi::OsString, path::PathBuf};

pub fn data_dir() -> PathBuf {
    resolve_data_dir(|key| std::env::var_os(key))
}

/// Resolve the fork's storage without adopting another application's directory.
pub fn resolve_data_dir(env: impl FnMut(&str) -> Option<OsString>) -> PathBuf {
    zeron_proto::identity::resolve_data_dir(std::env::consts::OS, env)
        .expect("User storage unavailable; set ZERUN_DATA_DIR")
}
