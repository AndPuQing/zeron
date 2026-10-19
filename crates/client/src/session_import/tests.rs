use super::*;
use tokio::sync::oneshot;

fn entry(id: &str) -> ExternalSessionEntry {
    ExternalSessionEntry {
        source_ref: id.into(),
        harness: HarnessId::Codex,
        title: id.into(),
        cwd: "/project".into(),
        updated_at_ms: 0,
        already_managed_chat_id: None,
        unavailable_reason: None,
        running: false,
    }
}
fn page(sessions: Vec<ExternalSessionEntry>, cursor: Option<&str>) -> ExternalSessionList {
    ExternalSessionList {
        sessions,
        next_cursor: cursor.map(str::to_owned),
        source_errors: vec![],
    }
}
enum Request {
    List(
        String,
        Option<String>,
        oneshot::Sender<Result<ExternalSessionList>>,
    ),
    Start(String, String, Vec<String>, oneshot::Sender<Result<Events>>),
    Cancel(String, String, oneshot::Sender<Result<()>>),
}
struct Fake(mpsc::UnboundedSender<Request>);
#[async_trait]
impl Transport for Fake {
    fn hosts(&self) -> Vec<ImportHost> {
        ["a", "b"]
            .into_iter()
            .map(|id| ImportHost {
                id: id.into(),
                name: id.into(),
                online: true,
                supported: true,
            })
            .collect()
    }
    async fn list(&self, device: &str, cursor: Option<String>) -> Result<ExternalSessionList> {
        let (tx, rx) = oneshot::channel();
        self.0
            .send(Request::List(device.into(), cursor, tx))
            .unwrap();
        rx.await.unwrap()
    }
    async fn start(&self, device: &str, operation: &str, refs: Vec<String>) -> Result<Events> {
        let (tx, rx) = oneshot::channel();
        self.0
            .send(Request::Start(device.into(), operation.into(), refs, tx))
            .unwrap();
        rx.await.unwrap()
    }
    async fn cancel(&self, device: &str, operation: &str) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.0
            .send(Request::Cancel(device.into(), operation.into(), tx))
            .unwrap();
        rx.await.unwrap()
    }
}
fn fixture() -> (Arc<ImportController>, mpsc::UnboundedReceiver<Request>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        ImportController::new(Arc::new(Fake(tx)), CancellationToken::new()),
        rx,
    )
}
async fn next(requests: &mut mpsc::UnboundedReceiver<Request>) -> Request {
    tokio::time::timeout(Duration::from_secs(5), requests.recv())
        .await
        .unwrap()
        .unwrap()
}
async fn wait(c: &ImportController, predicate: impl Fn(&ImportSnapshot) -> bool) -> ImportSnapshot {
    let mut changes = c.subscribe();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let snapshot = c.snapshot();
            if predicate(&snapshot) {
                return snapshot;
            }
            changes.changed().await.unwrap();
        }
    })
    .await
    .unwrap()
}
async fn discovered(
    entries: Vec<ExternalSessionEntry>,
) -> (Arc<ImportController>, mpsc::UnboundedReceiver<Request>) {
    let (c, mut requests) = fixture();
    let Request::List(device, cursor, reply) = next(&mut requests).await else {
        panic!("expected scan")
    };
    assert_eq!(device, "a");
    assert!(cursor.is_none());
    reply.send(Ok(page(entries, None))).unwrap();
    wait(&c, |s| !s.loading).await;
    (c, requests)
}

