//! Mobile-owned commands. Provider commands retain their own routing; the
//! `zeron:` namespace is reserved for explicit local actions across providers.
use super::types::{ComposerCatalog, ComposerCommand};
use zeron_proto::{HarnessId, invocation::Invocation};

const LOCAL_COMMANDS: &[&str] = &[
    "model",
    "plan",
    "permissions",
    "new",
    "resume",
    "rename",
    "archive",
    "copy",
    "side",
    "fork",
    "theme",
];

fn known_harness(harness: &str) -> Option<HarnessId> {
    serde_json::from_value(serde_json::Value::String(harness.to_owned())).ok()
}

/// Keep the host's native catalog and skill identities intact. Codex already
/// advertises its mapped client commands; other agents receive explicit Zerun
/// actions rather than having their native /model, /clear or /fork intercepted.
pub(super) fn with_mobile_actions(mut catalog: ComposerCatalog, harness: &str) -> ComposerCatalog {
    match known_harness(harness) {
        None | Some(HarnessId::Codex) => return catalog,
        Some(_) => {}
    }
    for name in LOCAL_COMMANDS {
        let name = format!("zeron:{name}");
        let token = format!("/{name}");
        if catalog
            .commands
            .iter()
            .any(|command| command.token == token)
        {
            continue;
        }
        let insertion = Invocation::Command { name }.link();
        let local = mobile_composer_action(harness.to_owned(), insertion.clone())
            .expect("registered local command");
        catalog.commands.push(ComposerCommand {
            token,
            description: local.description,
            input_hint: None,
            insertion,
            unavailable_reason: Some("Use this action in Zerun's mobile / menu.".into()),
        });
    }
    catalog
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileComposerAction {
    Model,
    New,
    Search,
    Rename,
    Archive,
    Copy,
    Side,
    Fork,
    Appearance,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileComposerContext {
    pub in_session: bool,
    pub is_child: bool,
    pub can_fork: bool,
    pub has_reply: bool,
    pub busy: bool,
    pub editing: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileComposerLocalCommand {
    pub action: MobileComposerAction,
    pub description: String,
    pub has_arguments: bool,
}

#[uniffi::export]
pub fn mobile_composer_action(harness: String, text: String) -> Option<MobileComposerLocalCommand> {
    use zeron_proto::invocation::{
        Invocation, invocation_links, invocation_prompt, leading_command,
    };
    known_harness(&harness)?;
    if invocation_links(&text)
        .iter()
        .any(|(_, i)| matches!(i, Invocation::Skill { .. }))
    {
        return None;
    }
    let decoded = invocation_prompt(&text);
    let (name, args) = leading_command(&decoded)?;
    let (name, local_namespace) = match name.strip_prefix("zeron:") {
        Some(name) if LOCAL_COMMANDS.contains(&name) => (name, true),
        _ if harness == "codex" || matches!(name, "plan" | "permissions" | "approvals") => {
            (name, false)
        }
        _ => return None,
    };
    use MobileComposerAction::*;
    let (action, description) = match name {
        "model" => (Model, "Choose a model"),
        "plan" | "permissions" | "approvals" => {
            (Model, "Configure permissions and Plan in the composer")
        }
        "new" | "clear" => (New, "Start a new conversation"),
        "resume" | "agents" => (Search, "Search conversations"),
        "rename" => (Rename, "Rename this conversation"),
        "archive" => (Archive, "Archive this conversation"),
        "copy" => (Copy, "Copy the latest completed assistant reply"),
        "side" | "btw" => (Side, "Start an empty side chat"),
        "fork" => (Fork, "Fork from the last completed reply"),
        "theme" => (Appearance, "Open appearance settings"),
        _ => return None,
    };
    Some(MobileComposerLocalCommand {
        action,
        description: if local_namespace {
            format!("Zerun: {description}")
        } else {
            description.into()
        },
        has_arguments: !args.is_empty(),
    })
}

#[uniffi::export]
pub fn mobile_composer_action_error(
    action: MobileComposerAction,
    context: MobileComposerContext,
) -> Option<String> {
    use MobileComposerAction::*;
    let reason = if context.editing {
        Some("Finish editing the queued message before using this action.")
    } else if !context.in_session && matches!(action, Rename | Archive | Copy | Side | Fork) {
        Some("Start a conversation before using this action.")
    } else if action == Copy && !context.has_reply {
        Some("Needs a completed assistant reply.")
    } else if matches!(action, Side | Fork) && context.busy {
        Some("A side chat operation is already in progress.")
    } else if action == Side && context.is_child {
        Some("Create a side chat from the main conversation.")
    } else if action == Fork && !context.can_fork {
        Some("Needs a completed reply before forking.")
    } else {
        None
    };
    reason.map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context() -> MobileComposerContext {
        MobileComposerContext {
            in_session: true,
            is_child: false,
            can_fork: true,
            has_reply: true,
            busy: false,
            editing: false,
        }
    }
    #[test]
    fn resolves_only_codex_commands_and_preserves_arguments() {
        for name in [
            "model", "new", "clear", "resume", "agents", "rename", "archive", "copy", "side",
            "btw", "fork", "theme",
        ] {
            let link = zeron_proto::invocation::Invocation::Command { name: name.into() }.link();
            assert!(mobile_composer_action("codex".into(), link.clone()).is_some());
            assert!(mobile_composer_action("claude-code".into(), link).is_none());
        }
        for text in [
            "/goal",
            "/fast",
            "/logout",
            "/quit",
            "explain /model",
            "    /model",
            "`/model`",
            "$model",
        ] {
            assert!(
                mobile_composer_action("codex".into(), text.into()).is_none(),
                "{text}"
            );
        }
        let skill = zeron_proto::invocation::Invocation::Skill {
            name: "model".into(),
            path: "/tmp/model/SKILL.md".into(),
            command: None,
        }
        .link();
        assert!(mobile_composer_action("codex".into(), skill).is_none());
        assert!(
            mobile_composer_action("codex".into(), "/rename title".into())
                .unwrap()
                .has_arguments
        );
    }
    const PROVIDERS: &[&str] = &[
        "claude-code",
        "codex",
        "copilot",
        "cursor",
        "devin",
        "grok",
        "hermes",
        "pi",
        "omp",
        "opencode",
        "antigravity",
        "dsh",
        "mock",
    ];

    fn command(name: &str) -> ComposerCommand {
        ComposerCommand {
            token: format!("/{name}"),
            description: "Provider command".into(),
            input_hint: Some("args".into()),
            insertion: Invocation::Command { name: name.into() }.link(),
            unavailable_reason: None,
        }
    }

    #[test]
    fn explicit_local_commands_work_across_all_supported_agents() {
        for harness in PROVIDERS {
            for name in LOCAL_COMMANDS {
                let text = format!("/zeron:{name}");
                let row = mobile_composer_action((*harness).into(), text).unwrap();
                assert!(!row.has_arguments);
                assert!(row.description.starts_with("Zerun: "));
                assert!(
                    mobile_composer_action(
                        (*harness).into(),
                        command(&format!("zeron:{name}")).insertion
                    )
                    .is_some()
                );
                let skill = Invocation::Skill {
                    name: format!("zeron:{name}"),
                    path: "/project/SKILL.md".into(),
                    command: None,
                };
                assert!(mobile_composer_action((*harness).into(), skill.link()).is_none());
                if *harness != "codex" && !matches!(*name, "plan" | "permissions") {
                    assert!(
                        mobile_composer_action((*harness).into(), format!("/{name}")).is_none()
                    );
                }
            }
            assert!(
                mobile_composer_action((*harness).into(), "/zeron:rename title".into())
                    .unwrap()
                    .has_arguments
            );
            for name in [
                "compact",
                "review",
                "name",
                "session",
                "export",
                "security",
                "zeron:unknown",
            ] {
                assert!(
                    mobile_composer_action((*harness).into(), command(name).insertion).is_none(),
                    "{harness}/{name}"
                );
            }
        }
        assert!(mobile_composer_action("unknown-agent".into(), "/zeron:model".into()).is_none());
    }

    #[test]
    fn execution_shortcuts_open_the_shared_controls_and_do_not_send_native_mode_changes() {
        for harness in PROVIDERS {
            for name in ["plan", "permissions", "approvals"] {
                let local = mobile_composer_action((*harness).into(), format!("/{name}")).unwrap();
                assert_eq!(local.action, MobileComposerAction::Model);
                assert!(!local.has_arguments);
            }
        }
    }

    #[test]
    fn catalog_keeps_native_commands_skills_hints_and_warnings() {
        for harness in PROVIDERS.iter().filter(|h| **h != "codex") {
            let native = command("model");
            let unavailable = ComposerCommand {
                unavailable_reason: Some("Provider unavailable".into()),
                ..command("fork")
            };
            let skill = ComposerCommand {
                token: "$model".into(),
                insertion: Invocation::Skill {
                    name: "model".into(),
                    path: "/project/SKILL.md".into(),
                    command: None,
                }
                .link(),
                ..command("model")
            };
            let original = vec![native, unavailable, skill];
            let catalog = with_mobile_actions(
                ComposerCatalog {
                    commands: original.clone(),
                    warning: Some("Skills discovery failed".into()),
                },
                harness,
            );
            assert_eq!(
                catalog.commands.len(),
                original.len() + LOCAL_COMMANDS.len()
            );
            assert_eq!(catalog.warning.as_deref(), Some("Skills discovery failed"));
            for (before, after) in original.iter().zip(&catalog.commands) {
                assert_eq!(before.token, after.token);
                assert_eq!(before.insertion, after.insertion);
                assert_eq!(before.description, after.description);
                assert_eq!(before.input_hint, after.input_hint);
                assert_eq!(before.unavailable_reason, after.unavailable_reason);
                assert!(
                    mobile_composer_action((*harness).into(), after.insertion.clone()).is_none()
                );
            }
            let twice = with_mobile_actions(catalog, harness);
            assert_eq!(twice.commands.len(), original.len() + LOCAL_COMMANDS.len());
        }
    }

    #[test]
    fn empty_catalog_still_provides_local_actions_without_fabricating_provider_commands() {
        for harness in ["cursor", "dsh", "claude-code"] {
            let catalog = with_mobile_actions(
                ComposerCatalog {
                    commands: vec![],
                    warning: Some("Host offline".into()),
                },
                harness,
            );
            assert_eq!(catalog.commands.len(), LOCAL_COMMANDS.len());
            assert!(
                catalog
                    .commands
                    .iter()
                    .all(|c| c.token.starts_with("/zeron:"))
            );
            assert_eq!(catalog.warning.as_deref(), Some("Host offline"));
        }
        for harness in ["codex", "unknown-agent"] {
            let catalog = with_mobile_actions(
                ComposerCatalog {
                    commands: vec![command("goal")],
                    warning: None,
                },
                harness,
            );
            assert_eq!(catalog.commands.len(), 1);
        }
    }

    #[test]
    fn checks_page_and_live_context() {
        let mut c = context();
        assert!(mobile_composer_action_error(MobileComposerAction::Fork, c.clone()).is_none());
        c.in_session = false;
        assert!(mobile_composer_action_error(MobileComposerAction::Rename, c.clone()).is_some());
        assert!(mobile_composer_action_error(MobileComposerAction::Model, c.clone()).is_none());
        c = context();
        c.is_child = true;
        assert!(mobile_composer_action_error(MobileComposerAction::Side, c.clone()).is_some());
        assert!(mobile_composer_action_error(MobileComposerAction::Fork, c.clone()).is_none());
        c.busy = true;
        assert!(mobile_composer_action_error(MobileComposerAction::Fork, c.clone()).is_some());
        c = context();
        c.has_reply = false;
        c.can_fork = false;
        assert!(mobile_composer_action_error(MobileComposerAction::Copy, c.clone()).is_some());
        assert!(mobile_composer_action_error(MobileComposerAction::Fork, c.clone()).is_some());
        c.editing = true;
        assert!(mobile_composer_action_error(MobileComposerAction::Model, c).is_some());
    }
}
