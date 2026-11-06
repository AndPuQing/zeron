//! Keep ACP discovery sessions and their indexes out of dsh's shared history.

use crate::HarnessError;
use crate::environment::{EnvironmentEntry, EnvironmentSnapshot};
use crate::process::Command;
use crate::scratch::ScratchDir;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use zeron_proto::Model;

/// Discovery needs `session/new` for models and commands. dsh persists that
/// session before replying, including its workspace and projection records.
/// Apply a final, process-local overlay to the standard base-profile storage
/// entries. Keep DSH_HOME intact so profiles, credentials and model settings
/// resolve exactly as they do for real runs.
pub(super) fn configure(command: &mut Command, scratch: &ScratchDir) -> Result<(), HarnessError> {
    let patches = json!([
        {
            "id": "session-persistence-jsonl",
            "config": { "root": scratch.path().join("sessions") }
        },
        {
            "id": "storage-json",
            "config": { "root": scratch.path().join("storages") }
        },
        {
            "id": "storage-domain",
            "config": { "backend": "json" }
        },
        {
            "id": "session-query-sqlite",
            "config": { "path": ":memory:", "openAt": "never" }
        }
    ]);
    let overlay = scratch.path().join("discovery.patch.yml");
    // JSON is valid YAML; the serializer also quotes Windows paths safely.
    let bytes = serde_json::to_vec(&patches)
        .map_err(|error| HarnessError::Protocol(format!("dsh discovery overlay: {error}")))?;
    std::fs::write(&overlay, bytes)?;
    command.arg("--patch").arg(overlay);
    Ok(())
}

/// The dsh data home this harness launches with: a `DSH_HOME` set through
/// Zerun's provider environment wins over the process environment, matching
/// the child that receives the snapshot.
pub(super) fn data_home(environment: &EnvironmentSnapshot) -> PathBuf {
    match environment
        .entries
        .iter()
        .find(|(name, _)| crate::environment::normalized_name(name) == "DSH_HOME")
        .map(|(_, entry)| entry)
    {
        Some(EnvironmentEntry::Set { value }) if !value.is_empty() => PathBuf::from(value),
        Some(EnvironmentEntry::Unset | EnvironmentEntry::Set { .. }) => {
            crate::executable::home_or_current_dir().join(".dsh")
        }
        _ => super::dsh_home(),
    }
}

/// dsh's ACP adapter derives model names from its live provider catalog. When
/// that catalog cannot resolve a route, it falls back to the selected model id,
/// so the picker shows `group/deepseek-flash` even though the profile names it
/// `DeepSeek V4.1 Flash (group)`. Prefer the configured name, then a readable
/// form of the id, for rows whose advertised label is only an id.
pub(super) fn enrich_model_labels(models: &mut [Model], home: &Path, profile: &str) {
    let configured = configured_model_names(home, profile);
    apply_configured_names(models, &configured);
}

fn apply_configured_names(models: &mut [Model], configured: &HashMap<(String, String), String>) {
    for model in models {
        let Some((provider, id)) = zeron_proto::split_provider_model(&model.id) else {
            continue;
        };
        // A real display name always wins: only replace labels that repeat the
        // tuple, the model slug, or another raw tuple.
        let raw = model.label.trim().is_empty()
            || model.label == model.id
            || model.label == id
            || zeron_proto::split_provider_model(&model.label).is_some();
        if !raw {
            continue;
        }
        model.label = configured
            .get(&(provider, id))
            .cloned()
            .unwrap_or_else(|| zeron_proto::display_model_name(&model.id));
    }
}

/// Model display names from the user's dsh layers. The profile layer is applied
/// after the home layer, so its name wins when both define the same route.
fn configured_model_names(home: &Path, profile: &str) -> HashMap<(String, String), String> {
    let profile = home.join("profiles").join(profile);
    let mut names = HashMap::new();
    for path in [
        home.join("cordis.patch.yml"),
        profile.join("cordis.patch.yml"),
    ] {
        merge_model_names(&mut names, &path);
    }
    names
}

fn merge_model_names(names: &mut HashMap<(String, String), String>, path: &Path) {
    let Ok(bytes) = std::fs::read(path) else {
        return;
    };
    let Ok(document) = serde_yaml_ng::from_slice::<serde_yaml_ng::Value>(&bytes) else {
        return;
    };
    let Some(entries) = document.as_sequence() else {
        return;
    };
    for entry in entries {
        let Some(providers) = entry
            .get("config")
            .and_then(|config| config.get("providers"))
            .and_then(serde_yaml_ng::Value::as_mapping)
        else {
            continue;
        };
        for (provider, config) in providers {
            let Some(provider) = provider.as_str() else {
                continue;
            };
            let Some(models) = config
                .get("models")
                .and_then(serde_yaml_ng::Value::as_sequence)
            else {
                continue;
            };
            for model in models {
                let Some(id) = model.get("id").and_then(serde_yaml_ng::Value::as_str) else {
                    continue;
                };
                let Some(name) = model.get("name").and_then(serde_yaml_ng::Value::as_str) else {
                    continue;
                };
                let name = name.trim();
                if name.is_empty() || name == id {
                    continue;
                }
                names.insert((provider.to_owned(), id.to_owned()), name.to_owned());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeron_proto::ReasoningLevel;

    fn model(id: &str, label: &str) -> Model {
        Model {
            id: id.into(),
            label: label.into(),
            description: None,
            reasoning_levels: Vec::<ReasoningLevel>::new(),
            options: Vec::new(),
        }
    }

    #[test]
    fn configured_names_replace_labels_that_only_echo_ids() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let profile = home.join("profiles/acp-plus");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(
            home.join("cordis.patch.yml"),
            r#"
- id: llm-pi-ai
  config:
    providers:
      local:
        models:
          - id: group/deepseek-flash
            name: Home name
"#,
        )
        .unwrap();
        std::fs::write(
            profile.join("cordis.patch.yml"),
            r#"
- id: session-persistence-jsonl
  config:
    root: !!js dshHomePath('sessions')
- id: llm-pi-ai
  config:
    providers:
      local:
        models:
          - id: group/deepseek-flash
            name: DeepSeek V4.1 Flash (group)
          - id: group/unnamed
"#,
        )
        .unwrap();

        let mut models = vec![
            model(
                "[\"local\",\"group/deepseek-flash\"]",
                "group/deepseek-flash",
            ),
            model(
                "[\"deepseek-official\",\"deepseek-v4-pro\"]",
                "[\"deepseek-official\",\"deepseek-v4-pro\"]",
            ),
            model("[\"local\",\"group/unnamed\"]", ""),
            model(
                "[\"local\",\"group/deepseek-flash\"]",
                "DeepSeek V4.1 Flash (group)",
            ),
        ];
        enrich_model_labels(&mut models, home, "acp-plus");

        let labels: Vec<&str> = models.iter().map(|model| model.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "DeepSeek V4.1 Flash (group)",
                "DeepSeek V4 Pro",
                "Unnamed",
                "DeepSeek V4.1 Flash (group)",
            ]
        );
    }
}
