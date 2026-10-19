//! Remote saved-session import. Native clients share selection, pagination and
//! operation lifetimes; only the host interprets opaque source references.
use crate::error::Result;
use crate::{Client, ClientError, client::Backend, lock};
use async_trait::async_trait;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;
use zeron_proto::{
    ExternalSessionEntry, ExternalSessionImportEvent as Event, ExternalSessionList, HarnessId,
};

const LIMIT: usize = zeron_proto::MAX_EXTERNAL_SESSION_IMPORT_BATCH;
type Events = mpsc::Receiver<Result<Event>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportHost {
    pub id: String,
    pub name: String,
    pub online: bool,
    pub supported: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportRow {
    pub source_ref: String,
    pub title: String,
    pub provider: String,
    pub cwd: String,
    pub updated_at_ms: i64,
    pub disabled_reason: Option<String>,
    pub selected: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportPhase {
    Pending,
    Importing,
    Imported,
    AlreadyManaged,
    Failed,
    Cancelled,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportResult {
    pub source_ref: String,
    pub title: String,
    pub phase: ImportPhase,
    pub reason: Option<String>,
    pub retryable: bool,
    pub chat_id: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportSnapshot {
    pub hosts: Vec<ImportHost>,
    pub device_id: Option<String>,
    pub rows: Vec<ImportRow>,
    pub query: String,
    pub provider: Option<String>,
    pub loading: bool,
    pub has_more: bool,
    pub error: Option<String>,
    pub discovery_error: Option<String>,
    pub source_errors: Vec<String>,
    pub selected_count: u32,
    pub selection_limit: u32,
    pub running: bool,
    pub cancelling: bool,
    pub results: Vec<ImportResult>,
}

#[async_trait]
trait Transport: Send + Sync {
    fn hosts(&self) -> Vec<ImportHost>;
    async fn list(&self, device: &str, cursor: Option<String>) -> Result<ExternalSessionList>;
    async fn start(&self, device: &str, operation: &str, refs: Vec<String>) -> Result<Events>;
    async fn cancel(&self, device: &str, operation: &str) -> Result<()>;
}
#[async_trait]
impl Transport for Client {
    fn hosts(&self) -> Vec<ImportHost> {
        self.workspace()
            .devices
            .iter()
            .filter(|d| d.is_execution_host)
            .map(|d| ImportHost {
                id: d.id.clone(),
                name: d.name.clone(),
                online: d.online,
                supported: d
                    .capabilities
                    .iter()
                    .any(|c| c == zeron_proto::capabilities::EXTERNAL_SESSION_IMPORT_V1),
            })
            .collect()
    }
    async fn list(&self, device: &str, cursor: Option<String>) -> Result<ExternalSessionList> {
        match self.inner.backend() {
            Backend::Demo(demo) => demo.list_external_sessions(device, cursor).await,
            Backend::Live(live) => {
                let v = live
                    .relay
                    .call(
                        device,
                        zeron_rpc::methods::LIST_EXTERNAL_SESSIONS,
                        serde_json::json!({"cursor": cursor}),
                    )
                    .await?;
                serde_json::from_value(v).map_err(|e| ClientError::HostError(e.to_string()))
            }
        }
    }
    async fn start(&self, device: &str, operation: &str, refs: Vec<String>) -> Result<Events> {
        match self.inner.backend() {
            Backend::Demo(demo) => demo.import_external_sessions(device, operation, refs),
            Backend::Live(live) => {
                let mut sub = live
                    .relay
                    .subscribe(
                        device,
                        zeron_rpc::methods::IMPORT_EXTERNAL_SESSIONS,
                        serde_json::json!({"operationId": operation, "sourceRefs": refs}),
                    )
                    .await?;
                let (tx, rx) = mpsc::channel(32);
                crate::runtime::shared().spawn(async move {
                    loop {
                        let value = tokio::select! {
                            _ = tx.closed() => break,
                            value = sub.recv() => match value { Some(v) => v, None => break },
                        };
                        let event = serde_json::from_value(value)
                            .map_err(|e| ClientError::HostError(e.to_string()));
                        if tx.send(event).await.is_err() {
                            break;
                        }
                    }
                });
                Ok(rx)
            }
        }
    }
    async fn cancel(&self, device: &str, operation: &str) -> Result<()> {
        match self.inner.backend() {
            Backend::Demo(demo) => {
                demo.cancel_external_import(operation);
                Ok(())
            }
            Backend::Live(live) => live
                .relay
                .call(
                    device,
                    zeron_rpc::methods::CANCEL_EXTERNAL_SESSION_IMPORT,
                    serde_json::json!({"operationId": operation}),
                )
                .await
                .map(|_| ()),
        }
    }
}

struct State {
    view: ImportSnapshot,
    entries: Vec<ExternalSessionEntry>,
    selected: HashSet<String>,
    cursor: Option<String>,
    generation: u64,
    scan: CancellationToken,
    operation: Option<(String, CancellationToken)>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            view: ImportSnapshot {
                selection_limit: LIMIT as u32,
                ..Default::default()
            },
            entries: vec![],
            selected: HashSet::new(),
            cursor: None,
            generation: 0,
            scan: CancellationToken::new(),
            operation: None,
        }
    }
}
impl State {
    fn visible(&self, row: &ExternalSessionEntry) -> bool {
        row.already_managed_chat_id.is_none()
            && self
                .view
                .provider
                .as_ref()
                .is_none_or(|p| p == provider(row.harness))
            && (self.view.query.trim().is_empty()
                || format!("{} {}", row.title, row.cwd)
                    .to_lowercase()
                    .contains(&self.view.query.trim().to_lowercase()))
    }
    fn snapshot(&self, hosts: Vec<ImportHost>) -> ImportSnapshot {
        let mut view = self.view.clone();
        view.hosts = hosts;
        view.has_more = self.cursor.is_some();
        view.selected_count = self.selected.len() as u32;
        view.rows = self
            .entries
            .iter()
            .filter(|row| self.visible(row))
            .map(|r| ImportRow {
                source_ref: r.source_ref.clone(),
                title: r.title.clone(),
                provider: provider(r.harness).into(),
                cwd: r.cwd.clone(),
                updated_at_ms: r.updated_at_ms,
                selected: self.selected.contains(&r.source_ref),
                disabled_reason: r
                    .unavailable_reason
                    .clone()
                    .or_else(|| r.running.then(|| "Session is still running".into()))
                    .or_else(|| {
                        zeron_proto::session_import_provider(r.harness)
                            .is_none()
                            .then(|| "This agent does not support import".into())
                    }),
            })
            .collect();
        view
    }
    fn finish(&mut self, cancelled: bool, error: Option<String>) {
        self.view.running = false;
        self.view.cancelling = false;
        self.operation = None;
        if error.is_some() {
            self.view.error = error.clone();
        }
        for row in &mut self.view.results {
            if matches!(row.phase, ImportPhase::Pending | ImportPhase::Importing) {
                row.phase = if cancelled {
                    ImportPhase::Cancelled
                } else {
                    ImportPhase::Failed
                };
                row.retryable = true;
                row.reason = error.clone().or_else(|| {
                    Some(
                        if cancelled {
                            "Cancelled"
                        } else {
                            "Import ended before a result arrived"
                        }
                        .into(),
                    )
                });
            }
        }
    }
    fn event(&mut self, event: Event) {
        let (source, phase, reason, retryable, chat) = match event {
            Event::Finished { cancelled } => {
                self.finish(cancelled, None);
                return;
            }
            Event::Importing { source_ref } => {
                (source_ref, ImportPhase::Importing, None, false, None)
            }
            Event::Imported {
                source_ref,
                chat_id,
            } => (
                source_ref,
                ImportPhase::Imported,
                None,
                false,
                Some(chat_id),
            ),
            Event::AlreadyManaged {
                source_ref,
                chat_id,
            } => (
                source_ref,
                ImportPhase::AlreadyManaged,
                None,
                false,
                Some(chat_id),
            ),
            Event::Failed {
                source_ref,
                reason,
                retryable,
            } => (
                source_ref,
                ImportPhase::Failed,
                Some(reason),
                retryable,
                None,
            ),
        };
        if let Some(row) = self
            .view
            .results
            .iter_mut()
            .find(|r| r.source_ref == source)
        {
            // Never let a late progress event replace a durable success.
            if matches!(
                row.phase,
                ImportPhase::Imported | ImportPhase::AlreadyManaged
            ) {
                return;
            }
            row.phase = phase;
            row.reason = reason;
            row.retryable = retryable;
            row.chat_id = chat.clone();
            if let Some(chat) = chat {
                if let Some(entry) = self.entries.iter_mut().find(|r| r.source_ref == source) {
                    entry.already_managed_chat_id = Some(chat);
                }
                self.selected.remove(&source);
            }
        }
    }
}

/// Own one per visible import screen; closing it cancels scanning and asks the
/// host to stop the current batch. Work already published remains imported.
pub struct ImportController {
    transport: Arc<dyn Transport>,
    state: Mutex<State>,
    changed: watch::Sender<u64>,
    closed: CancellationToken,
}
impl Drop for ImportController {
    fn drop(&mut self) {
        self.closed.cancel();
        lock(&self.state).scan.cancel();
    }
}
impl Client {
    pub fn session_import(&self) -> Arc<ImportController> {
        ImportController::new(Arc::new(self.clone()), self.inner.cancel.child_token())
    }
}
impl ImportController {
    fn new(transport: Arc<dyn Transport>, closed: CancellationToken) -> Arc<Self> {
        let (changed, _) = watch::channel(0);
        let this = Arc::new(Self {
            transport,
            state: Mutex::new(State::default()),
            changed,
            closed,
        });
        if let Some(host) = this
            .transport
            .hosts()
            .into_iter()
            .find(|h| h.online && h.supported)
        {
            this.select_device(&host.id);
        }
        this
    }
    fn notify(&self) {
        self.changed.send_modify(|v| *v = v.wrapping_add(1));
    }
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }
    pub fn snapshot(&self) -> ImportSnapshot {
        lock(&self.state).snapshot(self.transport.hosts())
    }
    pub fn update_hosts(self: &Arc<Self>) {
        if lock(&self.state).view.device_id.is_none() {
            if let Some(host) = self
                .transport
                .hosts()
                .into_iter()
                .find(|h| h.online && h.supported)
            {
                self.select_device(&host.id);
            }
        }
        self.notify();
    }
    pub fn close(&self) {
        self.closed.cancel();
        lock(&self.state).scan.cancel();
    }
    pub fn select_device(self: &Arc<Self>, id: &str) {
        if self.closed.is_cancelled() {
            return;
        }
        if !self
            .transport
            .hosts()
            .iter()
            .any(|h| h.id == id && h.online && h.supported)
        {
            return;
        }
        {
            let mut s = lock(&self.state);
            if s.view.running || s.view.device_id.as_deref() == Some(id) {
                return;
            }
            s.scan.cancel();
            let generation = s.generation.wrapping_add(1);
            *s = State::default();
            s.generation = generation;
            s.view.device_id = Some(id.into());
        }
        self.refresh();
    }
    pub fn set_query(&self, query: String) {
        lock(&self.state).view.query = query;
        self.notify();
    }
    pub fn set_provider(&self, provider: Option<String>) {
        if provider.as_deref().is_some_and(|p| {
            !zeron_proto::SESSION_IMPORT_PROVIDERS
                .iter()
                .any(|(_, key, _)| *key == p)
        }) {
            return;
        }
        lock(&self.state).view.provider = provider;
        self.notify();
    }
    pub fn toggle(&self, source: &str) {
        let mut s = lock(&self.state);
        if self.closed.is_cancelled() || s.view.running || s.view.loading {
            return;
        }
        if s.selected.remove(source) {
            drop(s);
            self.notify();
            return;
        }
        if s.selected.len() < LIMIT
            && s.entries
                .iter()
                .any(|r| r.source_ref == source && selectable(r))
        {
            s.selected.insert(source.into());
        }
        drop(s);
        self.notify();
    }
    pub fn select_visible(&self) {
        let mut s = lock(&self.state);
        if self.closed.is_cancelled() || s.view.running || s.view.loading {
            return;
        }
        let refs: Vec<_> = s
            .entries
            .iter()
            .filter(|r| s.visible(r) && selectable(r))
            .map(|r| r.source_ref.clone())
            .collect();
        // At the batch cap a partial selection cannot grow. Let the same
        // control clear the visible selection instead of becoming a dead end.
        let clear = !refs.is_empty()
            && (refs.iter().all(|r| s.selected.contains(r))
                || (s.selected.len() == LIMIT && refs.iter().any(|r| s.selected.contains(r))));
        for r in refs {
            if clear {
                s.selected.remove(&r);
            } else if s.selected.len() < LIMIT {
                s.selected.insert(r);
            }
        }
        drop(s);
        self.notify();
    }
    pub fn refresh(self: &Arc<Self>) {
        self.scan(false);
    }
    pub fn load_more(self: &Arc<Self>) {
        self.scan(true);
    }
    fn scan(self: &Arc<Self>, append: bool) {
        if self.closed.is_cancelled() {
            return;
        }
        let (device, cursor, generation, cancel) = {
            let mut s = lock(&self.state);
            if s.view.running || (append && (s.view.loading || s.cursor.is_none())) {
                return;
            }
            let Some(device) = s.view.device_id.clone() else {
                return;
            };
            s.scan.cancel();
            s.scan = self.closed.child_token();
            s.generation = s.generation.wrapping_add(1);
            if !append {
                s.entries.clear();
                s.selected.clear();
                s.cursor = None;
                s.view.source_errors.clear();
                s.view.results.clear();
                s.view.error = None;
            }
            s.view.loading = true;
            s.view.discovery_error = None;
            (device, s.cursor.clone(), s.generation, s.scan.clone())
        };
        self.notify();
        let weak = Arc::downgrade(self);
        let transport = self.transport.clone();
        crate::runtime::shared().spawn(async move {
            let reply = tokio::select! { _ = cancel.cancelled() => return, r = transport.list(&device, cursor) => r };
            let Some(this) = weak.upgrade() else { return; };
            let mut s = lock(&this.state);
            if generation != s.generation || cancel.is_cancelled() { return; }
            s.view.loading = false;
            match reply {
                Err(e) => s.view.discovery_error = Some(e.to_string()),
                Ok(page) => {
                    s.cursor = page.next_cursor;
                    for error in page.source_errors { if !s.view.source_errors.contains(&error) { s.view.source_errors.push(error); } }
                    for row in page.sessions {
                        if let Some(old) = s.entries.iter_mut().find(|r| r.source_ref == row.source_ref) { *old = row; }
                        else { s.entries.push(row); }
                    }
                    s.entries.sort_by(|a,b| b.updated_at_ms.cmp(&a.updated_at_ms).then(a.source_ref.cmp(&b.source_ref)));
                    let valid: HashSet<_> = s.entries.iter().filter(|r| selectable(r)).map(|r| r.source_ref.clone()).collect();
                    s.selected.retain(|r| valid.contains(r));
                }
            }
            drop(s); this.notify();
        });
    }
    pub fn start(self: &Arc<Self>, retry: bool) {
        if self.closed.is_cancelled() {
            return;
        }
        let (device, operation, refs, cancel) = {
            let mut s = lock(&self.state);
            if s.view.running || s.view.loading {
                return;
            }
            let Some(device) = s.view.device_id.clone() else {
                return;
            };
            if !self
                .transport
                .hosts()
                .iter()
                .any(|h| h.id == device && h.online && h.supported)
            {
                s.view.error = Some("The selected host is offline or needs an update".into());
                drop(s);
                self.notify();
                return;
            }
            let refs: Vec<_> = if retry {
                s.view
                    .results
                    .iter()
                    .filter(|r| r.retryable)
                    .map(|r| r.source_ref.clone())
                    .take(LIMIT)
                    .collect()
            } else {
                s.entries
                    .iter()
                    .filter(|r| s.selected.contains(&r.source_ref) && selectable(r))
                    .map(|r| r.source_ref.clone())
                    .take(LIMIT)
                    .collect()
            };
            if refs.is_empty() {
                return;
            }
            if retry {
                for r in &mut s.view.results {
                    if refs.contains(&r.source_ref) {
                        r.phase = ImportPhase::Pending;
                        r.reason = None;
                        r.retryable = false;
                    }
                }
            } else {
                s.view.results = refs
                    .iter()
                    .map(|id| ImportResult {
                        source_ref: id.clone(),
                        title: s
                            .entries
                            .iter()
                            .find(|r| &r.source_ref == id)
                            .map_or_else(|| id.clone(), |r| r.title.clone()),
                        phase: ImportPhase::Pending,
                        reason: None,
                        retryable: false,
                        chat_id: None,
                    })
                    .collect();
            }
            let operation = crate::new_id();
            let cancel = self.closed.child_token();
            s.view.running = true;
            s.view.error = None;
            s.operation = Some((operation.clone(), cancel.clone()));
            (device, operation, refs, cancel)
        };
        self.notify();
        let weak = Arc::downgrade(self);
        let transport = self.transport.clone();
        crate::runtime::shared().spawn(async move {
            // Finish the checked start before cancelling: cancel-before-start
            // would otherwise miss an operation not yet registered on the host.
            let mut events = match transport.start(&device, &operation, refs).await {
                Ok(events) => events,
                Err(e) => {
                    // The acknowledgement may have been lost after the host
                    // started; explicitly stop the uncertain operation too.
                    let _ = transport.cancel(&device, &operation).await;
                    finish(&weak, cancel.is_cancelled(), Some(e.to_string()));
                    return;
                }
            };
            let mut stop_sent = false;
            loop {
                let timeout = Duration::from_secs(if stop_sent { 30 } else { 240 });
                let event = tokio::select! {
                    _ = cancel.cancelled(), if !stop_sent => {
                        if let Err(e) = transport.cancel(&device, &operation).await {
                            finish(&weak, true, Some(format!("Could not stop import: {e}")));
                            return;
                        }
                        // The host may finish publishing its current item.
                        // Keep those results until Finished before enabling retry.
                        stop_sent = true;
                        continue;
                    }
                    event = tokio::time::timeout(timeout, events.recv()) => event,
                };
                match event {
                    Ok(Some(Ok(event))) => {
                        let done = matches!(event, Event::Finished { .. });
                        let Some(this) = weak.upgrade() else {
                            // Progress can win the select after the picker is
                            // dropped; the host still needs its stop request.
                            if !done && !stop_sent {
                                let _ = transport.cancel(&device, &operation).await;
                            }
                            return;
                        };
                        lock(&this.state).event(event); this.notify();
                        if done { return; }
                    }
                    other => {
                        let reason = match other {
                            Ok(Some(Err(e))) => e.to_string(),
                            Ok(None) => "Connection ended before import finished. Retry to check remaining sessions.".into(),
                            _ => "Import timed out. Retry to check remaining sessions.".into(),
                        };
                        let _ = transport.cancel(&device, &operation).await;
                        finish(&weak, stop_sent, Some(reason)); return;
                    }
                }
            }
        });
    }
    pub fn cancel(&self) {
        let mut s = lock(&self.state);
        if let Some((_, cancel)) = &s.operation {
            cancel.cancel();
            s.view.cancelling = true;
        }
        drop(s);
        self.notify();
    }
}
fn selectable(row: &ExternalSessionEntry) -> bool {
    row.already_managed_chat_id.is_none()
        && row.unavailable_reason.is_none()
        && !row.running
        && zeron_proto::session_import_provider(row.harness).is_some()
}
fn finish(weak: &Weak<ImportController>, cancelled: bool, error: Option<String>) {
    if let Some(this) = weak.upgrade() {
        lock(&this.state).finish(cancelled, error);
        this.notify();
    }
}

#[cfg(test)]
mod tests;

fn provider(harness: HarnessId) -> &'static str {
    zeron_proto::session_import_provider(harness).map_or("unsupported", |(key, _)| key)
}
