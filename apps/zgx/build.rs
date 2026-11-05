//! Stamp this independently built desktop with the root workspace version.
//! The internal environment names are also used by template metadata.

fn main() {
    // (Read as it runs, not with `env!`: a target dir shared between
    // checkouts can run a build script compiled in another, which then
    // read that checkout's manifest, or none once it was removed.)
    let dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let manifest = std::path::Path::new(&dir).join("../../Cargo.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let text = std::fs::read_to_string(&manifest).unwrap_or_default();
    let version = workspace_version(&text).unwrap_or_default();
    println!("cargo:rustc-env=ZERON_VERSION={version}");
    println!("cargo:rustc-env=ZERON_APP_VERSION={version}");
}

/// `version = "…"` under `[workspace.package]`.
fn workspace_version(toml: &str) -> Option<String> {
    let mut inside = false;
    for line in toml.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line == "[workspace.package]";
        } else if inside && let Some(rest) = line.strip_prefix("version") {
            let value = rest.trim_start().strip_prefix('=')?.trim();
            return Some(value.trim_matches('"').to_string());
        }
    }
    None
}
