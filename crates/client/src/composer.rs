//! Host-backed mobile composer catalog. No CLI catalog is duplicated on phones.
use crate::error::{ClientError, Result};
use zeron_proto::{
    SlashCommand,
    invocation::{
        Invocation, Skill, valid_invocation_name, valid_skill_command_name, valid_skill_path,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerCommand {
    pub token: String,
    pub description: String,
    pub input_hint: Option<String>,
    pub insertion: String,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ComposerCatalog {
    pub commands: Vec<ComposerCommand>,
    pub warning: Option<String>,
}

pub(crate) fn from_replies(
    commands: Result<serde_json::Value>,
    skills: Result<serde_json::Value>,
) -> ComposerCatalog {
    let mut warnings = Vec::new();
    let commands =
        decode::<Vec<SlashCommand>>(commands, "Commands", &mut warnings).unwrap_or_default();
    let skills = decode::<Option<Vec<Skill>>>(skills, "Skills", &mut warnings)
        .flatten()
        .unwrap_or_default();
    let skills: Vec<_> = skills
        .into_iter()
        .filter(|s| {
            valid_invocation_name(&s.name)
                && valid_skill_path(&s.path)
                && s.command
                    .as_ref()
                    .is_none_or(|c| valid_skill_command_name(&c.name))
        })
        .collect();
    let mut rows: Vec<_> = commands
        .into_iter()
        .filter(|c| {
            valid_skill_command_name(&c.name)
                && !skills
                    .iter()
                    .any(|s| s.command.as_ref().is_some_and(|sc| sc.name == c.name))
        })
        .map(|c| ComposerCommand {
            token: format!("/{}", c.name),
            insertion: Invocation::Command { name: c.name }.link(),
            description: c.description,
            input_hint: c.input_hint,
            unavailable_reason: c.unavailable_reason,
        })
        .collect();
    rows.extend(skills.into_iter().filter(|s| s.enabled).map(|s| {
        ComposerCommand {
            token: format!("${}", s.name),
            description: format!("{} — {}", s.description, s.path),
            input_hint: None,
            insertion: Invocation::Skill {
                name: s.name,
                path: s.path,
                command: s.command,
            }
            .link(),
            unavailable_reason: None,
        }
    }));
    // A unique display suffix disambiguates skills with the same name but different paths.
    let mut used = std::collections::HashSet::new();
    for row in &mut rows {
        let base = row.token.clone();
        let mut n = 2;
        while !used.insert(row.token.clone()) {
            row.token = format!("{base}:{n}");
            n += 1;
        }
    }
    rows.sort_by(|a, b| {
        a.unavailable_reason
            .is_some()
            .cmp(&b.unavailable_reason.is_some())
            .then(a.token.cmp(&b.token))
    });
    ComposerCatalog {
        commands: rows,
        warning: (!warnings.is_empty()).then(|| warnings.join("\n")),
    }
}

fn decode<T: serde::de::DeserializeOwned>(
    reply: Result<serde_json::Value>,
    label: &str,
    warnings: &mut Vec<String>,
) -> Option<T> {
    match reply
        .and_then(|v| serde_json::from_value(v).map_err(|e| ClientError::HostError(e.to_string())))
    {
        Ok(value) => Some(value),
        Err(e) => {
            warnings.push(format!("{label} unavailable: {e}. Reopen / to retry."));
            None
        }
    }
}

/// Encode a selected leading token without touching inline prose, code blocks,
/// prefix matches or existing canonical links. Completion only edits this token.
pub fn encode_selections(text: &str, selections: &[ComposerCommand]) -> String {
    let trimmed = text.trim_start();
    let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let token = &trimmed[..end];
    match selections
        .iter()
        .find(|s| s.unavailable_reason.is_none() && s.token == token)
    {
        Some(s) => format!(
            "{}{}{}",
            &text[..text.len() - trimmed.len()],
            s.insertion,
            &trimmed[end..]
        ),
        None => text.to_owned(),
    }
}

/// Restore the leading selected reference when reopening a saved or queued draft.
/// Keep the exact wire link so unchanged drafts round-trip byte for byte.
pub fn restore_draft(text: &str) -> (String, Vec<ComposerCommand>) {
    let Some((range, invocation)) = zeron_proto::invocation::invocation_links(text)
        .into_iter()
        .next()
    else {
        return (text.to_owned(), Vec::new());
    };
    if !text[..range.start].trim().is_empty()
        || text[range.end..]
            .chars()
            .next()
            .is_some_and(|c| !c.is_whitespace())
    {
        return (text.to_owned(), Vec::new());
    }
    let token = format!("{}{}", invocation.prefix(), invocation.name());
    let selection = ComposerCommand {
        token: token.clone(),
        description: String::new(),
        input_hint: None,
        insertion: text[range.clone()].to_owned(),
        unavailable_reason: None,
    };
    let mut visible = text.to_owned();
    visible.replace_range(range, &token);
    (visible, vec![selection])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn partial_failure_keeps_commands_and_reports_retry() {
        let c = from_replies(
            Ok(
                json!([{"name":"goal", "inputHint":"objective"}, {"name":"model", "unavailableReason":"Desktop only"}]),
            ),
            Err(ClientError::Unsupported("ListSkills".into())),
        );
        assert_eq!(c.commands.len(), 2);
        assert_eq!(c.commands[0].token, "/goal");
        assert_eq!(
            c.commands[1].unavailable_reason.as_deref(),
            Some("Desktop only")
        );
        assert!(c.warning.unwrap().contains("Skills unavailable"));
        assert_eq!(
            zeron_proto::invocation::invocation_prompt(&encode_selections(
                "/goal ship it",
                &c.commands
            )),
            "/goal ship it"
        );
        assert_eq!(
            encode_selections("explain /goal", &c.commands),
            "explain /goal"
        );
        assert_eq!(encode_selections("/goals", &c.commands), "/goals");
        assert_eq!(encode_selections("/model", &c.commands), "/model");
    }
    #[test]
    fn skills_keep_identity_and_filter_invalid_or_disabled_entries() {
        let c = from_replies(
            Ok(json!([{"name":"review"}, {"name":"hidden"}])),
            Ok(json!([
                {"name":"review","path":"/a/SKILL.md","description":"A","enabled":true,"command":{"name":"review","harness":"claude-code"}},
                {"name":"review","path":"/b/SKILL.md","description":"B","enabled":true},
                {"name":"hidden","path":"/c/SKILL.md","description":"","enabled":false,"command":{"name":"hidden","harness":"claude-code"}},
                {"name":"bad name","path":"/d","description":"","enabled":true}
            ])),
        );
        assert_eq!(c.commands.len(), 2);
        assert_eq!(c.commands[0].token, "$review");
        assert_eq!(c.commands[1].token, "$review:2");
        let encoded = encode_selections("$review:2 do this", &c.commands);
        let links = zeron_proto::invocation::invocation_links(&encoded);
        assert!(matches!(&links[0].1, Invocation::Skill { path, .. } if path == "/b/SKILL.md"));
    }
    #[test]
    fn saved_and_queued_drafts_preserve_skill_identity_and_arguments() {
        let invocation = Invocation::Skill {
            name: "review".into(),
            path: "/project B/SKILL.md".into(),
            command: None,
        };
        let wire = format!("  {} inspect this", invocation.link());
        let (text, selections) = restore_draft(&wire);
        assert_eq!(text, "  $review inspect this");
        assert_eq!(encode_selections(&text, &selections), wire);
        assert_eq!(restore_draft("plain text"), ("plain text".into(), vec![]));
        let code = format!("```\n{}\n```", invocation.link());
        assert_eq!(restore_draft(&code), (code, vec![]));
    }
    #[test]
    fn null_skills_and_empty_commands_are_not_errors() {
        let c = from_replies(Ok(json!([])), Ok(json!(null)));
        assert!(c.commands.is_empty());
        assert!(c.warning.is_none());
    }
}
