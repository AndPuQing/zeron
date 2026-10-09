//! Saved provider sessions: discovery is read-only; import creates a native copy.

use serde::{Deserialize, Serialize};

use crate::HarnessId;

pub const MAX_EXTERNAL_SESSION_IMPORT_BATCH: usize = 200;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionImportSource {
    pub harness: HarnessId,
    pub device_id: String,
    /// Identity of the provider's canonical storage root on the source device.
    pub store_id: String,
    pub original_native_id: String,
    pub copied_native_id: String,
    pub imported_at_ms: i64,
    #[serde(default)]
    pub history_notice: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalSessionEntry {
    pub source_ref: String,
    pub harness: HarnessId,
    pub title: String,
    pub cwd: String,
    pub updated_at_ms: i64,
    #[serde(default)]
    pub already_managed_chat_id: Option<String>,
    #[serde(default)]
    pub unavailable_reason: Option<String>,
    /// The source's latest turn was still running when it was listed.
    #[serde(default)]
    pub running: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalSessionList {
    pub sessions: Vec<ExternalSessionEntry>,
    pub next_cursor: Option<String>,
    pub source_errors: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListExternalSessionsParams {
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportExternalSessionsParams {
    pub operation_id: String,
    pub source_refs: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelExternalSessionImportParams {
    pub operation_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ExternalSessionImportEvent {
    Importing {
        source_ref: String,
    },
    Imported {
        source_ref: String,
        chat_id: String,
    },
    AlreadyManaged {
        source_ref: String,
        chat_id: String,
    },
    Failed {
        source_ref: String,
        reason: String,
        retryable: bool,
    },
    Finished {
        cancelled: bool,
    },
}
