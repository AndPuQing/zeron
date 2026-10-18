//! OMP RPC is JSONL, not JSON-RPC 2.0. It opens with a `ready` frame; the host
//! may then `negotiate_protocol` v2, whose oversized logical frames arrive as
//! ordered `rpc_chunk` sequences on the same stdout channel. Responses and
//! events share one ordered channel, so a response is a barrier after every
//! preceding lifecycle event.
//!
//! Frame rules mirror OMP's `src/modes/rpc/rpc-frame.ts`: one physical line is
//! at most 1 MiB, one reassembled logical frame at most 64 MiB, chunk payloads
//! are 256 KiB of base64. A chunk sequence that is interrupted, mismatched or
//! oversized is a protocol failure rather than silently dropped bytes.
use crate::HarnessError;
use base64::Engine;
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    sync::mpsc,
    task::JoinHandle,
};

/// OMP's physical frame ceiling is 1 MiB; allow generous JSON overhead.
const MAX_FRAME: usize = 4 * 1024 * 1024;
/// OMP chunks a logical frame only when it exceeds its 1 MiB physical ceiling.
const MIN_CHUNKED_FRAME: usize = 1024 * 1024;
/// OMP's reassembled logical frame ceiling.
const MAX_REASSEMBLED: usize = 64 * 1024 * 1024;
const CHUNK_PAYLOAD: usize = 256 * 1024;
const MAX_CHUNKS: usize = MAX_REASSEMBLED / CHUNK_PAYLOAD;

