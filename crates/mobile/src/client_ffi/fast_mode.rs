use super::ModelOption;
use std::collections::HashMap;
use zeron_proto::fast_mode;

#[derive(Debug, Clone, uniffi::Record)]
pub struct FastModeControl {
    pub id: String,
    pub on: String,
    pub off: String,
}

#[uniffi::export]
pub fn find_fast_mode(options: Vec<ModelOption>) -> Option<FastModeControl> {
    options.into_iter().find_map(|option| {
        let (on, off) = fast_mode::choices(
            &option.id,
            &option.default_choice,
            option.choices.iter().map(|choice| choice.id.as_str()),
        )?;
        Some(FastModeControl {
            on: on.into(),
            off: off.into(),
            id: option.id,
        })
    })
}

#[uniffi::export]
pub fn fast_mode_enabled(control: FastModeControl, value: Option<String>) -> Option<bool> {
    value.map(|value| fast_mode::enabled(&control.id, &control.on, &value))
}

#[uniffi::export]
pub fn set_fast_mode(
    control: FastModeControl,
    enabled: Option<bool>,
    mut values: HashMap<String, String>,
) -> HashMap<String, String> {
    if let Some(enabled) = enabled {
        values.insert(
            control.id,
            fast_mode::choice(&control.on, &control.off, enabled).into(),
        );
    } else {
        values.remove(&control.id);
    }
    values
}

#[uniffi::export]
pub fn fast_options_after_model_change(
    mut values: HashMap<String, String>,
    harness: String,
    previous_option_id: Option<String>,
) -> HashMap<String, String> {
    if let Some(id) = previous_option_id
        && !fast_mode::preserves_standard(
            harness == "codex",
            &id,
            values.get(&id).map(String::as_str),
        )
    {
        values.remove(&id);
    }
    values
}
