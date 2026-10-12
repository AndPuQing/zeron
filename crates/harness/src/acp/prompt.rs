//! ACP prompt content that keeps images attached through queued follow-ups.
use serde_json::{Value, json};
use zeron_proto::HarnessId;

#[derive(Clone, Default)]
pub(super) struct Prompt {
    text: String,
    images: Vec<Value>,
}

impl std::ops::Deref for Prompt {
    type Target = str;
    fn deref(&self) -> &str {
        &self.text
    }
}

impl Prompt {
    pub async fn new(harness: HarnessId, text: String, attachments: &[String]) -> Self {
        use base64::Engine;
        use tokio::io::AsyncReadExt;
        let mut prompt = Self {
            text,
            images: Vec::new(),
        };
        // Copilot advertises native image prompts. Other ACP agents retain
        // their existing path-reference behavior.
        if harness != HarnessId::Copilot {
            return prompt;
        }
        const CAP: u64 = 20 * 1024 * 1024;
        for path in attachments {
            let bytes = async {
                let file = tokio::fs::File::open(path).await?;
                if file.metadata().await?.len() > CAP {
                    return Ok::<_, std::io::Error>(Vec::new());
                }
                let mut bytes = Vec::new();
                file.take(CAP + 1).read_to_end(&mut bytes).await?;
                Ok(bytes)
            }
            .await;
            let bytes = match bytes {
                Ok(bytes) if bytes.len() <= CAP as usize => bytes,
                Ok(_) => continue,
                Err(error) => {
                    tracing::warn!(%error, "Copilot image attachment unreadable; using path reference");
                    continue;
                }
            };
            let mime = if bytes.starts_with(b"\x89PNG") {
                "image/png"
            } else if bytes.starts_with(b"\xff\xd8\xff") {
                "image/jpeg"
            } else if bytes.starts_with(b"GIF8") {
                "image/gif"
            } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
                "image/webp"
            } else {
                continue;
            };
            prompt.images.push(json!({"type":"image", "mimeType":mime,
                "data":base64::engine::general_purpose::STANDARD.encode(bytes)}));
        }
        prompt
    }

    pub fn join(prompts: Vec<Self>) -> Self {
        let text = prompts
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        Self {
            text,
            images: prompts.into_iter().flat_map(|p| p.images).collect(),
        }
    }

    pub fn content(self) -> Vec<Value> {
        let mut content = vec![json!({"type":"text", "text":self.text})];
        content.extend(self.images);
        content
    }
}
