//! Known user-global settings paths. Discovery never reads or creates a file.
use crate::environment::{EnvironmentEntry, EnvironmentSnapshot, normalized_name};
use std::path::{Path, PathBuf};
use zeron_proto::{HarnessConfigFile, HarnessConfigFiles, HarnessId};

pub fn list(
    id: HarnessId,
    environment: &EnvironmentSnapshot,
) -> Result<HarnessConfigFiles, String> {
    let home = crate::executable::home_dir().ok_or("Cannot locate the host's home directory")?;
    let var = |name: &str| match environment.entries.get(&normalized_name(name)) {
        Some(EnvironmentEntry::Set { value }) => Some(value.clone()),
        Some(EnvironmentEntry::Unset) => None,
        None => std::env::var(name).ok(),
    };
    let paths = resolve(id, &home, &var, &crate::acp::dsh_profile())?;
    let note = match id {
        HarnessId::Cursor => Some(
            "Cursor CLI settings; Zerun's SDK sessions may override individual options.".into(),
        ),
        HarnessId::Devin => {
            Some("This provider does not expose a verified user configuration file.".into())
        }
        HarnessId::Muse | HarnessId::Mock => {
            Some("Configuration files are not available for this provider.".into())
        }
        _ => None,
    };
    Ok(HarnessConfigFiles {
        files: paths
            .into_iter()
            .map(|(label, path)| HarnessConfigFile {
                label: label.into(),
                exists: path.is_file(),
                path: path.to_string_lossy().into_owned(),
            })
            .collect(),
        note,
    })
}

fn resolve(
    id: HarnessId,
    home: &Path,
    var: &impl Fn(&str) -> Option<String>,
    dsh_profile: &str,
) -> Result<Vec<(&'static str, PathBuf)>, String> {
    let root = |name: &str, fallback: PathBuf| -> Result<PathBuf, String> {
        match var(name).filter(|value| !value.is_empty()) {
            Some(value) => {
                let path = PathBuf::from(value);
                if !path.is_absolute() {
                    return Err(format!(
                        "{name} is relative to the session folder. Use an absolute configuration path to open it here."
                    ));
                }
                Ok(path)
            }
            None => Ok(fallback),
        }
    };
    let settings = |path| vec![("Settings", path)];
    Ok(match id {
        HarnessId::ClaudeCode => {
            settings(root("CLAUDE_CONFIG_DIR", home.join(".claude"))?.join("settings.json"))
        }
        HarnessId::Codex => settings(root("CODEX_HOME", home.join(".codex"))?.join("config.toml")),
        HarnessId::Copilot => {
            settings(root("COPILOT_HOME", home.join(".copilot"))?.join("config.json"))
        }
        HarnessId::Grok => settings(root("GROK_HOME", home.join(".grok"))?.join("config.toml")),
        HarnessId::Hermes => {
            settings(root("HERMES_HOME", home.join(".hermes"))?.join("config.yaml"))
        }
        HarnessId::Fx => settings(home.join(".fx/settings.json")),
        HarnessId::Pi => {
            let root = root("PI_CODING_AGENT_DIR", home.join(".pi/agent"))?;
            vec![
                ("Settings", root.join("settings.json")),
                ("Models", root.join("models.json")),
            ]
        }
        HarnessId::Omp => {
            let root = root("PI_CODING_AGENT_DIR", home.join(".omp/agent"))?;
            vec![
                ("Settings", root.join("config.yml")),
                ("Models", root.join("models.yml")),
            ]
        }
        HarnessId::Opencode => {
            let config = root("XDG_CONFIG_HOME", home.join(".config"))?.join("opencode");
            let mut files = vec![
                ("Settings (JSON)", config.join("opencode.json")),
                ("Settings (JSONC)", config.join("opencode.jsonc")),
            ];
            if var("OPENCODE_CONFIG").is_some_and(|value| !value.is_empty()) {
                files.push((
                    "Custom settings",
                    root("OPENCODE_CONFIG", config.join("opencode.json"))?,
                ));
            }
            if var("OPENCODE_CONFIG_DIR").is_some_and(|value| !value.is_empty()) {
                let custom = root("OPENCODE_CONFIG_DIR", config)?;
                files.extend([
                    ("Custom directory (JSON)", custom.join("opencode.json")),
                    ("Custom directory (JSONC)", custom.join("opencode.jsonc")),
                ]);
            }
            files
        }
        HarnessId::Antigravity => {
            // The native adapter expands GEMINI_HOME before spawning.
            let configured = var("GEMINI_HOME").map(PathBuf::from);
            let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
            let child_cwd = std::env::var_os("HOME")
                .map(|home| cwd.join(home))
                .unwrap_or(cwd);
            let root = crate::acp::antigravity_paths::resolve_home(
                configured.as_deref(),
                Some(home),
                &child_cwd,
            )
            .map_err(|e| e.to_string())?;
            settings(root.join("antigravity-acp/settings.json"))
        }
        HarnessId::Dsh => {
            let root = root("DSH_HOME", home.join(".dsh"))?;
            vec![
                ("Global settings", root.join("cordis.patch.yml")),
                (
                    "ACP profile settings",
                    root.join("profiles")
                        .join(dsh_profile)
                        .join("cordis.patch.yml"),
                ),
            ]
        }
        HarnessId::Cursor => {
            let root = if cfg!(target_os = "linux")
                && var("XDG_CONFIG_HOME").is_some_and(|value| !value.is_empty())
            {
                root("XDG_CONFIG_HOME", home.join(".config"))?.join("cursor")
            } else {
                home.join(".cursor")
            };
            settings(root.join("cli-config.json"))
        }
        HarnessId::Devin | HarnessId::Muse | HarnessId::Mock => vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_follow_roots_without_exposing_credentials_or_creating_files() {
        let dir = tempfile::tempdir().unwrap();
        let override_root = dir.path().join("custom");
        for (id, variable, file) in [
            (HarnessId::Codex, "CODEX_HOME", "config.toml"),
            (HarnessId::ClaudeCode, "CLAUDE_CONFIG_DIR", "settings.json"),
            (HarnessId::Pi, "PI_CODING_AGENT_DIR", "settings.json"),
            (HarnessId::Hermes, "HERMES_HOME", "config.yaml"),
        ] {
            let paths = resolve(
                id,
                dir.path(),
                &|name| (name == variable).then(|| override_root.to_string_lossy().into_owned()),
                "acp",
            )
            .unwrap();
            assert_eq!(paths[0].1, override_root.join(file));
            assert!(!override_root.exists());
        }
        for id in HarnessId::ALL {
            let paths = resolve(id, dir.path(), &|_| None, "acp-plus").unwrap();
            assert!(
                paths
                    .iter()
                    .all(|(_, path)| !path.to_string_lossy().contains("credential")
                        && path.file_name().unwrap() != "auth.json")
            );
        }
        let dsh = resolve(HarnessId::Dsh, dir.path(), &|_| None, "acp-plus").unwrap();
        assert_eq!(
            dsh[1].1,
            dir.path().join(".dsh/profiles/acp-plus/cordis.patch.yml")
        );
    }
    #[test]
    fn relative_overrides_are_not_mistaken_for_host_global_paths() {
        assert!(
            resolve(
                HarnessId::Codex,
                Path::new("/home/test"),
                &|_| Some("relative".into()),
                "acp"
            )
            .is_err()
        );
    }
}
