//! Immutable, validated environment binding for one provider operation.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use zeron_proto::{
    EnvironmentAction, EnvironmentChange, EnvironmentEntryMetadata, EnvironmentPolicy,
    HarnessEnvironmentMetadata,
};

pub const INITIAL_REVISION: &str = "initial";
pub const MAX_NAME_BYTES: usize = 128;
pub const MAX_VALUE_BYTES: usize = 16 * 1024;
pub const MAX_ENTRIES: usize = 64;
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
pub enum EnvironmentEntry {
    Set { value: String },
    Unset,
}
impl std::fmt::Debug for EnvironmentEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Set { .. } => f.debug_struct("Set").finish_non_exhaustive(),
            Self::Unset => f.write_str("Unset"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnvironmentSnapshot {
    pub revision: String,
    pub entries: BTreeMap<String, EnvironmentEntry>,
}
impl Default for EnvironmentSnapshot {
    fn default() -> Self {
        Self {
            revision: INITIAL_REVISION.into(),
            entries: BTreeMap::new(),
        }
    }
}

pub fn normalized_name(name: &str) -> String {
    if cfg!(windows) {
        name.to_ascii_uppercase()
    } else {
        name.to_owned()
    }
}

pub fn validate_name(name: &str) -> Result<(), String> {
    let mut bytes = name.bytes();
    if name.len() > MAX_NAME_BYTES
        || !bytes
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        || !bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_')
    {
        return Err("Use a variable name beginning with a letter or underscore, with only letters, digits and underscores (maximum 128 bytes)".into());
    }
    // Case-independent policy also prevents accidental acceptance on Unix of
    // a name whose meaning changes when the same editor targets Windows.
    let upper = name.to_ascii_uppercase();
    let roots = [
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "CODEX_HOME",
        "CLAUDE_CONFIG_DIR",
        "PI_CODING_AGENT_DIR",
        "GROK_HOME",
        "HERMES_HOME",
        "HERMES_SHARED_AUTH_DIR",
        "GEMINI_HOME",
        "OPENCODE_CONFIG",
        "OPENCODE_CONFIG_DIR",
        "OPENCODE_CONFIG_CONTENT",
    ];
    if roots.contains(&upper.as_str()) || upper.starts_with("XDG_") {
        return Err(format!(
            "{name} selects an identity/configuration directory managed by the engine"
        ));
    }
    if upper.starts_with("ZERON_")
        || upper.starts_with("ZERUN_")
        || [
            "CLAUDE_CODE_EXECUTABLE",
            "CODEX_EXECUTABLE",
            "CURSOR_SDK_SHIM_EXECUTABLE",
            "GROK_EXECUTABLE",
            "DEVIN_EXECUTABLE",
            "HERMES_EXECUTABLE",
            "ANTIGRAVITY_ACP_EXECUTABLE",
            "OPENCODE_EXECUTABLE",
            "PI_EXECUTABLE",
            "CLAUDECODE",
            "CLAUDE_CODE_ENTRYPOINT",
            "CLAUDE_CODE_SSE_PORT",
            "CLAUDE_AGENT_SDK_VERSION",
            "ANTIGRAVITY_HARNESS_PATH",
            "AGY_ACP_FORCE_FILE_STORAGE",
            "BROWSER",
            "PYTHONUNBUFFERED",
            "OPENCODE_PASSWORD",
            "OPENCODE_SERVER_PASSWORD",
            "OPENCODE_CLIENT",
            "TMPDIR",
            "TMP",
            "TEMP",
        ]
        .contains(&upper.as_str())
    {
        return Err(format!("{name} is reserved for engine process controls"));
    }
    Ok(())
}

impl EnvironmentSnapshot {
    pub fn metadata(&self) -> HarnessEnvironmentMetadata {
        HarnessEnvironmentMetadata {
            previous_environment_sessions: Vec::new(),
            revision: self.revision.clone(),
            entries: self
                .entries
                .iter()
                .map(|(name, entry)| {
                    let action = match entry {
                        EnvironmentEntry::Set { .. } => EnvironmentAction::Set,
                        EnvironmentEntry::Unset => EnvironmentAction::Unset,
                    };
                    EnvironmentEntryMetadata {
                        name: name.clone(),
                        action,
                    }
                })
                .collect(),
            policy: EnvironmentPolicy {
                max_name_bytes: MAX_NAME_BYTES,
                max_value_bytes: MAX_VALUE_BYTES,
                max_entries: MAX_ENTRIES,
                max_payload_bytes: MAX_PAYLOAD_BYTES,
                case_sensitive: !cfg!(windows),
            },
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.revision.is_empty() || self.revision.len() > 128 {
            return Err("Invalid environment revision".into());
        }
        if self.entries.len() > MAX_ENTRIES {
            return Err("At most 64 environment variables are allowed per provider".into());
        }
        let mut names = HashSet::new();
        for (name, entry) in &self.entries {
            validate_name(name)?;
            if !names.insert(normalized_name(name)) {
                return Err(format!("Duplicate environment variable: {name}"));
            }
            if let EnvironmentEntry::Set { value, .. } = entry
                && (value.contains('\0') || value.len() > MAX_VALUE_BYTES)
            {
                return Err(format!(
                    "{name}: values must have no NUL and be at most 16 KiB"
                ));
            }
        }
        if serde_json::to_vec(&self.entries)
            .map_err(|_| "Could not encode environment configuration")?
            .len()
            > MAX_PAYLOAD_BYTES
        {
            return Err("Environment overrides must fit within 64 KiB".into());
        }
        Ok(())
    }

    pub fn patched(&self, changes: &[EnvironmentChange]) -> Result<Self, String> {
        if changes.len() > MAX_ENTRIES * 2 {
            return Err("Too many environment changes in one save".into());
        }
        let mut candidate = self.clone();
        let mut names = HashSet::new();
        for change in changes {
            let name = change.name();
            validate_name(name)?;
            let normalized = normalized_name(name);
            if !names.insert(normalized.clone()) {
                return Err(format!("Duplicate environment change: {name}"));
            }
            candidate
                .entries
                .retain(|existing, _| normalized_name(existing) != normalized);
            match change {
                EnvironmentChange::Set { value, .. } => {
                    candidate.entries.insert(
                        name.into(),
                        EnvironmentEntry::Set {
                            value: value.clone(),
                        },
                    );
                }
                EnvironmentChange::Unset { .. } => {
                    candidate
                        .entries
                        .insert(name.into(), EnvironmentEntry::Unset);
                }
                EnvironmentChange::Delete { .. } => {}
            }
        }
        candidate.validate()?;
        Ok(candidate)
    }

    /// Call after shell PATH composition, before engine-owned launch controls.
    pub fn apply(&self, command: &mut crate::process::Command) {
        for (name, entry) in &self.entries {
            match entry {
                EnvironmentEntry::Set { value, .. } => {
                    command.env(name, value);
                }
                EnvironmentEntry::Unset => {
                    command.env_remove(name);
                }
            }
        }
    }

    pub fn partition_context(&self, mut context: crate::ModelContext) -> crate::ModelContext {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        hash.update(context.hash.as_bytes());
        hash.update([0]);
        hash.update(self.revision.as_bytes());
        context.hash = format!("{:x}", hash.finalize());
        context
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_literal_values_and_atomic_patch() {
        let original = EnvironmentSnapshot::default();
        let value = "  中文\n$(echo hello) $HOME `literal`  ";
        let changed = original
            .patched(&[EnvironmentChange::Set {
                name: "EXAMPLE".into(),
                value: value.into(),
            }])
            .unwrap();
        assert!(original.entries.is_empty());
        assert!(!format!("{changed:?}").contains("echo hello"));
        assert!(
            !serde_json::to_string(&changed.metadata())
                .unwrap()
                .contains("literal")
        );
        for name in [
            "",
            "1BAD",
            "A=B",
            " A",
            "HOME",
            "CODEX_HOME",
            "ZERUN_TEST",
            "CLAUDECODE",
        ] {
            assert!(validate_name(name).is_err(), "{name}");
        }
        assert!(
            changed
                .patched(&[EnvironmentChange::Set {
                    name: "A".into(),
                    value: "\0".into(),
                }])
                .is_err()
        );
        assert!(
            changed
                .patched(&[
                    EnvironmentChange::Unset { name: "A".into() },
                    EnvironmentChange::Delete { name: "A".into() }
                ])
                .is_err()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn actual_child_observes_overrides_without_changing_parent() {
        async fn observe(snapshot: EnvironmentSnapshot) -> serde_json::Value {
            let mut command = crate::process::Command::new("/usr/bin/env");
            command.env("ENV_INHERITED_FIXTURE", "inherited");
            snapshot.apply(&mut command);
            let output = command.output().await.unwrap();
            assert!(output.status.success());
            let text = String::from_utf8(output.stdout).unwrap();
            serde_json::Value::Object(
                text.lines()
                    .filter_map(|line| line.split_once('='))
                    .map(|(k, v)| (k.to_owned(), v.into()))
                    .collect(),
            )
        }
        let base = EnvironmentSnapshot::default();
        let one = base
            .patched(&[
                EnvironmentChange::Set {
                    name: "ENV_PROVIDER_FIXTURE".into(),
                    value: "one $HOME".into(),
                },
                EnvironmentChange::Set {
                    name: "ENV_EMPTY_FIXTURE".into(),
                    value: "".into(),
                },
                EnvironmentChange::Unset {
                    name: "ENV_INHERITED_FIXTURE".into(),
                },
            ])
            .unwrap();
        let two = base
            .patched(&[EnvironmentChange::Set {
                name: "ENV_PROVIDER_FIXTURE".into(),
                value: "two".into(),
            }])
            .unwrap();
        let (a, b) = tokio::join!(observe(one.clone()), observe(two));
        assert_eq!(a["ENV_PROVIDER_FIXTURE"], "one $HOME");
        assert_eq!(b["ENV_PROVIDER_FIXTURE"], "two");
        assert_eq!(a["ENV_EMPTY_FIXTURE"], "");
        assert!(a.get("ENV_INHERITED_FIXTURE").is_none());
        let restored = one
            .patched(&[EnvironmentChange::Delete {
                name: "ENV_INHERITED_FIXTURE".into(),
            }])
            .unwrap();
        assert_eq!(
            observe(restored).await["ENV_INHERITED_FIXTURE"],
            "inherited"
        );
        assert!(std::env::var_os("ENV_PROVIDER_FIXTURE").is_none());
    }
}
