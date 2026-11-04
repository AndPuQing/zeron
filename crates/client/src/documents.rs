//! Read-only documents on a conversation's execution host.
use crate::error::Result;
use crate::{Client, ClientError};

/// File bytes and the host context captured for the request.
pub struct HostDocument {
    pub device_id: String,
    pub cwd: Option<String>,
    pub file: zeron_proto::WorkspaceFileText,
}

impl Client {
    /// Read a document through its chat's execution host, never the viewer's disk.
    pub async fn read_chat_file(&self, chat_id: &str, path: &str) -> Result<HostDocument> {
        let workspace = self.workspace();
        let chat = workspace
            .session(chat_id)
            .ok_or_else(|| ClientError::NotFound("Document's chat is unavailable".into()))?;
        let request = zeron_proto::ReadWorkspaceFileRequest {
            target: zeron_proto::WorkspaceTarget {
                chat_id: Some(chat_id.to_owned()),
                space_id: None,
                checkout_path: None,
            },
            path: path.to_owned(),
        };
        let params = serde_json::to_value(request)
            .map_err(|e| ClientError::InvalidArgument(e.to_string()))?;
        let value = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.inner.host_rpc(
                &chat.device_id,
                zeron_rpc::methods::READ_WORKSPACE_FILE,
                params,
            ),
        )
        .await
        .map_err(|_| {
            ClientError::HostUnavailable(
                "Document read timed out; check the host connection and retry".into(),
            )
        })??;
        let file =
            serde_json::from_value(value).map_err(|e| ClientError::HostError(e.to_string()))?;
        Ok(HostDocument {
            device_id: chat.device_id.clone(),
            cwd: chat.cwd.clone(),
            file,
        })
    }
}
