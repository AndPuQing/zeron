//! OMP model and slash-command discovery. Thinking levels ride each model's
//! declared `thinking.efforts`, so discovery needs no per-model round-trip.
use super::{Process, normalize::string};
use crate::HarnessError;
use serde_json::{Value, json};
use zeron_proto::{Model, ModelOption, ModelOptionChoice, ReasoningLevel, SlashCommand};

fn levels(model: &Value) -> Vec<ReasoningLevel> {
    model["thinking"]["efforts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .collect()
}
pub(super) async fn models(process: &mut Process) -> Result<Vec<Model>, HarnessError> {
    let mut backlog = vec![];
    let data = process
        .query(json!({"type":"get_available_models"}), &mut backlog)
        .await?;
    let mut result = vec![];
    for model in data["models"].as_array().into_iter().flatten() {
        let provider = string(model, "provider");
        let id = string(model, "id");
        let reasoning_levels = levels(model);
        result.push(Model {
            id: format!("{provider}/{id}"),
            label: model["name"].as_str().unwrap_or(id).into(),
            description: Some(provider.into()),
            options: thinking_option(model),
            reasoning_levels,
        });
    }
    Ok(result)
}
fn thinking_option(model: &Value) -> Vec<ModelOption> {
    // `off` is the only thinking choice the effort ladder cannot express.
    if model["thinking"].is_object() || model["reasoning"] == true {
        vec![ModelOption {
            id: "omp_thinking".into(),
            label: "Thinking".into(),
            default_choice: "auto".into(),
            choices: vec![
                ModelOptionChoice {
                    id: "auto".into(),
                    label: "Use reasoning level".into(),
                },
                ModelOptionChoice {
                    id: "off".into(),
                    label: "Off".into(),
                },
            ],
        }]
    } else {
        vec![]
    }
}
pub(super) fn commands(data: &Value) -> Vec<SlashCommand> {
    data["commands"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| {
            let name = v["name"].as_str()?;
            if name.is_empty() {
                return None;
            }
            Some(SlashCommand {
                unavailable_reason: None,
                name: name.into(),
                description: string(v, "description").into(),
                input_hint: v["input"]["hint"].as_str().map(str::to_owned),
            })
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reasoning_levels_come_from_the_models_own_effort_ladder() {
        let model = json!({"id":"m","provider":"p","name":"M","reasoning":true,
            "thinking":{"mode":"effort","efforts":["low","high","max"]}});
        assert_eq!(
            levels(&model),
            vec![
                ReasoningLevel::Low,
                ReasoningLevel::High,
                ReasoningLevel::Max
            ]
        );
        let options = thinking_option(&model);
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].id, "omp_thinking");
    }

    #[test]
    fn commands_read_the_hint_and_skip_nameless_entries() {
        let data = json!({"commands":[
            {"name":"security","description":"scans","input":{"hint":"<plan|scan>"}},
            {"description":"no name"}
        ]});
        let commands = commands(&data);
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "security");
        assert_eq!(commands[0].input_hint.as_deref(), Some("<plan|scan>"));
    }
}
