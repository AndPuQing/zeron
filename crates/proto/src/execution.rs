//! Execution policy shared by hosts and clients. Unsupported modes are errors,
//! never a request to fall back to a less restrictive mode.
use crate::{AccessMode, ChatConfig, RunRequest};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ExecutionPolicy {
    pub access: AccessMode,
    pub plan: bool,
}

impl ExecutionPolicy {
    pub fn needs_support(self) -> bool {
        self != Self::default()
    }
    /// Combine independently delivered config/command restrictions. A stale
    /// workspace snapshot must never relax a policy carried by the command.
    pub fn restricted_by(self, other: Self) -> Self {
        Self {
            access: if rank(self.access) <= rank(other.access) {
                self.access
            } else {
                other.access
            },
            plan: self.plan || other.plan,
        }
    }
    /// Every delegated task keeps at least the caller's restrictions.
    pub fn permits_child(self, child: Self) -> bool {
        rank(child.access) <= rank(self.access) && (!self.plan || child.plan)
    }
}

fn rank(mode: AccessMode) -> u8 {
    match mode {
        AccessMode::Supervised => 0,
        AccessMode::AutoAcceptEdits => 1,
        AccessMode::FullAccess => 2,
    }
}

impl ExecutionPolicy {
    pub fn validate(self, modes: &[AccessMode], supports_plan: bool) -> Result<(), String> {
        if !self.access.is_full_access() && !modes.contains(&self.access) {
            return Err(format!(
                "{} is not supported by this agent. Choose a supported mode explicitly.",
                self.access.label()
            ));
        }
        if self.plan && !supports_plan {
            return Err("This agent does not support Plan mode in Zerun.".into());
        }
        Ok(())
    }
}

// A command snapshot, not a second persisted configuration: ChatConfig and
// RunRequest keep dev's access/plan wire fields as the sole source of truth.
macro_rules! policy_accessors {
    ($type:ty) => {
        impl $type {
            pub fn execution_policy(&self) -> ExecutionPolicy {
                ExecutionPolicy {
                    access: self.access,
                    plan: self.plan,
                }
            }
            pub fn set_execution_policy(&mut self, policy: ExecutionPolicy) {
                self.access = policy.access;
                self.plan = policy.plan;
            }
        }
    };
}
policy_accessors!(ChatConfig);
policy_accessors!(RunRequest);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_full_access_and_plan_off_are_not_legacy_omissions() {
        let config = ChatConfig {
            harness: crate::HarnessId::Codex,
            model: None,
            reasoning: None,
            model_options: Default::default(),
            sandbox: crate::SandboxLevel::WorkspaceWrite,
            access: AccessMode::FullAccess,
            plan: false,
        };
        let value = serde_json::to_value(config).unwrap();
        assert_eq!(value["access"], "full-access");
        assert_eq!(value["plan"], false);
        assert!(value.get("execution").is_none());
    }

    #[test]
    fn old_records_keep_their_existing_behavior() {
        assert_eq!(
            serde_json::from_str::<ExecutionPolicy>("{}").unwrap(),
            ExecutionPolicy::default()
        );
        assert!(!ExecutionPolicy::default().needs_support());
    }
    #[test]
    fn unsupported_modes_never_become_full_access() {
        assert!(ExecutionPolicy::default().validate(&[], false).is_ok());
        assert!(
            ExecutionPolicy {
                access: AccessMode::Supervised,
                plan: false
            }
            .validate(&[], false)
            .is_err()
        );
        assert!(
            ExecutionPolicy {
                plan: true,
                ..Default::default()
            }
            .validate(&AccessMode::ALL, false)
            .is_err()
        );
    }

    #[test]
    fn command_and_workspace_order_cannot_relax_permissions() {
        let plan = ExecutionPolicy {
            plan: true,
            ..Default::default()
        };
        let supervised = ExecutionPolicy {
            access: AccessMode::Supervised,
            plan: false,
        };
        assert_eq!(
            plan.restricted_by(supervised),
            ExecutionPolicy {
                access: AccessMode::Supervised,
                plan: true
            }
        );
        assert_eq!(
            supervised.restricted_by(ExecutionPolicy::default()),
            supervised
        );
        assert_eq!(
            ExecutionPolicy::default().restricted_by(supervised),
            supervised
        );
    }
    #[test]
    fn delegation_cannot_escape_access_or_plan() {
        let parent = ExecutionPolicy {
            access: AccessMode::Supervised,
            plan: true,
        };
        assert!(parent.permits_child(parent));
        assert!(!parent.permits_child(ExecutionPolicy {
            plan: false,
            ..parent
        }));
        assert!(!parent.permits_child(ExecutionPolicy {
            access: AccessMode::FullAccess,
            ..parent
        }));
    }
}
