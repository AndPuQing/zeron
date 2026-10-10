use chrono::DateTime;
use std::time::Duration;

use futures::future::{Either, select};
use gpui::{Context, Task};
use serde_json::json;
use zeron_proto::{ExternalSessionImportEvent, ExternalSessionList};
use zeron_rpc::methods;

use super::*;
use crate::state::EngineHandle;

#[derive(Default)]
pub(super) struct Requests {
    scan: Option<Task<()>>,
    import: Option<Task<()>>,
    cancel: Option<Task<()>>,
    operation: Option<(u64, String, String)>,
}

fn discovery(result: ExternalSessionList) -> SessionDiscovery {
    SessionDiscovery {
        sessions: result
            .sessions
            .into_iter()
            .filter_map(|row| {
                let provider = match row.harness {
                    zeron_proto::HarnessId::Codex => SessionProvider::Codex,
                    zeron_proto::HarnessId::ClaudeCode => SessionProvider::ClaudeCode,
                    _ => return None,
                };
                let eligibility = if let Some(chat_id) = row.already_managed_chat_id {
                    ImportEligibility::AlreadyManaged { chat_id }
                } else if let Some(reason) = row.unavailable_reason {
                    ImportEligibility::Unavailable { reason }
                } else if row.running {
                    ImportEligibility::Running
                } else {
                    ImportEligibility::Available
                };
                let project = row
                    .cwd
                    .rsplit(['/', '\\'])
                    .find(|s| !s.is_empty())
                    .unwrap_or(&row.cwd)
                    .to_owned();
                Some(ExternalSession {
                    source_ref: row.source_ref,
                    title: row.title,
                    provider,
                    project,
                    cwd: row.cwd,
                    updated_at: DateTime::from_timestamp_millis(row.updated_at_ms)
                        .unwrap_or_default(),
                    eligibility,
                })
            })
            .collect(),
        next_cursor: result.next_cursor,
        source_errors: result.source_errors,
    }
}

