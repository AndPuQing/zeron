//! Offline host fixture uses the same discovery and progress protocol as relay.
use super::*;
use tokio::sync::mpsc;
use zeron_proto::{
    ExternalSessionEntry, ExternalSessionImportEvent as Event, ExternalSessionList, HarnessId,
};

fn entries(device: &str) -> Vec<ExternalSessionEntry> {
    [
        (
            "codex",
            "Improve import navigation",
            HarnessId::Codex,
            None,
            false,
        ),
        (
            "claude",
            "Review the deployment workflow",
            HarnessId::ClaudeCode,
            None,
            false,
        ),
        (
            "managed",
            "Already in Sessions",
            HarnessId::Codex,
            None,
            false,
        ),
        (
            "running",
            "Refactor the session sidebar",
            HarnessId::Codex,
            None,
            true,
        ),
        (
            "missing",
            "Archived project notes",
            HarnessId::ClaudeCode,
            Some("Project directory no longer exists"),
            false,
        ),
        (
            "retry",
            "Polish the mobile layout",
            HarnessId::ClaudeCode,
            None,
            false,
        ),
        (
            "dsh",
            "Review the import pipeline",
            HarnessId::Dsh,
            None,
            false,
        ),
        ("pi", "Trace the session tree", HarnessId::Pi, None, false),
        (
            "opencode",
            "Verify native session forks",
            HarnessId::Opencode,
            None,
            false,
        ),
    ]
    .into_iter()
    .enumerate()
    .map(
        |(i, (id, title, harness, reason, running))| ExternalSessionEntry {
            source_ref: format!("{device}:{id}"),
            harness,
            title: title.into(),
            cwd: if id == "missing" {
                "/Users/dev/old-project"
            } else {
                "/Users/dev/zeron"
            }
            .into(),
            updated_at_ms: now_ms() - i as i64 * 3_600_000,
            already_managed_chat_id: (id == "managed").then(|| "chat-deploy".into()),
            unavailable_reason: reason.map(str::to_owned),
            running,
        },
    )
    .collect()
}
impl DemoHost {
    pub(crate) async fn list_external_sessions(
        &self,
        device: &str,
        cursor: Option<String>,
    ) -> Result<ExternalSessionList> {
        tokio::time::sleep(Duration::from_millis(180)).await;
        let mut rows = entries(device);
        for row in &mut rows {
            if let Some(chat) = lock(&self.imported).get(&row.source_ref) {
                row.already_managed_chat_id = Some(chat.clone());
            }
        }
        let more = cursor.is_none();
        Ok(ExternalSessionList {
            sessions: if more {
                rows[..5].to_vec()
            } else {
                rows[5..].to_vec()
            },
            next_cursor: more.then(|| "second-page".into()),
            source_errors: vec![],
        })
    }
    pub(crate) fn cancel_external_import(&self, operation: &str) {
        if let Some(cancel) = lock(&self.import_operations).get(operation) {
            cancel.cancel();
        }
    }
    pub(crate) fn import_external_sessions(
        self: &Arc<Self>,
        device: &str,
        operation: &str,
        refs: Vec<String>,
    ) -> Result<mpsc::Receiver<Result<Event>>> {
        let (tx, rx) = mpsc::channel(32);
        let cancel = self.client()?.cancel.child_token();
        lock(&self.import_operations).insert(operation.into(), cancel.clone());
        let host = self.clone();
        let device = device.to_owned();
        let operation = operation.to_owned();
        crate::runtime::shared().spawn(async move {
            let available = entries(&device);
            for source in refs {
                if cancel.is_cancelled() { break; }
                if tx.send(Ok(Event::Importing { source_ref: source.clone() })).await.is_err() { break; }
                tokio::select! { _ = cancel.cancelled() => break, _ = tokio::time::sleep(Duration::from_millis(300)) => {} }
                let result = host.demo_import_one(&device, &source, &available);
                if tx.send(result).await.is_err() { break; }
            }
            let _ = tx.send(Ok(Event::Finished { cancelled: cancel.is_cancelled() })).await;
            lock(&host.import_operations).remove(&operation);
        });
        Ok(rx)
    }
    fn demo_import_one(
        &self,
        device: &str,
        source: &str,
        entries: &[ExternalSessionEntry],
    ) -> Result<Event> {
        let Some(entry) = entries.iter().find(|r| r.source_ref == source) else {
            return Ok(Event::Failed {
                source_ref: source.into(),
                reason: "Source no longer exists".into(),
                retryable: false,
            });
        };
        if let Some(chat) = lock(&self.imported).get(source) {
            return Ok(Event::AlreadyManaged {
                source_ref: source.into(),
                chat_id: chat.clone(),
            });
        }
        if source.ends_with(":retry") && lock(&self.import_attempted).insert(source.into()) {
            return Ok(Event::Failed {
                source_ref: source.into(),
                reason: "Host temporarily unavailable. Try again.".into(),
                retryable: true,
            });
        }
        if entry.running
            || entry.unavailable_reason.is_some()
            || entry.already_managed_chat_id.is_some()
        {
            return Ok(Event::Failed {
                source_ref: source.into(),
                reason: "Session is unavailable".into(),
                retryable: false,
            });
        }
        let client = self.client()?;
        let base = client
            .workspace
            .chat("chat-deploy")
            .ok_or_else(|| ClientError::NotFound("demo source".into()))?;
        let id = crate::new_id();
        let mut chat = Chat {
            id: id.clone(),
            device_id: device.into(),
            title: Some(entry.title.clone()),
            cwd: Some(entry.cwd.clone()),
            parent_chat_id: None,
            branch: None,
            checkout_id: None,
            source_context: None,
            archived: true,
            created_at: Utc::now(),
            last_message_at: Some(Utc::now()),
            harness_session_id: None,
            harness_session_cwd: None,
            space_id: None,
            import_source: None,
            ..base
        };
        if let Some(config) = &mut chat.config {
            config.harness = entry.harness;
        }
        client.registry_write(|doc| doc.upsert_chat(&chat))?;
        lock(&self.imported).insert(source.into(), id.clone());
        Ok(Event::Imported {
            source_ref: source.into(),
            chat_id: id,
        })
    }
}
