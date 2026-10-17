//! Host-backed mobile composer catalog. No CLI catalog is duplicated on phones.
use crate::error::{ClientError, Result};
use zeron_proto::{
    SlashCommand,
    invocation::{
        Invocation, Skill, valid_invocation_name, valid_skill_command_name, valid_skill_path,
    },
};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

/// Reserve a display token before inserting a catalog selection. `text` excludes
/// the completion query being replaced. Restored drafts can assign different
/// tokens than a freshly loaded catalog, so catalog tokens cannot overwrite them.
pub fn prepare_selection(
    text: &str,
    selections: &[ComposerCommand],
    mut command: ComposerCommand,
) -> ComposerCommand {
    if let Some(existing) = selections.iter().find(|s| s.insertion == command.insertion) {
        command.token = existing.token.clone();
        return command;
    }
    let base = zeron_proto::invocation::invocation_links(&command.insertion)
        .into_iter()
        .next()
        .map(|(_, invocation)| format!("{}{}", invocation.prefix(), invocation.name()))
        .unwrap_or_else(|| command.token.clone());
    let mut suffix = 2;
    while selections.iter().any(|s| s.token == command.token) || text.contains(&command.token) {
        command.token = format!("{base}:{suffix}");
        suffix += 1;
    }
    command
}

/// Find selected tokens in Markdown prose, keeping code, links and escapes literal.
/// Skills can move within prose; slash commands must remain leading commands.
fn selected_ranges<'a>(
    text: &str,
    selections: &'a [ComposerCommand],
) -> Vec<(std::ops::Range<usize>, &'a ComposerCommand)> {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    let mut candidates: Vec<_> = selections.iter().collect();
    candidates.sort_by_key(|s| std::cmp::Reverse(s.token.len()));
    let mut protected = 0usize;
    let mut matches = Vec::new();
    for (event, range) in Parser::new(text).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_) | Tag::Link { .. } | Tag::Image { .. }) => {
                protected += 1
            }
            Event::End(TagEnd::CodeBlock | TagEnd::Link | TagEnd::Image) => protected -= 1,
            Event::Text(_) if protected == 0 => {
                let mut at = range.start;
                while at < range.end {
                    let selected = candidates.iter().find(|s| {
                        if s.token.is_empty() || !text[at..range.end].starts_with(&s.token) {
                            return false;
                        }
                        let end = at + s.token.len();
                        let before = text[..at].chars().next_back();
                        let after = text[end..].chars().next();
                        let boundary =
                            |c: char| c.is_whitespace() || ",;!?()[]{}，。；！？、\"'".contains(c);
                        let ends = end == range.end
                            || after.is_none_or(boundary)
                            || (after == Some('.')
                                && text[end + 1..]
                                    .chars()
                                    .next()
                                    .is_none_or(char::is_whitespace));
                        let begins = before.is_none_or(boundary)
                            || (at == range.start && before != Some('\\'));
                        let slash = !s.token.starts_with('/')
                            || (text[..at].trim().is_empty()
                                && zeron_proto::invocation::leading_command(text).is_some());
                        begins && ends && slash
                    });
                    if let Some(s) = selected {
                        let end = at + s.token.len();
                        matches.push((at..end, *s));
                        at = end;
                    } else {
                        at += text[at..].chars().next().unwrap().len_utf8();
                    }
                }
            }
            _ => {}
        }
    }
    matches
}

/// Encode selected references at their current prose positions.
pub fn encode_selections(text: &str, selections: &[ComposerCommand]) -> String {
    let mut result = text.to_owned();
    for (range, selection) in selected_ranges(text, selections).into_iter().rev() {
        if selection.unavailable_reason.is_none() {
            result.replace_range(range, &selection.insertion);
        }
    }
    result
}

/// Sending must stop if a selected reference still in the draft was invalidated.
pub fn selection_error(text: &str, selections: &[ComposerCommand]) -> Option<String> {
    let mut invalid: Vec<_> = selected_ranges(text, selections)
        .into_iter()
        .filter(|(_, s)| s.unavailable_reason.is_some())
        .map(|(_, s)| s.token.as_str())
        .collect();
    invalid.sort_unstable();
    invalid.dedup();
    (!invalid.is_empty()).then(|| {
        format!(
            "Agent, host or project changed. Remove or reselect {} before sending.",
            invalid.join(", ")
        )
    })
}

const SAVED_DRAFT_PREFIX: &str = "zeron-composer-draft-v1:";

