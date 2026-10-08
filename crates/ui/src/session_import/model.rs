use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionProvider {
    Codex,
    ClaudeCode,
}

impl SessionProvider {
    pub fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::ClaudeCode => "Claude Code",
        }
    }

    pub fn harness(self) -> zeron_proto::HarnessId {
        match self {
            Self::Codex => zeron_proto::HarnessId::Codex,
            Self::ClaudeCode => zeron_proto::HarnessId::ClaudeCode,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ImportDevice {
    pub id: String,
    pub name: String,
    pub current_project: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportEligibility {
    Available,
    AlreadyManaged { chat_id: String },
    Unavailable { reason: String },
}

#[derive(Clone, Debug)]
pub struct ExternalSession {
    /// Opaque reference from the importer, never a client-supplied file path.
    pub source_ref: String,
    pub title: String,
    pub provider: SessionProvider,
    pub project: String,
    pub cwd: String,
    pub updated_at: DateTime<Utc>,
    pub eligibility: ImportEligibility,
}

#[derive(Clone, Debug, Default)]
pub struct SessionDiscovery {
    pub sessions: Vec<ExternalSession>,
    pub next_cursor: Option<String>,
    /// One source can fail while the other still returns usable sessions.
    pub source_errors: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportOutcome {
    Imported { chat_id: String },
    AlreadyManaged { chat_id: String },
    Failed { reason: String, retryable: bool },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportItemState {
    Pending,
    Importing,
    Finished(ImportOutcome),
    Cancelled,
}

impl ImportItemState {
    pub(super) fn chat_id(&self) -> Option<&str> {
        match self {
            Self::Finished(
                ImportOutcome::Imported { chat_id } | ImportOutcome::AlreadyManaged { chat_id },
            ) => Some(chat_id),
            _ => None,
        }
    }

    pub(super) fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Cancelled
                | Self::Finished(ImportOutcome::Failed {
                    retryable: true,
                    ..
                })
        )
    }

    fn is_terminal(&self) -> bool {
        matches!(self, Self::Finished(_) | Self::Cancelled)
    }
}

#[derive(Clone, Debug)]
pub enum SessionImportEvent {
    Discover {
        request_id: u64,
        device_id: String,
        cursor: Option<String>,
    },
    Import {
        request_id: u64,
        device_id: String,
        source_refs: Vec<String>,
    },
    Cancel {
        request_id: u64,
    },
    OpenChat {
        chat_id: String,
    },
    Close,
}

pub(super) struct ImportBatch {
    pub request_id: u64,
    pub rows: Vec<ExternalSession>,
    pub states: HashMap<String, ImportItemState>,
    pub running: bool,
    pub cancelling: bool,
}

impl ImportBatch {
    pub fn imported(&self) -> usize {
        self.states
            .values()
            .filter(|state| {
                matches!(
                    state,
                    ImportItemState::Finished(ImportOutcome::Imported { .. })
                )
            })
            .count()
    }

    pub fn completed(&self) -> usize {
        self.states
            .values()
            .filter(|state| state.is_terminal())
            .count()
    }

    pub fn retry_refs(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter(|row| self.states[&row.source_ref].retryable())
            .map(|row| row.source_ref.clone())
            .collect()
    }
}

pub(super) struct ImportModel {
    pub devices: Vec<ImportDevice>,
    pub device_index: usize,
    pub provider: Option<SessionProvider>,
    pub current_project_only: bool,
    pub sessions: Vec<ExternalSession>,
    pub selected: HashSet<String>,
    pub active: Option<String>,
    pub discovery: crate::popover::Loadable<()>,
    pub next_cursor: Option<String>,
    pub source_errors: Vec<String>,
    pub batch: Option<ImportBatch>,
    generation: u64,
    list_request: u64,
    append_list: bool,
}

impl ImportModel {
    pub fn new(devices: Vec<ImportDevice>, device_id: &str) -> Self {
        let device_index = devices
            .iter()
            .position(|device| device.id == device_id)
            .unwrap_or(0);
        let current_project_only = devices
            .get(device_index)
            .is_some_and(|d| d.current_project.is_some());
        Self {
            devices,
            device_index,
            provider: None,
            current_project_only,
            sessions: Vec::new(),
            selected: HashSet::new(),
            active: None,
            discovery: crate::popover::Loadable::Idle,
            next_cursor: None,
            source_errors: Vec::new(),
            batch: None,
            generation: 0,
            list_request: 0,
            append_list: false,
        }
    }

    fn next_request(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    pub fn discover(&mut self, more: bool) -> Option<SessionImportEvent> {
        let device_id = self.devices.get(self.device_index)?.id.clone();
        let cursor = if more {
            Some(self.next_cursor.clone()?)
        } else {
            None
        };
        self.append_list = more;
        self.list_request = self.next_request();
        self.discovery = crate::popover::Loadable::Loading;
        if !more {
            self.sessions.clear();
            self.selected.clear();
            self.active = None;
            self.next_cursor = None;
            self.source_errors.clear();
        }
        Some(SessionImportEvent::Discover {
            request_id: self.list_request,
            device_id,
            cursor,
        })
    }

    pub fn receive_discovery(
        &mut self,
        request_id: u64,
        result: Result<SessionDiscovery, String>,
    ) -> bool {
        if request_id != self.list_request || !self.discovery.is_loading() {
            return false;
        }
        match result {
            Ok(result) => {
                if !self.append_list {
                    self.sessions.clear();
                }
                for session in result.sessions {
                    if let Some(existing) = self
                        .sessions
                        .iter_mut()
                        .find(|row| row.source_ref == session.source_ref)
                    {
                        *existing = session;
                    } else {
                        self.sessions.push(session);
                    }
                }
                self.sessions
                    .sort_by_key(|row| std::cmp::Reverse(row.updated_at));
                self.selected.retain(|id| {
                    self.sessions.iter().any(|row| {
                        &row.source_ref == id && row.eligibility == ImportEligibility::Available
                    })
                });
                self.next_cursor = result.next_cursor;
                for error in result.source_errors {
                    if !self.source_errors.contains(&error) {
                        self.source_errors.push(error);
                    }
                }
                self.discovery = crate::popover::Loadable::Ready(());
            }
            Err(error) => self.discovery = crate::popover::Loadable::Error(error),
        }
        true
    }

    pub fn visible_sessions(&self, query: &str) -> Vec<&ExternalSession> {
        let query = query.trim().to_lowercase();
        let project = self
            .devices
            .get(self.device_index)
            .and_then(|d| d.current_project.as_deref());
        let mut rows = self
            .sessions
            .iter()
            .filter(|row| {
                self.provider
                    .is_none_or(|provider| row.provider == provider)
                    && (!self.current_project_only
                        || project.is_none_or(|path| {
                            row.cwd == path || crate::session_import::path_within(&row.cwd, path)
                        }))
                    && query.split_whitespace().all(|word| {
                        format!(
                            "{} {} {} {}",
                            row.title,
                            row.project,
                            row.cwd,
                            row.provider.label()
                        )
                        .to_lowercase()
                        .contains(word)
                    })
            })
            .collect::<Vec<_>>();
        let mut projects = HashMap::new();
        for row in &rows {
            let next = projects.len();
            projects.entry(row.cwd.as_str()).or_insert(next);
        }
        rows.sort_by_key(|row| projects[row.cwd.as_str()]);
        rows
    }

    pub fn toggle_selection(&mut self, source_ref: &str) {
        if self.batch.is_some()
            || !self.sessions.iter().any(|row| {
                row.source_ref == source_ref && row.eligibility == ImportEligibility::Available
            })
        {
            return;
        }
        if !self.selected.remove(source_ref)
            && self.selected.len() < zeron_proto::MAX_EXTERNAL_SESSION_IMPORT_BATCH
        {
            self.selected.insert(source_ref.to_owned());
        }
    }

    pub fn begin_import(&mut self, retry: bool) -> Option<SessionImportEvent> {
        let device_id = self.devices.get(self.device_index)?.id.clone();
        let mut rows = if retry {
            let batch = self.batch.as_ref().filter(|batch| !batch.running)?;
            let refs = batch.retry_refs();
            batch
                .rows
                .iter()
                .filter(|row| refs.contains(&row.source_ref))
                .cloned()
                .collect::<Vec<_>>()
        } else {
            if self.batch.is_some() {
                return None;
            }
            self.sessions
                .iter()
                .filter(|row| {
                    self.selected.contains(&row.source_ref)
                        && row.eligibility == ImportEligibility::Available
                })
                .cloned()
                .collect::<Vec<_>>()
        };
        if rows.is_empty() {
            return None;
        }
        let mut projects = HashMap::new();
        for row in &rows {
            let next = projects.len();
            projects.entry(row.cwd.clone()).or_insert(next);
        }
        rows.sort_by_key(|row| projects[&row.cwd]);
        let request_id = self.next_request();
        let source_refs = rows
            .iter()
            .map(|row| row.source_ref.clone())
            .collect::<Vec<_>>();
        if retry {
            let batch = self.batch.as_mut()?;
            batch.request_id = request_id;
            batch.running = true;
            batch.cancelling = false;
            for id in &source_refs {
                batch.states.insert(id.clone(), ImportItemState::Pending);
            }
        } else {
            self.batch = Some(ImportBatch {
                request_id,
                states: source_refs
                    .iter()
                    .map(|id| (id.clone(), ImportItemState::Pending))
                    .collect(),
                rows,
                running: true,
                cancelling: false,
            });
        }
        Some(SessionImportEvent::Import {
            request_id,
            device_id,
            source_refs,
        })
    }

    pub fn update_item(
        &mut self,
        request_id: u64,
        source_ref: &str,
        state: ImportItemState,
    ) -> bool {
        let Some(batch) = self
            .batch
            .as_mut()
            .filter(|batch| batch.running && batch.request_id == request_id)
        else {
            return false;
        };
        let Some(previous) = batch
            .states
            .get_mut(source_ref)
            .filter(|state| !state.is_terminal())
        else {
            return false;
        };
        if let Some(chat_id) = state.chat_id()
            && let Some(row) = self
                .sessions
                .iter_mut()
                .find(|row| row.source_ref == source_ref)
        {
            row.eligibility = ImportEligibility::AlreadyManaged {
                chat_id: chat_id.to_owned(),
            };
            self.selected.remove(source_ref);
        }
        *previous = state;
        true
    }

    pub fn cancel(&mut self) -> Option<SessionImportEvent> {
        let batch = self
            .batch
            .as_mut()
            .filter(|batch| batch.running && !batch.cancelling)?;
        batch.cancelling = true;
        Some(SessionImportEvent::Cancel {
            request_id: batch.request_id,
        })
    }

    pub fn finish_import(&mut self, request_id: u64, cancelled: bool) -> bool {
        let Some(batch) = self
            .batch
            .as_mut()
            .filter(|batch| batch.running && batch.request_id == request_id)
        else {
            return false;
        };
        for state in batch
            .states
            .values_mut()
            .filter(|state| !state.is_terminal())
        {
            *state = if cancelled {
                ImportItemState::Cancelled
            } else {
                ImportItemState::Finished(ImportOutcome::Failed {
                    reason: "Import ended before this session was saved. Try again.".into(),
                    retryable: true,
                })
            };
        }
        batch.running = false;
        batch.cancelling = false;
        true
    }
}