#[tokio::test]
async fn managed_first_page_keeps_pagination_and_partial_source_failures() {
    let (c, mut requests) = fixture();
    let Request::List(_, _, reply) = next(&mut requests).await else {
        panic!()
    };
    let mut managed = entry("managed");
    managed.already_managed_chat_id = Some("chat".into());
    reply
        .send(Ok(ExternalSessionList {
            source_errors: vec!["Claude scan unavailable".into()],
            ..page(vec![managed], Some("more"))
        }))
        .unwrap();
    let s = wait(&c, |s| !s.loading).await;
    assert!(s.rows.is_empty());
    assert!(s.has_more);
    assert_eq!(s.source_errors, ["Claude scan unavailable"]);
    c.load_more();
    let Request::List(device, cursor, reply) = next(&mut requests).await else {
        panic!()
    };
    assert_eq!(device, "a");
    assert_eq!(cursor.as_deref(), Some("more"));
    let mut running = entry("running");
    running.running = true;
    let mut missing = entry("missing");
    missing.unavailable_reason = Some("Directory missing".into());
    reply
        .send(Ok(page(vec![entry("ready"), running, missing], None)))
        .unwrap();
    let s = wait(&c, |s| !s.loading).await;
    assert_eq!(s.rows.len(), 3);
    assert!(!s.has_more);
    assert_eq!(
        s.rows
            .iter()
            .find(|r| r.source_ref == "missing")
            .unwrap()
            .disabled_reason
            .as_deref(),
        Some("Directory missing")
    );
    c.select_visible();
    assert_eq!(c.snapshot().selected_count, 1);
    c.toggle("running");
    c.toggle("missing");
    c.toggle("managed");
    assert_eq!(c.snapshot().selected_count, 1);
    c.set_query("no match".into());
    assert!(c.snapshot().rows.is_empty());
    assert_eq!(c.snapshot().selected_count, 1);
}

#[tokio::test]
async fn switching_hosts_discards_pending_discovery_and_selections() {
    let (c, mut requests) = fixture();
    let Request::List(_, _, old) = next(&mut requests).await else {
        panic!()
    };
    c.select_device("b");
    let Request::List(device, _, new) = next(&mut requests).await else {
        panic!()
    };
    assert_eq!(device, "b");
    new.send(Ok(page(vec![entry("from-b")], None))).unwrap();
    let s = wait(&c, |s| !s.loading).await;
    assert_eq!(s.rows[0].source_ref, "from-b");
    let _ = old.send(Ok(page(vec![entry("stale-a")], None)));
    c.toggle("from-b");
    c.select_device("a");
    assert_eq!(c.snapshot().selected_count, 0);
    assert!(c.snapshot().rows.is_empty());
    let Request::List(_, _, reply) = next(&mut requests).await else {
        panic!()
    };
    reply.send(Ok(page(vec![entry("fresh-a")], None))).unwrap();
    let s = wait(&c, |s| !s.loading).await;
    assert_eq!(s.device_id.as_deref(), Some("a"));
    assert_eq!(s.rows[0].source_ref, "fresh-a");
}

#[tokio::test]
async fn selection_limit_and_filters_preserve_explicit_choices() {
    let (c, _) = discovered(
        (0..LIMIT + 3)
            .map(|i| entry(&format!("item-{i:03}")))
            .collect(),
    )
    .await;
    c.select_visible();
    assert_eq!(c.snapshot().selected_count, LIMIT as u32);
    c.toggle("item-202");
    assert_eq!(c.snapshot().selected_count, LIMIT as u32);
    c.set_provider(Some("claude-code".into()));
    assert!(c.snapshot().rows.is_empty());
    assert_eq!(c.snapshot().selected_count, LIMIT as u32);
    c.set_provider(None);
    c.toggle("item-000");
    c.toggle("item-202");
    assert_eq!(c.snapshot().selected_count, LIMIT as u32);
    assert!(
        c.snapshot()
            .rows
            .iter()
            .find(|r| r.source_ref == "item-202")
            .unwrap()
            .selected
    );
}

