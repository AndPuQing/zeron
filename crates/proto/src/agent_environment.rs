//! Device-local provider configuration. Listings deliberately carry no values.
use crate::HarnessId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EnvironmentAction {
    Set,
    Unset,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentEntryMetadata {
    pub name: String,
    pub action: EnvironmentAction,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentPolicy {
    pub max_name_bytes: usize,
    pub max_value_bytes: usize,
    pub max_entries: usize,
    pub max_payload_bytes: usize,
    pub case_sensitive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessEnvironmentMetadata {
    pub revision: String,
    pub entries: Vec<EnvironmentEntryMetadata>,
    pub policy: EnvironmentPolicy,
    #[serde(default)]
    pub previous_environment_sessions: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessEnvironmentParams {
    pub harness: HarnessId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_device_id: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum EnvironmentChange {
    Set { name: String, value: String },
    Unset { name: String },
    Delete { name: String },
}
impl EnvironmentChange {
    pub fn name(&self) -> &str {
        match self {
            Self::Set { name, .. } | Self::Unset { name } | Self::Delete { name } => name,
        }
    }
}
impl std::fmt::Debug for EnvironmentChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Set { name, .. } => f
                .debug_struct("Set")
                .field("name", name)
                .finish_non_exhaustive(),
            Self::Unset { name } => f.debug_tuple("Unset").field(name).finish(),
            Self::Delete { name } => f.debug_tuple("Delete").field(name).finish(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchHarnessEnvironmentParams {
    pub harness: HarnessId,
    pub expected_revision: String,
    pub changes: Vec<EnvironmentChange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_device_id: Option<String>,
}

/// A conflict returns fresh metadata so the editor can preserve its draft.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchHarnessEnvironmentReply {
    pub conflict: bool,
    pub metadata: HarnessEnvironmentMetadata,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevealHarnessEnvironmentParams {
    pub harness: HarnessId,
    pub name: String,
    pub expected_revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_device_id: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevealedEnvironmentValue {
    pub revision: String,
    pub value: String,
}
impl std::fmt::Debug for RevealedEnvironmentValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RevealedEnvironmentValue")
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wire_operations_distinguish_empty_unset_and_delete() {
        let set: EnvironmentChange =
            serde_json::from_value(serde_json::json!({"action":"set","name":"API_KEY","value":""}))
                .unwrap();
        assert_eq!(
            set,
            EnvironmentChange::Set {
                name: "API_KEY".into(),
                value: "".into(),
            }
        );
        for action in ["unset", "delete"] {
            let operation: EnvironmentChange =
                serde_json::from_value(serde_json::json!({"action":action,"name":"API_KEY"}))
                    .unwrap();
            assert_eq!(serde_json::to_value(operation).unwrap()["action"], action);
        }
    }
}
