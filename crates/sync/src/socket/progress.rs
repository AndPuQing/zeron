//! Byte progress below WebSocket framing. A slow frame is healthy while its
//! bytes move, even though Stream::next or Sink::send has not completed.
use std::{
    future::Future,
    io,
    pin::Pin,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::Instant;
use tokio_tungstenite::WebSocketStream;

struct Activity {
    origin: Instant,
    read_ms: AtomicU64,
    write_ms: AtomicU64,
    completed_write_ms: AtomicU64,
    writing: AtomicBool,
    blocked_write: AtomicBool,
    route: OnceLock<(Arc<crate::edge_route::Router>, usize)>,
    route_failed: AtomicBool,
}

/// Shared byte-activity clock. It sends no extra frames and allocates once
/// per connection; updates are relaxed atomic stores, not wake broadcasts.
#[derive(Clone)]
pub struct Progress(Arc<Activity>);
impl Default for Progress {
    fn default() -> Self {
        Self(Arc::new(Activity {
            origin: Instant::now(),
            read_ms: AtomicU64::new(0),
            write_ms: AtomicU64::new(0),
            completed_write_ms: AtomicU64::new(0),
            writing: AtomicBool::new(false),
            blocked_write: AtomicBool::new(false),
            route: OnceLock::new(),
            route_failed: AtomicBool::new(false),
        }))
    }
}
impl Progress {
    pub(crate) fn set_route(&self, router: Arc<crate::edge_route::Router>, index: usize) {
        let _ = self.0.route.set((router, index));
    }
    pub(crate) fn transport_failed(&self) {
        if let Some((router, index)) = self.0.route.get()
            && !self.0.route_failed.swap(true, Ordering::Relaxed)
        {
            router.failed(*index);
        }
    }
    fn record(&self, write: bool) {
        let clock = if write {
            &self.0.write_ms
        } else {
            &self.0.read_ms
        };
        let ms = self.0.origin.elapsed().as_millis() as u64;
        clock.store(ms, Ordering::Relaxed);
    }
    fn latest(&self, write_only: bool) -> Instant {
        let write = self.0.write_ms.load(Ordering::Relaxed);
        let ms = if write_only {
            write
        } else {
            let received = self.0.read_ms.load(Ordering::Relaxed);
            let completed = self.0.completed_write_ms.load(Ordering::Relaxed);
            received
                .max(completed)
                .max(if self.0.writing.load(Ordering::Relaxed) {
                    write
                } else {
                    0
                })
        };
        self.0.origin + Duration::from_millis(ms)
    }
    pub(crate) fn writing(&self) -> impl Drop + '_ {
        self.0.blocked_write.store(false, Ordering::Relaxed);
        self.0.writing.store(true, Ordering::Relaxed);
        struct Guard<'a>(&'a Activity);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                // Give a slow completed send time to receive its reply.
                // Immediately buffered pings/echoes are not peer liveness.
                if self.0.blocked_write.load(Ordering::Relaxed) {
                    self.0
                        .completed_write_ms
                        .store(self.0.write_ms.load(Ordering::Relaxed), Ordering::Relaxed);
                }
                self.0.writing.store(false, Ordering::Relaxed);
            }
        }
        Guard(&self.0)
    }
    pub(crate) async fn while_progressing<F: Future>(
        &self,
        lease: Duration,
        write_only: bool,
        future: F,
    ) -> Result<F::Output, ()> {
        let started = Instant::now();
        tokio::pin!(future);
        loop {
            let deadline = self.latest(write_only).max(started) + lease;
            tokio::select! {
                // Poll I/O before its watchdog when both become ready.
                biased;
                result = futures::future::poll_fn(|cx| {
                    let result = future.as_mut().poll(cx);
                    if write_only && result.is_pending() {
                        self.0.blocked_write.store(true, Ordering::Relaxed);
                    }
                    result
                }) => return Ok(result),
                _ = tokio::time::sleep_until(deadline) => {
                    if self.latest(write_only).max(started) + lease <= Instant::now() {
                        self.transport_failed();
                        return Err(());
                    }
                }
            }
        }
    }
}

/// Couples a WebSocket with the activity clock of its underlying byte stream.
pub struct Connection<S> {
    pub socket: WebSocketStream<S>,
    pub progress: Progress,
}

/// Transparent stream wrapper: framing, encryption and payloads are unchanged.
pub struct ProgressIo<S> {
    inner: S,
    progress: Progress,
}
impl<S> ProgressIo<S> {
    pub fn new(inner: S) -> (Self, Progress) {
        let progress = Progress::default();
        (
            Self {
                inner,
                progress: progress.clone(),
            },
            progress,
        )
    }
}
impl<S: AsyncRead + Unpin> AsyncRead for ProgressIo<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let result = Pin::new(&mut this.inner).poll_read(cx, buf);
        if buf.filled().len() > before {
            this.progress.record(false);
        }
        if matches!(result, Poll::Ready(Err(_))) {
            this.progress.transport_failed();
        }
        result
    }
}
impl<S: AsyncWrite + Unpin> AsyncWrite for ProgressIo<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_write(cx, bytes);
        if matches!(result, Poll::Ready(Ok(n)) if n > 0) {
            this.progress.record(true);
        }
        if matches!(result, Poll::Ready(Err(_))) {
            this.progress.transport_failed();
        }
        result
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_flush(cx);
        if matches!(result, Poll::Ready(Err(_))) {
            this.progress.transport_failed();
        }
        result
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}