#[tokio::test]
async fn select_visible_can_clear_at_the_cap_without_clearing_hidden_choices() {
    let (c, _) = discovered(
        (0..LIMIT + 3)
            .map(|i| entry(&format!("item-{i:03}")))
            .collect(),
    )
    .await;
    c.select_visible();
    assert_eq!(c.snapshot().selected_count, LIMIT as u32);
    c.set_query("item-00".into());
    c.select_visible();
    assert_eq!(c.snapshot().selected_count, (LIMIT - 10) as u32);
    assert!(c.snapshot().rows.iter().all(|r| !r.selected));
    c.set_query(String::new());
    assert!(
        c.snapshot()
            .rows
            .iter()
            .any(|r| r.source_ref == "item-010" && r.selected)
    );
    c.select_visible();
    assert_eq!(c.snapshot().selected_count, LIMIT as u32);
    c.select_visible();
    assert_eq!(c.snapshot().selected_count, 0);
}

#[tokio::test]
async fn cancellation_waits_for_start_and_keeps_the_current_items_durable_result() {
    let (c, mut requests) = discovered(vec![entry("one"), entry("two")]).await;
    c.select_visible();
    c.start(false);
    let Request::Start(device, operation, refs, started) = next(&mut requests).await else {
        panic!()
    };
    assert_eq!(device, "a");
    assert_eq!(refs, ["one", "two"]);
    c.cancel();
    assert!(c.snapshot().cancelling && c.snapshot().running);
    assert!(
        requests.try_recv().is_err(),
        "cancel must follow the checked start acknowledgement"
    );
    let (events, rx) = mpsc::channel(8);
    assert!(started.send(Ok(rx)).is_ok());
    let Request::Cancel(device, stopped, reply) = next(&mut requests).await else {
        panic!()
    };
    assert_eq!(device, "a");
    assert_eq!(stopped, operation);
    reply.send(Ok(())).unwrap();
    events
        .send(Ok(Event::Imported {
            source_ref: "one".into(),
            chat_id: "chat-one".into(),
        }))
        .await
        .unwrap();
    events
        .send(Ok(Event::Finished { cancelled: true }))
        .await
        .unwrap();
    let s = wait(&c, |s| !s.running).await;
    assert_eq!(s.results[0].phase, ImportPhase::Imported);
    assert!(!s.results[0].retryable);
    assert_eq!(s.results[1].phase, ImportPhase::Cancelled);
    assert!(s.results[1].retryable);
    assert_eq!(s.rows.len(), 1);
    assert_eq!(s.rows[0].source_ref, "two");
    c.start(true);
    let Request::Start(_, retry_operation, refs, started) = next(&mut requests).await else {
        panic!()
    };
    assert_ne!(operation, retry_operation);
    assert_eq!(refs, ["two"]);
    let (retry_events, rx) = mpsc::channel(8);
    assert!(started.send(Ok(rx)).is_ok());
    retry_events
        .send(Ok(Event::AlreadyManaged {
            source_ref: "two".into(),
            chat_id: "chat-two".into(),
        }))
        .await
        .unwrap();
    retry_events
        .send(Ok(Event::Finished { cancelled: false }))
        .await
        .unwrap();
    let s = wait(&c, |s| !s.running).await;
    assert!(s.rows.is_empty());
    assert_eq!(s.results[0].chat_id.as_deref(), Some("chat-one"));
    assert_eq!(s.results[1].phase, ImportPhase::AlreadyManaged);
}

#[tokio::test]
async fn dropping_a_picker_while_start_is_pending_still_cancels_on_the_host() {
    let (c, mut requests) = discovered(vec![entry("one")]).await;
    c.select_visible();
    c.start(false);
    let Request::Start(_, operation, _, started) = next(&mut requests).await else {
        panic!()
    };
    drop(c);
    let (events, rx) = mpsc::channel(8);
    // Both progress and cancellation may be ready when start acknowledges.
    events
        .send(Ok(Event::Importing {
            source_ref: "one".into(),
        }))
        .await
        .unwrap();
    assert!(started.send(Ok(rx)).is_ok());
    let Request::Cancel(_, stopped, reply) = next(&mut requests).await else {
        panic!()
    };
    assert_eq!(stopped, operation);
    reply.send(Ok(())).unwrap();
    let _ = events.send(Ok(Event::Finished { cancelled: true })).await;
}

