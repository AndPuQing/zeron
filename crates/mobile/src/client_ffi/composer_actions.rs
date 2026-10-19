//! Mobile-owned commands. Resolve canonical commands, never same-name skills.
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
    if harness != "codex"
        || invocation_links(&text)
            .iter()
            .any(|(_, i)| matches!(i, Invocation::Skill { .. }))
    {
        return None;
    }
    let decoded = invocation_prompt(&text);
    let (name, args) = leading_command(&decoded)?;
    use MobileComposerAction::*;
    let (action, description) = match name {
        "model" => (Model, "Choose a model"),
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
        description: description.into(),
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
