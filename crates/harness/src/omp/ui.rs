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
                    // OMP accepts a pure custom answer (`selectedOptions: []`
                    // plus `customInput`), one option alone, or several options
                    // for a multi-select question. It rejects more than one
                    // option, or an option together with custom text, for a
                    // single-select question.
                    let mut selected: Vec<String> = vec![];
                    let mut custom: Option<String> = None;
                    let labels = answers
                        .iter()
                        .find(|a| a.question_id == question.id)
                        .map(|a| a.labels.as_slice())
                        .unwrap_or_default();
                    for label in labels {
                        let label = label.trim();
                        if label.is_empty() {
                            continue;
                        }
                        if options.iter().any(|option| option == label) {
                            if !selected.iter().any(|chosen| chosen == label) {
                                selected.push(label.to_owned());
                            }
                        } else if custom.is_none() {
                            custom = Some(label.to_owned());
                        }
                    }
                    if !multi {
                        if custom.is_some() {
                            // A single-select question answers with either an
                            // option or free text, never both.
                            selected.clear();
                        } else {
                            selected.truncate(1);
                        }
                    }
                    if selected.is_empty() && custom.is_none() {
                        // No usable answer at all: cancel the whole dialog
                        // rather than inventing a choice.
                        response = json!({"type":"extension_ui_response","id":id,"cancelled":true});
                        let _ = client.send(response);
                        return;
                    }
                    let mut entry = json!({"id":omp_id,"selectedOptions":selected});
                    if let Some(custom) = custom {
                        entry["customInput"] = json!(custom);
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, BufReader};

    fn answering(labels: Vec<String>) -> Arc<Input> {
        Arc::new(move |questions: Vec<UserInputQuestion>| {
            let (tx, rx) = oneshot::channel();
            let answers = questions
                .iter()
                .map(|question| UserInputAnswer {
                    question_id: question.id.clone(),
                    labels: labels.clone(),
                })
                .collect();
            let _ = tx.send(answers);
            rx
        })
    }

    fn ask_frame(multi: bool, options: &[&str]) -> Value {
        json!({
            "id": "ask-1",
            "method": "ask",
            "questions": [{
                "id": "q1",
                "question": "Pick one",
                "header": "H",
                "multi": multi,
                "options": options
                    .iter()
                    .map(|label| json!({ "label": label }))
                    .collect::<Vec<_>>(),
            }]
        })
    }

    /// Drive the real `request -> extension_ui_response` path and read the frame
    /// the driver would write to OMP.
    async fn ask_response(labels: Vec<String>, frame: Value) -> Value {
        let (writer, peer) = tokio::io::duplex(4096);
        let (_peer_writer, reader) = tokio::io::duplex(4096);
        let transport = super::super::rpc::Transport::new(writer, reader);
        let mut dialogs = Dialogs::default();
        dialogs.input = Some(answering(labels));
        dialogs.request(transport.client.clone(), &frame);
        let mut lines = BufReader::new(peer).lines();
        let line = tokio::time::timeout(std::time::Duration::from_secs(2), lines.next_line())
            .await
            .expect("dialog response timed out")
            .expect("dialog stream closed")
            .expect("dialog response line");
        serde_json::from_str(&line).expect("dialog response JSON")
    }

    #[tokio::test]
    async fn ask_single_select_custom_text_is_kept() {
        let response = ask_response(
            vec!["my own answer".into()],
            ask_frame(false, &["Alpha", "Beta"]),
        )
        .await;
        assert_eq!(response["answers"][0]["selectedOptions"], json!([]));
        assert_eq!(response["answers"][0]["customInput"], "my own answer");
        assert!(response.get("cancelled").is_none());
    }

    #[tokio::test]
    async fn ask_free_text_question_without_options_is_kept() {
        let response = ask_response(vec!["free form".into()], ask_frame(false, &[])).await;
        assert_eq!(response["answers"][0]["selectedOptions"], json!([]));
        assert_eq!(response["answers"][0]["customInput"], "free form");
        assert!(response.get("cancelled").is_none());
    }

    #[tokio::test]
    async fn ask_single_select_option_is_selected() {
        let response = ask_response(vec!["Beta".into()], ask_frame(false, &["Alpha", "Beta"])).await;
        assert_eq!(response["answers"][0]["selectedOptions"], json!(["Beta"]));
        assert!(response["answers"][0].get("customInput").is_none());
    }

    #[tokio::test]
    async fn ask_multi_select_keeps_options_and_custom() {
        let response = ask_response(
            vec!["Alpha".into(), "Beta".into(), "extra".into()],
            ask_frame(true, &["Alpha", "Beta"]),
        )
        .await;
        assert_eq!(
            response["answers"][0]["selectedOptions"],
            json!(["Alpha", "Beta"])
        );
        assert_eq!(response["answers"][0]["customInput"], "extra");
    }

    #[tokio::test]
    async fn ask_empty_answer_cancels() {
        let response = ask_response(vec![], ask_frame(false, &["Alpha"])).await;
        assert_eq!(response["cancelled"], true);
    }
}