impl SessionImportDialog {
    pub(crate) fn handle_rpc_event(
        &mut self,
        engine: EngineHandle,
        event: SessionImportEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            SessionImportEvent::Discover {
                request_id,
                device_id,
                cursor,
            } => {
                self.rpc.scan = Some(cx.spawn(async move |this, cx| {
                    let params = json!({"targetDeviceId":device_id,"cursor":cursor});
                    let request = engine.client().call_as::<ExternalSessionList>(methods::LIST_EXTERNAL_SESSIONS, params);
                    let result = match select(Box::pin(request), cx.background_executor().timer(Duration::from_secs(150))).await {
                        Either::Left((result, _)) => result.map(discovery).map_err(|e| e.to_string()),
                        Either::Right(_) => Err("Session discovery timed out. Retry when the source device is reachable.".into()),
                    };
                    let _ = this.update(cx, |view,cx| view.receive_discovery(request_id, result, cx));
                }));
            }
            SessionImportEvent::Import {
                request_id,
                device_id,
                source_refs,
            } => {
                let operation_id = uuid::Uuid::new_v4().to_string();
                self.rpc.operation = Some((request_id, device_id.clone(), operation_id.clone()));
                self.rpc.import = Some(cx.spawn(async move |this,cx| {
                    let request = engine.client().subscribe_checked(methods::IMPORT_EXTERNAL_SESSIONS,
                        json!({"targetDeviceId":device_id,"operationId":operation_id,"sourceRefs":source_refs}));
                    let result = match select(Box::pin(request),cx.background_executor().timer(Duration::from_secs(30))).await {
                        Either::Left((result,_)) => result.map_err(|e| e.to_string()),
                        Either::Right(_) => Err("Could not start import on the source device. Try again.".into()),
                    };
                    let failure = match result {
                        Ok(mut stream) => loop {
                            let item = match select(Box::pin(stream.recv()),cx.background_executor().timer(Duration::from_secs(240))).await {
                                Either::Left((item,_)) => item,
                                Either::Right(_) => break "The source device stopped reporting import progress. Retry to recover the saved copy.".to_string(),
                            };
                            let Some(item) = item else { break "Import ended before all selected sessions were saved. Try again.".to_string(); };
                            let event = match serde_json::from_value::<ExternalSessionImportEvent>(item) {
                                Ok(event) => event,
                                Err(error) => break format!("Could not read import progress: {error}"),
                            };
                            let finished = matches!(&event, ExternalSessionImportEvent::Finished { .. });
                            if this.update(cx, |view,cx| match event {
                                ExternalSessionImportEvent::Importing { source_ref } => view.update_import_item(request_id,&source_ref,ImportItemState::Importing,cx),
                                ExternalSessionImportEvent::Imported { source_ref, chat_id } => view.update_import_item(request_id,&source_ref,ImportItemState::Finished(ImportOutcome::Imported {chat_id}),cx),
                                ExternalSessionImportEvent::AlreadyManaged { source_ref, chat_id } => view.update_import_item(request_id,&source_ref,ImportItemState::Finished(ImportOutcome::AlreadyManaged {chat_id}),cx),
                                ExternalSessionImportEvent::Failed { source_ref, reason, retryable } => view.update_import_item(request_id,&source_ref,ImportItemState::Finished(ImportOutcome::Failed {reason,retryable}),cx),
                                ExternalSessionImportEvent::Finished { cancelled } => view.finish_import(request_id,cancelled,cx),
                            }).is_err() || finished { return; }
                        },
                        Err(error) => error,
                    };
                    let _ = this.update(cx, |view,cx| {
                        for source_ref in source_refs {
                            view.update_import_item(request_id,&source_ref,ImportItemState::Finished(ImportOutcome::Failed {reason:failure.clone(),retryable:true}),cx);
                        }
                        view.finish_import(request_id,false,cx);
                    });
                }));
            }
            SessionImportEvent::Cancel { request_id } => {
                let Some((running, device_id, operation_id)) = self.rpc.operation.clone() else {
                    return;
                };
                if running != request_id {
                    return;
                }
                self.rpc.cancel = Some(cx.spawn(async move |this, cx| {
                    let request = engine.client().call(
                        methods::CANCEL_EXTERNAL_SESSION_IMPORT,
                        json!({"targetDeviceId":device_id,"operationId":operation_id}),
                    );
                    let result = select(
                        Box::pin(request),
                        cx.background_executor().timer(Duration::from_secs(30)),
                    )
                    .await;
                    let failed = !matches!(result, Either::Left((Ok(_), _)));
                    if failed {
                        let _ = this.update(cx, |view, cx| {
                            view.rpc.import = None;
                            view.finish_import(request_id, true, cx);
                        });
                    }
                }));
            }
            SessionImportEvent::OpenChat { .. } | SessionImportEvent::Close => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::StreamExt;
    use std::sync::{Arc, Mutex};
    use zeron_rpc::{RpcError, RpcReply, RpcService};

    #[derive(Default)]
    struct SourceDevice {
        calls: Mutex<Vec<(String, serde_json::Value)>>,
        progress: Mutex<Option<tokio::sync::mpsc::UnboundedSender<serde_json::Value>>>,
    }
    #[async_trait]
    impl RpcService for SourceDevice {
        async fn handle(
            &self,
            method: &str,
            params: serde_json::Value,
        ) -> Result<RpcReply, RpcError> {
            self.calls
                .lock()
                .unwrap()
                .push((method.into(), params.clone()));
            assert_eq!(params["targetDeviceId"], "remote");
            match method {
                methods::LIST_EXTERNAL_SESSIONS => RpcReply::value(&json!({"sessions":[
                    {"sourceRef":"a","harness":"codex","title":"A","cwd":"/project","updatedAtMs":2000},
                    {"sourceRef":"b","harness":"claude-code","title":"B","cwd":"/project","updatedAtMs":1000}],"nextCursor":null,"sourceErrors":[]})),
                methods::IMPORT_EXTERNAL_SESSIONS => {
                    assert!(uuid::Uuid::parse_str(params["operationId"].as_str().unwrap()).is_ok());
                    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
                    tx.send(json!({"kind":"importing","sourceRef":"a"}))
                        .unwrap();
                    tx.send(json!({"kind":"imported","sourceRef":"a","chatId":"copy-a"}))
                        .unwrap();
                    tx.send(json!({"kind":"importing","sourceRef":"b"}))
                        .unwrap();
                    *self.progress.lock().unwrap() = Some(tx);
                    Ok(RpcReply::Stream(
                        futures::stream::poll_fn(move |cx| rx.poll_recv(cx)).boxed(),
                    ))
                }
                methods::CANCEL_EXTERNAL_SESSION_IMPORT => {
                    self.progress
                        .lock()
                        .unwrap()
                        .take()
                        .unwrap()
                        .send(json!({"kind":"finished","cancelled":true}))
                        .unwrap();
                    RpcReply::value(&json!({"ok":true}))
                }
                _ => Err(RpcError::UnknownMethod(method.into())),
            }
        }
    }

    fn settle(
        view: &Entity<SessionImportDialog>,
        cx: &mut gpui::TestAppContext,
        runtime: &tokio::runtime::Runtime,
        ready: impl Fn(&SessionImportDialog) -> bool,
    ) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            runtime.block_on(tokio::task::yield_now());
            cx.run_until_parked();
            if view.read_with(cx, |view, _| ready(view)) {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "RPC adapter did not settle"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[gpui::test]
    fn adapter_routes_discovery_import_and_cancel_to_the_selected_device(
        cx: &mut gpui::TestAppContext,
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _runtime = runtime.enter();
        let service = Arc::new(SourceDevice::default());
        let engine = EngineHandle::from_test_client(zeron_rpc::memory_client(service.clone()));
        cx.update(|cx| {
            gpui_base::init(cx);
            cx.set_global(Theme::default());
            crate::composer::init(cx, Default::default());
        });
        let view = cx.new(|cx| {
            SessionImportDialog::new(
                vec![ImportDevice {
                    id: "remote".into(),
                    name: "Remote".into(),
                    current_project: None,
                }],
                "remote",
                cx,
            )
        });
        view.update(cx, |view, cx| {
            let event = view.model.discover(false).unwrap();
            view.handle_rpc_event(engine.clone(), event, cx);
        });
        settle(&view, cx, &runtime, |view| view.model.sessions.len() == 2);
        view.update(cx, |view, cx| {
            view.model.toggle_selection("a");
            view.model.toggle_selection("b");
            let event = view.model.begin_import(false).unwrap();
            view.handle_rpc_event(engine.clone(), event, cx);
        });
        settle(&view, cx, &runtime, |view| {
            view.model
                .batch
                .as_ref()
                .is_some_and(|b| b.imported() == 1 && b.states["b"] == ImportItemState::Importing)
        });
        view.update(cx, |view, cx| {
            let event = view.model.cancel().unwrap();
            view.handle_rpc_event(engine.clone(), event, cx);
        });
        settle(&view, cx, &runtime, |view| !view.is_importing());
        view.read_with(cx, |view, _| {
            let batch = view.model.batch.as_ref().unwrap();
            assert_eq!(batch.imported(), 1);
            assert_eq!(batch.states["b"], ImportItemState::Cancelled);
        });
        let calls = service.calls.lock().unwrap();
        let import = calls
            .iter()
            .find(|(m, _)| m == methods::IMPORT_EXTERNAL_SESSIONS)
            .unwrap();
        let cancel = calls
            .iter()
            .find(|(m, _)| m == methods::CANCEL_EXTERNAL_SESSION_IMPORT)
            .unwrap();
        assert_eq!(import.1["operationId"], cancel.1["operationId"]);
    }
}
