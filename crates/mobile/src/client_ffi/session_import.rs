//! Native import picker facade. All state transitions live in zeron-client.
use super::*;
use zeron_client::session_import as imp;

#[derive(Clone, uniffi::Record)]
pub struct SessionImportHost {
    pub id: String,
    pub name: String,
    pub online: bool,
    pub supported: bool,
}
#[derive(Clone, uniffi::Record)]
pub struct SessionImportRow {
    pub source_ref: String,
    pub title: String,
    pub provider: String,
    pub cwd: String,
    pub updated_at_ms: i64,
    pub disabled_reason: Option<String>,
    pub selected: bool,
}
#[derive(Clone, Copy, uniffi::Enum)]
pub enum SessionImportPhase {
    Pending,
    Importing,
    Imported,
    AlreadyManaged,
    Failed,
    Cancelled,
}
#[derive(Clone, uniffi::Record)]
pub struct SessionImportResult {
    pub source_ref: String,
    pub title: String,
    pub phase: SessionImportPhase,
    pub reason: Option<String>,
    pub retryable: bool,
    pub chat_id: Option<String>,
}
#[derive(Clone, uniffi::Record)]
pub struct SessionImportSnapshot {
    pub hosts: Vec<SessionImportHost>,
    pub device_id: Option<String>,
    pub rows: Vec<SessionImportRow>,
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
    pub results: Vec<SessionImportResult>,
}
impl From<imp::ImportSnapshot> for SessionImportSnapshot {
    fn from(v: imp::ImportSnapshot) -> Self {
        Self {
            hosts: v
                .hosts
                .into_iter()
                .map(|h| SessionImportHost {
                    id: h.id,
                    name: h.name,
                    online: h.online,
                    supported: h.supported,
                })
                .collect(),
            device_id: v.device_id,
            rows: v
                .rows
                .into_iter()
                .map(|r| SessionImportRow {
                    source_ref: r.source_ref,
                    title: r.title,
                    provider: r.provider,
                    cwd: r.cwd,
                    updated_at_ms: r.updated_at_ms,
                    disabled_reason: r.disabled_reason,
                    selected: r.selected,
                })
                .collect(),
            query: v.query,
            provider: v.provider,
            loading: v.loading,
            has_more: v.has_more,
            error: v.error,
            discovery_error: v.discovery_error,
            source_errors: v.source_errors,
            selected_count: v.selected_count,
            selection_limit: v.selection_limit,
            running: v.running,
            cancelling: v.cancelling,
            results: v
                .results
                .into_iter()
                .map(|r| SessionImportResult {
                    source_ref: r.source_ref,
                    title: r.title,
                    phase: match r.phase {
                        imp::ImportPhase::Pending => SessionImportPhase::Pending,
                        imp::ImportPhase::Importing => SessionImportPhase::Importing,
                        imp::ImportPhase::Imported => SessionImportPhase::Imported,
                        imp::ImportPhase::AlreadyManaged => SessionImportPhase::AlreadyManaged,
                        imp::ImportPhase::Failed => SessionImportPhase::Failed,
                        imp::ImportPhase::Cancelled => SessionImportPhase::Cancelled,
                    },
                    reason: r.reason,
                    retryable: r.retryable,
                    chat_id: r.chat_id,
                })
                .collect(),
        }
    }
}
#[uniffi::export(with_foreign)]
pub trait SessionImportListener: Send + Sync {
    /// Delivered on a client worker: dispatch to the platform's UI thread.
    fn on_import_changed(&self, snapshot: SessionImportSnapshot);
}
#[derive(uniffi::Object)]
pub struct CoreSessionImport {
    controller: Arc<imp::ImportController>,
    observer: tokio::task::JoinHandle<()>,
}
impl Drop for CoreSessionImport {
    fn drop(&mut self) {
        self.controller.close();
        self.observer.abort();
    }
}
#[uniffi::export]
impl CoreSessionImport {
    pub fn snapshot(&self) -> SessionImportSnapshot {
        self.controller.snapshot().into()
    }
    pub fn select_device(&self, device_id: String) {
        self.controller.select_device(&device_id);
    }
    pub fn set_query(&self, query: String) {
        self.controller.set_query(query);
    }
    pub fn set_provider(&self, provider: Option<String>) {
        self.controller.set_provider(provider);
    }
    pub fn toggle(&self, source_ref: String) {
        self.controller.toggle(&source_ref);
    }
    pub fn select_visible(&self) {
        self.controller.select_visible();
    }
    pub fn update_hosts(&self) {
        self.controller.update_hosts();
    }
    pub fn refresh(&self) {
        self.controller.refresh();
    }
    pub fn load_more(&self) {
        self.controller.load_more();
    }
    pub fn start(&self, retry: bool) {
        self.controller.start(retry);
    }
    pub fn cancel(&self) {
        self.controller.cancel();
    }
    pub fn dismiss(&self) {
        self.controller.close();
        self.observer.abort();
    }
}
#[uniffi::export]
impl CoreClient {
    pub fn session_import(
        &self,
        listener: Arc<dyn SessionImportListener>,
    ) -> Arc<CoreSessionImport> {
        let controller = self.client.session_import();
        let mut changes = controller.subscribe();
        let weak = Arc::downgrade(&controller);
        let observer = zc::runtime::shared().spawn(async move {
            loop {
                let Some(controller) = weak.upgrade() else {
                    return;
                };
                listener.on_import_changed(controller.snapshot().into());
                drop(controller);
                if changes.changed().await.is_err() {
                    return;
                }
            }
        });
        Arc::new(CoreSessionImport {
            controller,
            observer,
        })
    }
}