/// Invalid selections are local draft state, never a provider prompt. Preserve
/// them across navigation and queue-edit stashing so reopening cannot bypass validation.
pub fn save_draft(text: &str, selections: &[ComposerCommand]) -> String {
    if selection_error(text, selections).is_some() {
        format!(
            "{SAVED_DRAFT_PREFIX}{}",
            serde_json::to_string(&(text, selections)).expect("draft serializes")
        )
    } else {
        encode_selections(text, selections)
    }
}

/// Restore every reference, including inline and same-name skills. Never bind
/// an existing literal token to an unrelated restored reference.
pub fn restore_draft(text: &str) -> (String, Vec<ComposerCommand>) {
    if let Some(saved) = text.strip_prefix(SAVED_DRAFT_PREFIX)
        && let Ok(draft) = serde_json::from_str::<(String, Vec<ComposerCommand>)>(saved)
    {
        return draft;
    }
    let links = zeron_proto::invocation::invocation_links(text);
    let mut literal = text.to_owned();
    for (range, _) in links.iter().rev() {
        literal.replace_range(range.clone(), "");
    }
    let mut visible = String::new();
    let mut selections: Vec<ComposerCommand> = Vec::new();
    let mut at = 0;
    for (range, invocation) in links {
        visible.push_str(&text[at..range.start]);
        let insertion = &text[range.clone()];
        let token = if let Some(existing) = selections.iter().find(|s| s.insertion == insertion) {
            existing.token.clone()
        } else {
            let base = format!("{}{}", invocation.prefix(), invocation.name());
            let mut token = base.clone();
            let mut suffix = 2;
            while literal.contains(&token) || selections.iter().any(|s| s.token == token) {
                token = format!("{base}:{suffix}");
                suffix += 1;
            }
            selections.push(ComposerCommand {
                token: token.clone(),
                description: String::new(),
                input_hint: None,
                insertion: insertion.to_owned(),
                unavailable_reason: None,
            });
            token
        };
        visible.push_str(&token);
        at = range.end;
    }
    visible.push_str(&text[at..]);
    // Adjacent Markdown constructs may not have unambiguous plain-token
    // boundaries. Keep their canonical references rather than losing identity.
    if encode_selections(&visible, &selections) != text {
        return (text.to_owned(), Vec::new());
    }
    (visible, selections)
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
    fn selected_skill(token: &str, path: &str) -> ComposerCommand {
        ComposerCommand {
            token: token.into(),
            description: String::new(),
            input_hint: None,
            insertion: Invocation::Skill {
                name: "review".into(),
                path: path.into(),
                command: None,
            }
            .link(),
            unavailable_reason: None,
        }
    }

    #[test]
    fn inserting_a_same_name_skill_after_restoring_does_not_rebind_the_draft() {
        let a = selected_skill("$review", "/a/SKILL.md");
        let b = selected_skill("$review:2", "/b/SKILL.md");
        let wire = encode_selections("$review:2 check this", std::slice::from_ref(&b));
        let (text, mut selections) = restore_draft(&wire);
        assert_eq!(text, "$review check this");

        let selected = prepare_selection(&text, &selections, a.clone());
        assert_eq!(selected.token, "$review:2");
        let next = format!("{} {text}", selected.token);
        selections.push(selected);
        let expected = format!("{} {wire}", a.insertion);
        assert_eq!(encode_selections(&next, &selections), expected);
        let (restored, selections) = restore_draft(&save_draft(&next, &selections));
        assert_eq!(encode_selections(&restored, &selections), expected);
    }

    #[test]
    fn new_selections_preserve_literal_tokens_and_unrelated_invalid_references() {
        let a = selected_skill("$review", "/a/SKILL.md");
        let selected = prepare_selection("Literal $review and $review:2", &[], a.clone());
        assert_eq!(selected.token, "$review:3");
        assert_eq!(
            encode_selections(
                "$review:3 Literal $review and $review:2",
                std::slice::from_ref(&selected)
            ),
            format!("{} Literal $review and $review:2", a.insertion)
        );

        let mut invalid = selected_skill("$review", "/b/SKILL.md");
        invalid.unavailable_reason = Some("Context changed".into());
        let selected = prepare_selection("$review check this", &[invalid.clone()], a);
        assert_ne!(selected.token, invalid.token);
        let text = format!("{} $review check this", selected.token);
        assert!(selection_error(&text, &[invalid.clone(), selected.clone()]).is_some());

        // Explicitly selecting the same identity revalidates its existing token.
        let b = selected_skill("$review:2", "/b/SKILL.md");
        let reselected = prepare_selection(&text, &[invalid, selected.clone()], b);
        assert_eq!(reselected.token, "$review");
        assert!(selection_error(&text, &[reselected, selected]).is_none());
    }

    #[test]
    fn skills_survive_prepended_text_punctuation_and_repeated_mentions() {
        let skill = selected_skill("$review:2", "/b/SKILL.md");
        let text = "请使用 $review:2 检查代码，再用 ($review:2)。";
        let encoded = encode_selections(text, std::slice::from_ref(&skill));
        let links = zeron_proto::invocation::invocation_links(&encoded);
        assert_eq!(links.len(), 2);
        assert!(
            links
                .iter()
                .all(|(_, i)| matches!(i, Invocation::Skill { path, .. } if path == "/b/SKILL.md"))
        );
        assert!(encoded.starts_with("请使用 "));
        let (restored, selections) = restore_draft(&encoded);
        assert_eq!(encode_selections(&restored, &selections), encoded);
    }

    #[test]
    fn literal_code_links_escapes_and_token_prefixes_are_not_rewritten() {
        let skill = selected_skill("$review", "/b/SKILL.md");
        for text in [
            "`$review`",
            "```\n$review\n```",
            "    $review",
            "[$review](https://example.com)",
            "![caption $review](image.png)",
            r"\$review",
            "$review:2",
            "$review-more",
            "$review.md",
        ] {
            assert_eq!(
                encode_selections(text, std::slice::from_ref(&skill)),
                text,
                "{text}"
            );
        }
        assert_eq!(
            encode_selections("Use $review.", std::slice::from_ref(&skill)),
            format!("Use {}.", skill.insertion)
        );
    }

    #[test]
    fn formatted_skills_are_preserved_and_ambiguous_restores_stay_canonical() {
        let skill = selected_skill("$review", "/b/SKILL.md");
        for text in ["**$review**", "_$review_", "> $review", "- $review"] {
            let encoded = encode_selections(text, std::slice::from_ref(&skill));
            assert_eq!(
                zeron_proto::invocation::invocation_links(&encoded).len(),
                1,
                "{text}"
            );
        }
        let wire = format!("prefix{}suffix", skill.insertion);
        let (text, selections) = restore_draft(&wire);
        assert_eq!(encode_selections(&text, &selections), wire);
    }

    #[test]
    fn restore_multiple_same_name_skills_without_rebinding_literal_text() {
        let a = selected_skill("$review", "/a/SKILL.md");
        let b = selected_skill("$review:2", "/b/SKILL.md");
        let wire = format!(
            "Literal $review; use {} and {} then {}.",
            a.insertion, b.insertion, a.insertion
        );
        let (text, selections) = restore_draft(&wire);
        assert_eq!(selections.len(), 2);
        assert_ne!(selections[0].token, selections[1].token);
        assert!(selections.iter().all(|s| s.token != "$review"));
        assert_eq!(encode_selections(&text, &selections), wire);
    }

    #[test]
    fn invalid_selection_blocks_send_until_removed_or_reselected_and_survives_saving() {
        let mut skill = selected_skill("$review:2", "/b/SKILL.md");
        skill.unavailable_reason = Some("Context changed".into());
        let text = "Please use $review:2 to review";
        assert!(
            selection_error(text, std::slice::from_ref(&skill))
                .unwrap()
                .contains("$review:2")
        );
        let saved = save_draft(text, std::slice::from_ref(&skill));
        assert!(saved.starts_with(SAVED_DRAFT_PREFIX));
        let (restored, selections) = restore_draft(&saved);
        assert_eq!(restored, text);
        assert!(selection_error(&restored, &selections).is_some());
        assert_eq!(selection_error("Please review", &selections), None);
        assert_eq!(selection_error("Example: `$review:2`", &selections), None);
        skill.unavailable_reason = None;
        assert_eq!(selection_error(text, std::slice::from_ref(&skill)), None);
        assert!(!save_draft(text, &[skill]).starts_with(SAVED_DRAFT_PREFIX));
    }

    #[test]
    fn null_skills_and_empty_commands_are_not_errors() {
        let c = from_replies(Ok(json!([])), Ok(json!(null)));
        assert!(c.commands.is_empty());
        assert!(c.warning.is_none());
    }
}
