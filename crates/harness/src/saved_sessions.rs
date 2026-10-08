//! Provider-native history without document or execution dependencies.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeron_proto::{HarnessId, ToolCall};

use crate::HarnessError;

pub const MAX_HISTORY_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_RECORD_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_HISTORY_MESSAGES: usize = 20_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedSession {
    pub harness: HarnessId,
    pub native_id: String,
    pub store_id: String,
    pub title: String,
    pub cwd: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub model: Option<String>,
    /// Provider-local locator; never accepted from a UI request.
    pub locator: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct SavedSessionPage {
    pub sessions: Vec<SavedSession>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SavedMessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SavedHistoryPart {
    Text(String),
    Reasoning(String),
    Tool {
        call: ToolCall,
        output: Option<String>,
        is_error: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedHistoryMessage {
    pub id: String,
    pub role: SavedMessageRole,
    pub timestamp_ms: i64,
    pub parts: Vec<SavedHistoryPart>,
}

#[derive(Debug, Clone, Default)]
pub struct SavedSessionHistory {
    pub messages: Vec<SavedHistoryMessage>,
    pub notice: Option<String>,
}

impl SavedSessionHistory {
    pub fn add_notice(&mut self, text: &str) {
        if let Some(notice) = &mut self.notice {
            if !notice.contains(text) {
                notice.push(' ');
                notice.push_str(text);
            }
        } else {
            self.notice = Some(text.into());
        }
    }

    pub fn push_checked(
        &mut self,
        message: SavedHistoryMessage,
        bytes: &mut usize,
    ) -> Result<(), HarnessError> {
        *bytes += serde_json::to_vec(&message)
            .map_err(|e| HarnessError::Protocol(e.to_string()))?
            .len()
            + 1;
        if *bytes > MAX_HISTORY_BYTES || self.messages.len() >= MAX_HISTORY_MESSAGES {
            return Err(HarnessError::Protocol("Saved history exceeds the import limit; the native session has not been truncated.".into()));
        }
        self.messages.push(message);
        Ok(())
    }
    pub fn check_budget(&self) -> Result<(), HarnessError> {
        if self.messages.len() > MAX_HISTORY_MESSAGES
            || serde_json::to_vec(&self.messages)
                .map_err(|e| HarnessError::Protocol(e.to_string()))?
                .len()
                > MAX_HISTORY_BYTES
        {
            return Err(HarnessError::Protocol("Saved history exceeds the import limit; the native session has not been truncated.".into()));
        }
        Ok(())
    }
}

pub fn storage_root(
    override_root: Option<&Path>,
    variable: &str,
    default: &str,
) -> Result<PathBuf, HarnessError> {
    let path = override_root
        .map(Path::to_path_buf)
        .or_else(|| {
            std::env::var_os(variable)
                .filter(|p| !p.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| crate::executable::home_dir().map(|p| p.join(default)))
        .ok_or_else(|| {
            HarnessError::Protocol("The provider home directory is unavailable.".into())
        })?;
    Ok(std::fs::canonicalize(path)?)
}

pub fn store_id(harness: HarnessId, root: &Path) -> String {
    let mut hash = Sha256::new();
    hash.update(format!("{harness:?}\0"));
    hash.update(root.to_string_lossy().as_bytes());
    format!("{:x}", hash.finalize())
}

pub fn unsupported() -> HarnessError {
    HarnessError::Protocol("This provider does not support saved-session import.".into())
}
