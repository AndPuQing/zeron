//! Synthetic responses only; never accesses agent history or an engine.
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use super::*;
use crate::session_import::*;

fn sessions(device: &str) -> Vec<ExternalSession> {
    let root = if device == "local" {
        "/projects"
    } else {
        "/srv/projects"
    };
    [
        (
            "native-1",
            "Improve keyboard navigation",
            SessionProvider::Codex,
            "fieldnotes",
            ImportEligibility::Available,
        ),
        (
            "native-2",
            "Polish the session sidebar",
            SessionProvider::ClaudeCode,
            "fieldnotes",
            ImportEligibility::Available,
        ),
        (
            "native-3",
            "Handle authentication redirects",
            SessionProvider::Codex,
            "fieldnotes",
            ImportEligibility::Available,
        ),
        (
            "native-7",
            "Refactor the settings store",
            SessionProvider::ClaudeCode,
            "fieldnotes",
            ImportEligibility::Running,
        ),
        (
            "native-4",
            "Add deployment status",
            SessionProvider::ClaudeCode,
            "fieldnotes",
            ImportEligibility::AlreadyManaged {
                chat_id: if device == "local" {
                    "existing-session"
                } else {
                    "remote-existing-session"
                }
                .into(),
            },
        ),
        (
            "native-5",
            "Review the old worktree",
            SessionProvider::Codex,
            "fieldnotes/old-worktree",
            ImportEligibility::Unavailable {
                reason: "Working directory no longer exists".into(),
            },
        ),
        (
            "native-6",
            "Update API documentation",
            SessionProvider::ClaudeCode,
            "lighthouse",
            ImportEligibility::Available,
        ),
    ]
    .into_iter()
    .enumerate()
    .map(
        |(index, (id, title, provider, project, eligibility))| ExternalSession {
            source_ref: format!("{device}/{id}"),
            title: title.into(),
            provider,
            project: project.rsplit('/').next().unwrap().into(),
            cwd: format!("{root}/{project}"),
            updated_at: Utc::now() - chrono::Duration::hours((index as i64 + 1) * 3),
            eligibility,
        },
    )
    .collect()
}

fn transcript(device_id: &str) -> Vec<zeron_doc::SessionMessageEntry> {
    [
        (
            "user",
            "Can you make the session list work with the keyboard?",
        ),
        (
            "assistant",
            "The list now supports arrow keys to navigate and Enter to select a session.",
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (role, text))| {
        serde_json::from_value(serde_json::json!({
            "id": format!("import-message-{index}"),
            "role": role,
            "parts": [{"id":format!("part-{index}"),"kind":"text","text":text}],
            "createdAt": 1791330000000_i64 + index as i64 * 1000,
            "deviceId": device_id,
            "status": if role == "assistant" { Some("complete") } else { None },
        }))
        .unwrap()
    })
    .collect()
}

impl Shell {
    pub fn fixture_session_import_capture_ready(
        &mut self,
        scene: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if matches!(scene, "entry" | "fresh-entry") {
            if !self.session_import.menu.is_open() {
                self.session_import.menu.open(());
                window.focus(&self.session_import.menu_focus, cx);
                cx.notify();
                return false;
            }
            return true;
        }
        self.session_import
            .dialog
            .as_ref()
            .is_some_and(|dialog| dialog.view.read(cx).fixture_capture_ready(scene))
    }