#[derive(Clone)]
pub(super) struct Client {
    writer: mpsc::UnboundedSender<Value>,
    next_id: Arc<AtomicU64>,
}
impl Client {
    pub fn send(&self, frame: Value) -> Result<(), HarnessError> {
        self.writer
            .send(frame)
            .map_err(|_| HarnessError::Protocol("OMP stdin closed".into()))
    }
    pub fn request(&self, mut frame: Value) -> Result<String, HarnessError> {
        let id = format!("zeron-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        frame["id"] = Value::String(id.clone());
        self.send(frame)?;
        Ok(id)
    }
}

pub(super) struct Transport {
    pub client: Client,
    pub incoming: mpsc::Receiver<Result<Value, HarnessError>>,
    reader: JoinHandle<()>,
    writer: JoinHandle<()>,
}
impl Drop for Transport {
    fn drop(&mut self) {
        self.reader.abort();
        self.writer.abort();
    }
}
impl Transport {
    pub fn new(
        input: impl AsyncWrite + Unpin + Send + 'static,
        output: impl AsyncRead + Unpin + Send + 'static,
    ) -> Self {
        let (writer_tx, mut writer_rx) = mpsc::unbounded_channel::<Value>();
        let (tx, incoming) = mpsc::channel(256);
        let errors = tx.clone();
        let writer = tokio::spawn(async move {
            let mut input = input;
            while let Some(frame) = writer_rx.recv().await {
                let mut bytes = serde_json::to_vec(&frame).expect("JSON value");
                bytes.push(b'\n');
                let result = async {
                    input.write_all(&bytes).await?;
                    input.flush().await
                }
                .await;
                if let Err(e) = result {
                    let _ = errors.send(Err(e.into())).await;
                    break;
                }
            }
        });
        let reader = tokio::spawn(async move {
            let mut reader = BufReader::new(output);
            let mut bytes = Vec::new();
            let mut chunks = Reassembler::default();
            loop {
                // fill_buf bounds allocation before a hostile/noisy peer sends LF.
                let chunk = match reader.fill_buf().await {
                    Ok([]) => {
                        let _ = tx
                            .send(Err(HarnessError::Protocol("OMP stdout closed".into())))
                            .await;
                        break;
                    }
                    Ok(chunk) => chunk,
                    Err(e) => {
                        let _ = tx.send(Err(e.into())).await;
                        break;
                    }
                };
                let count = chunk
                    .iter()
                    .position(|b| *b == b'\n')
                    .map_or(chunk.len(), |i| i + 1);
                if bytes.len() + count > MAX_FRAME {
                    let _ = tx
                        .send(Err(HarnessError::Protocol(
                            "OMP RPC frame exceeds 4 MiB".into(),
                        )))
                        .await;
                    break;
                }
                bytes.extend_from_slice(&chunk[..count]);
                reader.consume(count);
                if bytes.last() != Some(&b'\n') {
                    continue;
                }
                if bytes.iter().all(u8::is_ascii_whitespace) {
                    bytes.clear();
                    continue;
                }
                let frame = serde_json::from_slice::<Value>(&bytes)
                    .map_err(|e| HarnessError::Protocol(format!("Invalid OMP RPC JSON: {e}")));
                bytes.clear();
                let frame = match frame.and_then(|value| chunks.push(value)) {
                    Ok(Some(frame)) => Ok(frame),
                    Ok(None) => continue,
                    Err(e) => Err(e),
                };
                let failed = frame.is_err();
                if tx.send(frame).await.is_err() || failed {
                    break;
                }
            }
        });
        Self {
            client: Client {
                writer: writer_tx,
                next_id: Arc::new(AtomicU64::new(1)),
            },
            incoming,
            reader,
            writer,
        }
    }
}

/// Reassembles protocol-v2 `rpc_chunk` sequences. Plain frames pass through.
#[derive(Default)]
struct Reassembler {
    pending: Option<Pending>,
}
struct Pending {
    chunk_id: String,
    count: usize,
    byte_length: usize,
    next_index: usize,
    bytes: Vec<u8>,
}
impl Reassembler {
    fn push(&mut self, value: Value) -> Result<Option<Value>, HarnessError> {
        if value["type"] != "rpc_chunk" {
            if self.pending.is_some() {
                return Err(HarnessError::Protocol(
                    "OMP RPC chunk sequence interrupted".into(),
                ));
            }
            return Ok(Some(value));
        }
        let chunk_id = value["chunkId"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 128)
            .ok_or_else(|| HarnessError::Protocol("Invalid OMP RPC chunk metadata".into()))?
            .to_owned();
        let index = value["index"]
            .as_u64()
            .filter(|n| *n <= MAX_CHUNKS as u64)
            .ok_or_else(|| HarnessError::Protocol("Invalid OMP RPC chunk metadata".into()))?
            as usize;
        let count = value["count"]
            .as_u64()
            .filter(|n| (2..=MAX_CHUNKS as u64).contains(n))
            .ok_or_else(|| HarnessError::Protocol("Invalid OMP RPC chunk metadata".into()))?
            as usize;
        let byte_length = value["byteLength"]
            .as_u64()
            .filter(|n| *n >= MIN_CHUNKED_FRAME as u64 && *n <= MAX_REASSEMBLED as u64)
            .ok_or_else(|| HarnessError::Protocol("Invalid OMP RPC chunk metadata".into()))?
            as usize;
        if index >= count {
            return Err(HarnessError::Protocol(
                "Invalid OMP RPC chunk metadata".into(),
            ));
        }
        let data = value["data"]
            .as_str()
            .ok_or_else(|| HarnessError::Protocol("Invalid OMP RPC chunk data".into()))?;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| HarnessError::Protocol("Invalid OMP RPC chunk data".into()))?;
        if decoded.len() > CHUNK_PAYLOAD {
            return Err(HarnessError::Protocol(
                "OMP RPC chunk payload exceeds the transport limit".into(),
            ));
        }
        if self.pending.is_none() {
            if index != 0 {
                return Err(HarnessError::Protocol(
                    "OMP RPC chunk sequence must start at index 0".into(),
                ));
            }
            self.pending = Some(Pending {
                chunk_id: chunk_id.clone(),
                count,
                byte_length,
                next_index: 0,
                bytes: Vec::new(),
            });
        }
        let pending = self.pending.as_mut().expect("pending chunk sequence");
        if pending.chunk_id != chunk_id
            || pending.count != count
            || pending.byte_length != byte_length
            || pending.next_index != index
        {
            return Err(HarnessError::Protocol(
                "OMP RPC chunk sequence mismatch".into(),
            ));
        }
        pending.bytes.extend_from_slice(&decoded);
        pending.next_index += 1;
        if pending.bytes.len() > pending.byte_length {
            return Err(HarnessError::Protocol(
                "OMP RPC chunk sequence exceeds declared length".into(),
            ));
        }
        if pending.next_index < pending.count {
            return Ok(None);
        }
        let pending = self.pending.take().expect("completed chunk sequence");
        if pending.bytes.len() != pending.byte_length {
            return Err(HarnessError::Protocol(
                "OMP RPC chunk sequence length mismatch".into(),
            ));
        }
        let text = String::from_utf8(pending.bytes)
            .map_err(|_| HarnessError::Protocol("OMP RPC chunk is not UTF-8".into()))?;
        let frame: Value = serde_json::from_str(&text)
            .map_err(|e| HarnessError::Protocol(format!("Invalid OMP RPC JSON: {e}")))?;
        if !frame.is_object() {
            return Err(HarnessError::Protocol(
                "OMP RPC frame must be an object".into(),
            ));
        }
        Ok(Some(frame))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn fragmented_utf8_and_responses_preserve_wire_order() {
        let (host, peer) = tokio::io::duplex(8192);
        let (read, write) = tokio::io::split(host);
        let (peer_read, mut peer_write) = tokio::io::split(peer);
        let mut transport = Transport::new(write, read);
        let id = transport
            .client
            .request(json!({"type":"get_state"}))
            .unwrap();
        let mut lines = BufReader::new(peer_read).lines();
        let request: Value =
            serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(request["id"], id);
        assert!(request.get("jsonrpc").is_none());
        let frames = [
            json!({"type":"message_update", "text":"ñ\u{2028}ok"}),
            json!({"type":"session_settled"}),
            json!({"type":"response","id":id,"success":true}),
        ];
        for frame in &frames {
            for byte in format!("{frame}\r\n").bytes() {
                peer_write.write_all(&[byte]).await.unwrap();
            }
        }
        for expected in frames {
            assert_eq!(transport.incoming.recv().await.unwrap().unwrap(), expected);
        }
        peer_write.shutdown().await.unwrap();
        drop(peer_write);
        assert!(transport.incoming.recv().await.unwrap().is_err());
    }

