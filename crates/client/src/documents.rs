//! Read-only documents on a conversation's execution host.
use std::time::Duration;

use base64::Engine as _;
use serde::Serialize;
use serde::de::DeserializeOwned;
use zeron_proto::{
    ReadWorkspaceFileRequest, ReadWorkspaceTextChunkRequest, WorkspaceFileText,
    WorkspaceReadOnlyReason, WorkspaceTarget, WorkspaceTextChunk,
};
use zeron_rpc::methods;

use crate::error::Result;
use crate::{Client, ClientError};

/// Deadline for one host call; a long document takes several.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// A document's text and the host context captured for the request.
pub struct HostDocument {
    pub device_id: String,
    pub cwd: Option<String>,
    /// `None` when the host has no text for the file (binary, too large, …);
    /// `read_only_reason` then says why.
    pub text: Option<String>,
    pub read_only_reason: Option<WorkspaceReadOnlyReason>,
    /// The file continues past `text`.
    pub truncated: bool,
}

impl Client {
    /// Read up to about `limit` bytes of a document through its chat's
    /// execution host, never the viewer's disk. A relative path names a file
    /// below the chat's folder, the base the agent and the transcript's
    /// images use. Only files inside the chat's workspace are returned.
    pub async fn read_chat_file(
        &self,
        chat_id: &str,
        path: &str,
        limit: usize,
    ) -> Result<HostDocument> {
        let workspace = self.workspace();
        let chat = workspace
            .session(chat_id)
            .ok_or_else(|| ClientError::NotFound("Document's chat is unavailable".into()))?;
        let target = WorkspaceTarget {
            chat_id: Some(chat_id.to_owned()),
            space_id: None,
            checkout_path: None,
        };
        let path = host_path(path, chat.cwd.as_deref());
        let chunked = self.read_text_chunks(&chat.device_id, &target, &path, limit).await;
        let (text, read_only_reason, truncated) =
            match chunked {
                // A host that predates chunked reads answers in one reply.
                Err(ClientError::Unsupported(_)) => {
                    let request = ReadWorkspaceFileRequest { target, path };
                    let file: WorkspaceFileText = self
                        .document_call(&chat.device_id, methods::READ_WORKSPACE_FILE, &request)
                        .await?;
                    inside_workspace(&file.checkout_id, file.read_only_reason)?;
                    (file.text, file.read_only_reason, file.truncated)
                }
                other => other?,
            };
        Ok(HostDocument {
            device_id: chat.device_id.clone(),
            cwd: chat.cwd.clone(),
            text,
            read_only_reason,
            truncated,
        })
    }

    /// `ReadWorkspaceTextChunk` until the text ends or `limit` bytes arrived:
    /// each reply stays far below the relay's message cap.
    async fn read_text_chunks(
        &self,
        device_id: &str,
        target: &WorkspaceTarget,
        path: &str,
        limit: usize,
    ) -> Result<(Option<String>, Option<WorkspaceReadOnlyReason>, bool)> {
        let mut bytes = Vec::new();
        let mut hash = None;
        loop {
            let request = ReadWorkspaceTextChunkRequest {
                target: target.clone(),
                path: path.to_owned(),
                offset: bytes.len(),
                expected_content_hash: hash.clone(),
            };
            let chunk: WorkspaceTextChunk = self
                .document_call(device_id, methods::READ_WORKSPACE_TEXT_CHUNK, &request)
                .await?;
            // Refused before any further slice is read.
            inside_workspace(&chunk.checkout_id, chunk.read_only_reason)?;
            if chunk.text_len.is_none() {
                return Ok((None, chunk.read_only_reason, false));
            }
            let data = base64::engine::general_purpose::STANDARD
                .decode(&chunk.data)
                .map_err(|e| ClientError::HostError(e.to_string()))?;
            if chunk.next_offset != bytes.len() + data.len()
                || (data.is_empty() && !chunk.done)
            {
                return Err(ClientError::HostError(
                    "The host returned an inconsistent document chunk".into(),
                ));
            }
            bytes.extend(data);
            hash = chunk.content_hash;
            if chunk.done || bytes.len() >= limit {
                // A slice may end inside a character; keep whole ones only.
                let valid = match std::str::from_utf8(&bytes) {
                    Ok(_) => bytes.len(),
                    Err(e) if e.error_len().is_none() && !chunk.done => e.valid_up_to(),
                    Err(e) => return Err(ClientError::HostError(e.to_string())),
                };
                bytes.truncate(valid);
                let text = String::from_utf8(bytes).map_err(|e| ClientError::HostError(e.to_string()))?;
                return Ok((Some(text), chunk.read_only_reason, !chunk.done));
            }
        }
    }

