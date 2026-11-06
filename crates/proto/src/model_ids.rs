//! Provider-scoped model ids and their display names.
//!
//! Some ACP agents persist the selected model as the exact select value, which
//! for dsh is a JSON tuple (`["provider","model"]`). The tuple is the routing
//! id, not a label: pickers and session rows should show the model's name.

use serde_json::Value;

/// Split a provider-scoped JSON-tuple model id (`["provider","model"]`) into
/// its two non-empty parts. Every other id (a plain wire id, a display name)
/// returns `None`.
pub fn split_provider_model(id: &str) -> Option<(String, String)> {
    let value: Value = serde_json::from_str(id.trim()).ok()?;
    let [provider, model] = value.as_array()?.as_slice() else {
        return None;
    };
    let provider = provider.as_str()?;
    let model = model.as_str()?;
    (!provider.is_empty() && !model.is_empty()).then(|| (provider.to_owned(), model.to_owned()))
}

/// Readable name for a model id that arrived without one (or whose advertised
/// name only echoes the id): drop a provider-scoped tuple or namespace and
/// split the model slug into words, keeping common brand casing.
///
/// `["local","group/deepseek-flash"]` and `group/deepseek-flash` both become
/// `DeepSeek Flash`; `["deepseek-official","deepseek-v4-pro"]` becomes
/// `DeepSeek V4 Pro`.
pub fn display_model_name(id: &str) -> String {
    let trimmed = id.trim();
    let raw = split_provider_model(trimmed)
        .map(|(_, model)| model)
        .unwrap_or_else(|| trimmed.to_owned());
    let slug = raw.rsplit('/').next().unwrap_or(&raw).trim();
    if slug.is_empty() {
        return trimmed.to_owned();
    }
    slug.split(['-', '_', ' '])
        .filter(|word| !word.is_empty())
        .map(canonical_word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn canonical_word(word: &str) -> String {
    let lower = word.to_ascii_lowercase();
    let known = match lower.as_str() {
        "deepseek" => "DeepSeek",
        "openai" => "OpenAI",
        "gpt" => "GPT",
        "glm" => "GLM",
        "api" => "API",
        "ai" => "AI",
        "qwen" => "Qwen",
        "kimi" => "Kimi",
        "llama" => "Llama",
        "mistral" => "Mistral",
        "minimax" => "MiniMax",
        "claude" => "Claude",
        "gemini" => "Gemini",
        "grok" => "Grok",
        _ => "",
    };
    if !known.is_empty() {
        return known.to_owned();
    }
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_tuples_split_into_routing_parts() {
        assert_eq!(
            split_provider_model("[\"local\",\"group/deepseek-flash\"]"),
            Some(("local".into(), "group/deepseek-flash".into()))
        );
        assert_eq!(
            split_provider_model(" [\"deepseek-official\",\"deepseek-v4-pro\"] "),
            Some(("deepseek-official".into(), "deepseek-v4-pro".into()))
        );
        assert_eq!(split_provider_model("claude-opus-5"), None);
        assert_eq!(split_provider_model("[\"only-one\"]"), None);
        assert_eq!(split_provider_model("[\"\",\"model\"]"), None);
    }

    #[test]
    fn display_names_hide_raw_tuples_and_namespaces() {
        assert_eq!(
            display_model_name("[\"local\",\"group/deepseek-flash\"]"),
            "DeepSeek Flash"
        );
        assert_eq!(display_model_name("group/deepseek-flash"), "DeepSeek Flash");
        assert_eq!(
            display_model_name("[\"deepseek-official\",\"deepseek-v4-pro\"]"),
            "DeepSeek V4 Pro"
        );
        assert_eq!(display_model_name("deepseek-chat"), "DeepSeek Chat");
        assert_eq!(display_model_name("unknown-model"), "Unknown Model");
        assert_eq!(
            display_model_name("DeepSeek V4.1 Flash (group)"),
            "DeepSeek V4.1 Flash (group)"
        );
    }
}
