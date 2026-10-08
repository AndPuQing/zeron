//! Application storage paths. Provider credentials keep their own locations.

use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    zeron_proto::identity::data_dir().expect("User storage unavailable; set ZERUN_DATA_DIR")
}