    async fn document_call<T: DeserializeOwned>(
        &self,
        device_id: &str,
        method: &str,
        request: &impl Serialize,
    ) -> Result<T> {
        let params = serde_json::to_value(request)
            .map_err(|e| ClientError::InvalidArgument(e.to_string()))?;
        let value = tokio::time::timeout(CALL_TIMEOUT, self.inner.host_rpc(device_id, method, params))
            .await
            .map_err(|_| {
                ClientError::HostUnavailable(
                    "Document read timed out; check the host connection and retry".into(),
                )
            })?
            .map_err(|error| document_error(method, error))?;
        serde_json::from_value(value).map_err(|e| ClientError::HostError(e.to_string()))
    }
}

/// The path the host reads. A relative path joins an absolute chat folder:
/// a project chat's host otherwise resolves it from the checkout root, which
/// differs when the project is a subfolder of its repository. A home-relative
/// folder (a projectless chat's) is the host's root for relative paths already.
fn host_path(path: &str, cwd: Option<&str>) -> String {
    match cwd.map(str::trim) {
        Some(cwd) if cwd.starts_with('/') && !path.starts_with('/') => {
            format!("{}/{}", cwd.trim_end_matches('/'), path.trim_start_matches("./"))
        }
        _ => path.to_owned(),
    }
}

/// Fail closed on a host file beyond the chat's workspace. The host marks an
/// outside read `OutsideWorkspace` only when nothing else already made it
/// read-only, but it always clears the checkout identity.
fn inside_workspace(checkout_id: &str, reason: Option<WorkspaceReadOnlyReason>) -> Result<()> {
    if checkout_id.is_empty() || reason == Some(WorkspaceReadOnlyReason::OutsideWorkspace) {
        return Err(ClientError::InvalidArgument(
            "This file is outside the conversation's workspace, so it can't be previewed.".into(),
        ));
    }
    Ok(())
}

/// Host failures without the RPC method prefix, in words for the viewer.
fn document_error(method: &str, error: ClientError) -> ClientError {
    match error {
        ClientError::HostError(message) => {
            let message = message
                .strip_prefix(method)
                .and_then(|rest| rest.strip_prefix(": "))
                .unwrap_or(&message);
            ClientError::HostError(match message {
                "file not found" => "This document wasn't found on the host.".into(),
                other => other.to_owned(),
            })
        }
        ClientError::Unsupported(_) if method == methods::READ_WORKSPACE_FILE => {
            ClientError::Unsupported(
                "The conversation's host can't preview documents; update Zeron there.".into(),
            )
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_join_an_absolute_chat_folder_only() {
        assert_eq!(host_path("docs/a.md", Some("/repo/pkg/")), "/repo/pkg/docs/a.md");
        assert_eq!(host_path("/etc/a.md", Some("/repo")), "/etc/a.md");
        assert_eq!(host_path("docs/a.md", Some("~/.zeron/x")), "docs/a.md");
        assert_eq!(host_path("docs/a.md", None), "docs/a.md");
    }

    #[test]
    fn outside_reads_fail_closed_whatever_else_made_them_read_only() {
        assert!(inside_workspace("co", None).is_ok());
        assert!(inside_workspace("co", Some(WorkspaceReadOnlyReason::TooLarge)).is_ok());
        assert!(inside_workspace("", Some(WorkspaceReadOnlyReason::TooLarge)).is_err());
        assert!(inside_workspace("", Some(WorkspaceReadOnlyReason::MixedLineEndings)).is_err());
        assert!(inside_workspace("co", Some(WorkspaceReadOnlyReason::OutsideWorkspace)).is_err());
        let error = document_error(
            methods::READ_WORKSPACE_TEXT_CHUNK,
            ClientError::HostError("ReadWorkspaceTextChunk: file not found".into()),
        );
        assert!(error.to_string().contains("wasn't found on the host"));
    }
}
