//! Fast controls are backed by each model's advertised option and wire choices.

pub fn choices<'a>(
    id: &str,
    default: &'a str,
    choices: impl Iterator<Item = &'a str> + Clone,
) -> Option<(&'a str, &'a str)> {
    let has = |value| choices.clone().any(|choice| choice == value);
    let on = if matches!(id, "fastMode" | "fast_mode") && has("on") {
        "on"
    } else if id == "fast" && has("true") {
        "true"
    } else if has("fast") {
        "fast"
    } else {
        return None;
    };
    let off = if default != on && has(default) {
        default
    } else {
        choices.clone().find(|choice| *choice != on)?
    };
    Some((on, off))
}

/// Migrate only known Fast encodings. An unknown or retired choice is not an Off selection.
pub fn legacy_choice<'a>(
    id: &str,
    on: &'a str,
    off: &'a str,
    value: &serde_json::Value,
) -> Option<&'a str> {
    legacy_state(id, on, stored_value(value)?).map(|state| choice(on, off, state))
}

fn stored_value(value: &serde_json::Value) -> Option<&str> {
    match value {
        serde_json::Value::Bool(true) => Some("true"),
        serde_json::Value::Bool(false) => Some("false"),
        other => other.as_str(),
    }
}

fn legacy_state(id: &str, on: &str, value: &str) -> Option<bool> {
    match (id, on, value) {
        ("serviceTier", "fast", "priority") => Some(true),
        ("fastMode" | "fast_mode", "on", boolean) | ("fast", "true", boolean) => {
            boolean.parse().ok()
        }
        _ => None,
    }
}

pub fn enabled(id: &str, on: &str, value: &str) -> bool {
    value == on || legacy_state(id, on, value) == Some(true)
}

pub fn enabled_json(id: &str, on: &str, value: &serde_json::Value) -> bool {
    stored_value(value).is_some_and(|s| enabled(id, on, s))
}

pub fn choice<'a>(on: &'a str, off: &'a str, enabled: bool) -> &'a str {
    if enabled { on } else { off }
}

pub fn requires_explicit_tier(is_codex: bool, id: &str) -> bool {
    is_codex && id == "serviceTier"
}

/// Codex requires an explicit Standard tier to reset an inherited Fast tier.
pub fn preserves_standard(is_codex: bool, id: &str, value: Option<&str>) -> bool {
    requires_explicit_tier(is_codex, id) && value == Some("default")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodings_use_the_advertised_off_choice_even_when_fast_is_default() {
        for (id, options, default, expected) in [
            (
                "serviceTier",
                ["default", "fast"],
                "fast",
                ("fast", "default"),
            ),
            ("fastMode", ["off", "on"], "off", ("on", "off")),
            ("fast_mode", ["off", "on"], "off", ("on", "off")),
            ("fast", ["false", "true"], "true", ("true", "false")),
            (
                "speed",
                ["standard", "fast"],
                "standard",
                ("fast", "standard"),
            ),
        ] {
            let (on, off) = choices(id, default, options.into_iter()).unwrap();
            assert_eq!((on, off), expected);
            assert!(enabled(id, on, choice(on, off, true)));
            assert!(!enabled(id, on, choice(on, off, false)));
        }
        assert_eq!(choices("thinking", "off", ["off", "on"].into_iter()), None);
        assert_eq!(choices("speed", "fast", ["fast"].into_iter()), None);
    }

    #[test]
    fn canonicalize_only_known_legacy_choices() {
        use serde_json::json;

        assert_eq!(
            legacy_choice("serviceTier", "fast", "default", &json!("priority")),
            Some("fast")
        );
        for id in ["fastMode", "fast_mode"] {
            assert_eq!(legacy_choice(id, "on", "off", &json!(true)), Some("on"));
            assert_eq!(legacy_choice(id, "on", "off", &json!("false")), Some("off"));
        }
        assert_eq!(
            legacy_choice("fast", "true", "false", &json!(false)),
            Some("false")
        );
        assert_eq!(legacy_choice("speed", "fast", "normal", &json!(true)), None);
        assert_eq!(
            legacy_choice("serviceTier", "fast", "default", &json!("retired")),
            None
        );
    }

    #[test]
    fn aliases_and_the_codex_standard_reset_are_distinct_from_inheritance() {
        assert!(enabled("serviceTier", "fast", "priority"));
        assert!(enabled("fastMode", "on", "true"));
        assert!(!enabled("speed", "fast", "priority"));
        assert!(preserves_standard(true, "serviceTier", Some("default")));
        assert!(!preserves_standard(true, "serviceTier", None));
        assert!(!preserves_standard(true, "serviceTier", Some("priority")));
        assert!(!preserves_standard(false, "serviceTier", Some("default")));
    }
}