    #[tokio::test]
    async fn malformed_json_is_a_protocol_failure() {
        let (host, mut peer) = tokio::io::duplex(64);
        let (read, write) = tokio::io::split(host);
        let mut transport = Transport::new(write, read);
        peer.write_all(b"not json\n").await.unwrap();
        assert!(
            transport
                .incoming
                .recv()
                .await
                .unwrap()
                .unwrap_err()
                .to_string()
                .contains("Invalid OMP")
        );
    }

    #[test]
    fn chunk_sequences_reassemble_and_reject_mismatch() {
        let payload = "x".repeat(1200 * 1024);
        let encoded = serde_json::to_vec(&json!({"type":"message_end","message":payload})).unwrap();
        let chunks: Vec<_> = encoded.chunks(CHUNK_PAYLOAD).collect();
        let count = chunks.len();
        assert!(count >= 2);
        let mut r = Reassembler::default();
        let mut out = None;
        for (index, chunk) in chunks.iter().enumerate() {
            let frame = json!({
                "type":"rpc_chunk",
                "chunkId":"rpc-1",
                "index":index,
                "count":count,
                "byteLength":encoded.len(),
                "data":base64::engine::general_purpose::STANDARD.encode(chunk),
            });
            out = r.push(frame).unwrap();
            assert_eq!(out.is_some(), index + 1 == count);
        }
        let frame = out.unwrap();
        assert_eq!(frame["type"], "message_end");
        assert_eq!(frame["message"].as_str().unwrap().len(), 1200 * 1024);

        // An interrupted sequence is a protocol error, never a silent drop.
        let mut r = Reassembler::default();
        assert!(
            r.push(json!({
                "type":"rpc_chunk","chunkId":"a","index":0,"count":2,
                "byteLength":2 * 1024 * 1024,
                "data":base64::engine::general_purpose::STANDARD.encode(b"half"),
            }))
            .unwrap()
            .is_none()
        );
        assert!(r.push(json!({"type":"session_settled"})).is_err());
    }
}