#[tokio::test]
async fn a_broken_stream_preserves_successes_and_marks_only_unfinished_items_retryable() {
    let (c, mut requests) = discovered(vec![entry("one"), entry("two"), entry("three")]).await;
    c.select_visible();
    c.start(false);
    let Request::Start(_, _, _, started) = next(&mut requests).await else {
        panic!()
    };
    let (events, rx) = mpsc::channel(8);
    assert!(started.send(Ok(rx)).is_ok());
    events
        .send(Ok(Event::Imported {
            source_ref: "one".into(),
            chat_id: "chat-one".into(),
        }))
        .await
        .unwrap();
    events
        .send(Ok(Event::Failed {
            source_ref: "two".into(),
            reason: "Unsupported transcript".into(),
            retryable: false,
        }))
        .await
        .unwrap();
    drop(events);
    let Request::Cancel(_, _, reply) = next(&mut requests).await else {
        panic!()
    };
    reply.send(Ok(())).unwrap();
    let s = wait(&c, |s| !s.running).await;
    assert!(s.error.is_some());
    assert!(s.discovery_error.is_none());
    let result = |id: &str| s.results.iter().find(|r| r.source_ref == id).unwrap();
    assert_eq!(result("one").phase, ImportPhase::Imported);
    assert!(!result("two").retryable);
    assert!(result("three").retryable);
}

#[tokio::test]
async fn failed_pagination_can_retry_the_same_cursor_without_losing_selection() {
    let (c, mut requests) = fixture();
    let Request::List(_, _, reply) = next(&mut requests).await else {
        panic!()
    };
    reply
        .send(Ok(page(vec![entry("one")], Some("next"))))
        .unwrap();
    wait(&c, |s| !s.loading).await;
    c.toggle("one");
    c.load_more();
    let Request::List(_, cursor, reply) = next(&mut requests).await else {
        panic!()
    };
    assert_eq!(cursor.as_deref(), Some("next"));
    reply
        .send(Err(ClientError::HostUnavailable("Disconnected".into())))
        .unwrap();
    let s = wait(&c, |s| !s.loading).await;
    assert!(s.discovery_error.is_some());
    assert!(s.error.is_none());
    assert!(s.has_more);
    assert_eq!(s.selected_count, 1);
    c.load_more();
    let Request::List(_, cursor, reply) = next(&mut requests).await else {
        panic!()
    };
    assert_eq!(cursor.as_deref(), Some("next"));
    reply.send(Ok(page(vec![entry("two")], None))).unwrap();
    let s = wait(&c, |s| !s.loading).await;
    assert!(s.discovery_error.is_none());
    assert_eq!(s.selected_count, 1);
    assert_eq!(s.rows.len(), 2);
}

#[tokio::test]
async fn every_supported_harness_can_be_filtered_selected_and_submitted() {
    let entries = zeron_proto::SESSION_IMPORT_PROVIDERS
        .iter()
        .map(|(harness, key, _)| ExternalSessionEntry {
            harness: *harness,
            ..entry(key)
        })
        .collect();
    let (c, mut requests) = discovered(entries).await;
    for (_, key, _) in zeron_proto::SESSION_IMPORT_PROVIDERS {
        c.set_provider(Some((*key).into()));
        let s = c.snapshot();
        assert_eq!(s.rows.len(), 1);
        assert!(s.rows[0].disabled_reason.is_none());
        c.select_visible();
    }
    assert_eq!(
        c.snapshot().selected_count,
        zeron_proto::SESSION_IMPORT_PROVIDERS.len() as u32
    );
    c.start(false);
    let Request::Start(_, _, refs, reply) = next(&mut requests).await else {
        panic!()
    };
    assert_eq!(refs.len(), zeron_proto::SESSION_IMPORT_PROVIDERS.len());
    let (tx, rx) = mpsc::channel(8);
    reply.send(Ok(rx)).unwrap();
    tx.send(Ok(Event::Finished { cancelled: false }))
        .await
        .unwrap();
    wait(&c, |s| !s.running).await;
}