    /// Runs the production import surfaces with an offline response adapter.
    pub fn fixture_session_import(
        &mut self,
        scene: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let scene = scene.to_owned();
        let state = self.state.clone();
        let cancelled = Rc::new(Cell::new(None::<u64>));
        let failed_once = Rc::new(RefCell::new(HashSet::<String>::new()));
        let prepared_scene = Rc::new(Cell::new(false));
        let controller = SessionImportController {
            devices: vec![
                ImportDevice {
                    id: "local".into(),
                    name: "This device".into(),
                    current_project: Some("/projects/fieldnotes".into()),
                },
                ImportDevice {
                    id: "remote".into(),
                    name: "Build server".into(),
                    current_project: None,
                },
            ],
            handle: Rc::new({
                let scene = scene.clone();
                move |view, event, cx| match event {
                    SessionImportEvent::Discover {
                        request_id,
                        device_id,
                        ..
                    } => {
                        if scene == "scanning" {
                            return;
                        }
                        let result = if scene == "scan-error" {
                            Err(
                                "The source device disconnected. Reconnect it and try again."
                                    .into(),
                            )
                        } else {
                            let mut result = SessionDiscovery {
                                sessions: if scene == "empty" {
                                    Vec::new()
                                } else {
                                    sessions(&device_id)
                                },
                                ..Default::default()
                            };
                            if scene == "fresh-entry" {
                                for row in &mut result.sessions {
                                    if matches!(
                                        row.eligibility,
                                        ImportEligibility::AlreadyManaged { .. }
                                    ) {
                                        row.eligibility = ImportEligibility::Available;
                                    }
                                }
                            }
                            if scene == "source-error" {
                                result
                                    .sessions
                                    .retain(|row| row.provider == SessionProvider::ClaudeCode);
                                result.source_errors.push("Codex is not available on this device. Claude Code sessions can still be imported.".into());
                            }
                            Ok(result)
                        };
                        let scene = scene.clone();
                        let prepared_scene = prepared_scene.clone();
                        cx.defer(move |cx| {
                            view.update(cx, |view, cx| {
                                view.receive_discovery(request_id, result, cx);
                                if !prepared_scene.replace(true) {
                                    view.fixture_prepare_scene(&scene, cx);
                                }
                            })
                        });
                    }
                    SessionImportEvent::Import {
                        request_id,
                        device_id,
                        source_refs,
                    } => {
                        cancelled.set(None);
                        let cancelled = cancelled.clone();
                        let failed_once = failed_once.clone();
                        let partial = scene == "partial";
                        let state = state.clone();
                        cx.spawn(async move |cx| {
                            for source_ref in source_refs {
                                if cancelled.get() == Some(request_id) { break; }
                                view.update(cx, |view, cx| view.update_import_item(request_id, &source_ref, ImportItemState::Importing, cx));
                                cx.background_executor().timer(Duration::from_millis(600)).await;
                                let failed = partial && source_ref.ends_with("native-3") && failed_once.borrow_mut().insert(source_ref.clone());
                                let outcome = if failed { ImportOutcome::Failed { reason: "Source changed during import. Retry this session.".into(), retryable: true } } else {
                                    let row = sessions(&device_id).into_iter().find(|row| row.source_ref == source_ref).unwrap();
                                    let chat_id = format!("imported-{}", source_ref.replace('/', "-"));
                                    state.update(cx, |state, cx| {
                                        if !state.chats.iter().any(|chat| chat.id == chat_id) {
                                            let space_id = state.spaces.iter().find(|space| space.device_id == device_id && crate::session_import::path_within(&row.cwd, &space.path)).map(|space| space.id.clone()).unwrap_or_else(|| {
                                                let id = format!("fixture-{device_id}-{}", row.project);
                                                state.spaces.push(serde_json::from_value(serde_json::json!({"id":id,"deviceId":device_id,"path":row.cwd,"createdAt":Utc::now()})).unwrap());
                                                id
                                            });
                                            state.chats.push(serde_json::from_value(serde_json::json!({
                                                "id":chat_id,"deviceId":device_id,"spaceId":space_id,
                                                "title":row.title,"cwd":row.cwd,"archived":true,"createdAt":row.updated_at,
                                                "harnessSessionId":source_ref,"harnessSessionCwd":row.cwd,
                                                "config":{"harness":row.provider.harness(),"model":null,"reasoning":null,"sandbox":"workspace-write"}
                                            })).unwrap());
                                        }
                                        cx.notify();
                                    });
                                    ImportOutcome::Imported { chat_id }
                                };
                                view.update(cx, |view, cx| view.update_import_item(request_id, &source_ref, ImportItemState::Finished(outcome), cx));
                            }
                            view.update(cx, |view, cx| view.finish_import(request_id, cancelled.get() == Some(request_id), cx));
                        }).detach();
                    }
                    SessionImportEvent::Cancel { request_id } => cancelled.set(Some(request_id)),
                    SessionImportEvent::OpenChat { chat_id } => {
                        let state = state.clone();
                        cx.defer(move |cx| {
                            state.update(cx, |state, cx| {
                                let device =
                                    state.chats.iter_mut().find(|chat| chat.id == chat_id).map(
                                        |chat| {
                                            chat.archived = false;
                                            chat.device_id.clone()
                                        },
                                    );
                                if let Some(device) = device {
                                    state.apply_transcript(transcript(&device));
                                }
                                cx.notify();
                            })
                        });
                    }
                    SessionImportEvent::Close => {}
                }
            }),
        };
        self.composer
            .read(cx)
            .pickers()
            .clone()
            .update(cx, |pickers, cx| pickers.fixture_model_catalog(cx));
        self.configure_session_import(controller, cx);
        if !matches!(scene.as_str(), "entry" | "fresh-entry") {
            self.open_session_import(window, cx);
        }
    }
}
