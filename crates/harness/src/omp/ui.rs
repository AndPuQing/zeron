use super::{normalize::string, rpc::Client};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::{sync::oneshot, task::JoinSet};
use zeron_proto::{UserInputAnswer, UserInputQuestion};
pub(super) type Input =
    dyn Fn(Vec<UserInputQuestion>) -> oneshot::Receiver<Vec<UserInputAnswer>> + Send + Sync;
#[derive(Default)]
pub(super) struct Dialogs {
    pub input: Option<Arc<Input>>,
    tasks: JoinSet<()>,
}
impl Dialogs {
    pub fn request(&mut self, client: Client, frame: &Value) {
        let method = string(frame, "method").to_owned();
        if !matches!(
            method.as_str(),
            "select" | "confirm" | "input" | "editor" | "ask"
        ) {
            return;
        }
        while self.tasks.try_join_next().is_some() {}
        let id = frame["id"].clone();
        let Some(input) = &self.input else {
            let _ = client.send(json!({"type":"extension_ui_response","id":id,"cancelled":true}));
            return;
        };
        // `ask` carries several questions with structured options; the older
        // single-question methods are projections of the same bridge.
        let questions: Vec<(String, UserInputQuestion, Vec<String>, bool)> = if method == "ask" {
            frame["questions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|question| {
                    let options: Vec<String> = question["options"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|option| option["label"].as_str().map(str::to_owned))
                        .collect();
                    let multi = question["multi"].as_bool().unwrap_or(false);
                    let bridge_id = uuid::Uuid::new_v4().to_string();
                    Some((
                        string(question, "id").to_owned(),
                        UserInputQuestion {
                            id: bridge_id,
                            header: question["header"]
                                .as_str()
                                .filter(|h| !h.is_empty())
                                .unwrap_or("OMP")
                                .into(),
                            question: string(question, "question").into(),
                            options: options.clone(),
                            multi_select: multi,
                            prefill: None,
                            multiline: false,
                        },
                        options,
                        multi,
                    ))
                })
                .collect()
        } else {
            let options = if method == "confirm" {
                vec!["Yes".into(), "No".into()]
            } else {
                frame["options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            };
            let title = string(frame, "title");
            let message = string(frame, "message");
            vec![(
                String::new(),
                UserInputQuestion {
                    id: uuid::Uuid::new_v4().to_string(),
                    header: "OMP".into(),
                    question: if message.is_empty() {
                        title.into()
                    } else {
                        format!("{title}\n{message}")
                    },
                    options: options.clone(),
                    multi_select: false,
                    prefill: frame["prefill"].as_str().map(str::to_owned),
                    multiline: method == "editor",
                },
                options,
                false,
            )]
        };
        if questions.is_empty() {
            let _ = client.send(json!({"type":"extension_ui_response","id":id,"cancelled":true}));
            return;
        }
        let bridge: Vec<UserInputQuestion> = questions
            .iter()
            .map(|(_, question, _, _)| question.clone())
            .collect();
        let answer = (input)(bridge);
        let timeout = frame["timeout"].as_u64();
        self.tasks.spawn(async move {
            let deadline = async {
                match timeout {
                    Some(ms) => tokio::time::sleep(std::time::Duration::from_millis(ms)).await,
                    None => std::future::pending().await,
                }
            };
            let answers =
                tokio::select! { result=answer=>result.unwrap_or_default(),_=deadline=>vec![] };
            let mut response = json!({"type":"extension_ui_response","id":id});
            if method == "ask" {
                let mut resolved = vec![];
                for (omp_id, question, options, multi) in &questions {
                    let labels = answers
                        .iter()
                        .find(|a| a.question_id == question.id)
                        .map(|a| a.labels.as_slice())
                        .unwrap_or_default();
                    let selected: Vec<&String> = labels
                        .iter()
                        .filter(|label| options.contains(label))
                        .collect();
                    let custom: Vec<&String> = labels
                        .iter()
                        .filter(|label| !options.contains(label))
                        .collect();
                    let mut entry = json!({"id":omp_id,"selectedOptions":selected});
                    if !custom.is_empty() {
                        entry["customInput"] = json!(custom[0]);
                    }
                    if selected.is_empty() && (!multi || custom.is_empty()) {
                        // No usable answer: cancel the whole dialog rather than
                        // silently selecting the first option.
                        response = json!({"type":"extension_ui_response","id":id,"cancelled":true});
                        let _ = client.send(response);
                        return;
                    }
                    resolved.push(entry);
                }
                response["answers"] = json!(resolved);
            } else {
                let (_, question, options, _) = &questions[0];
                let value = answers
                    .iter()
                    .find(|a| a.question_id == question.id)
                    .and_then(|a| a.labels.first());
                match (method.as_str(), value) {
                    ("confirm", Some(value)) if options.contains(value) => {
                        response["confirmed"] = json!(value == "Yes")
                    }
                    ("select", Some(value)) if options.contains(value) => {
                        response["value"] = json!(value)
                    }
                    ("input" | "editor", Some(value)) => response["value"] = json!(value),
                    _ => response["cancelled"] = json!(true),
                }
            }
            let _ = client.send(response);
        });
    }
    pub fn cancel(&mut self) {
        self.tasks.abort_all();
    }
}
