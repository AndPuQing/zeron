//! Happy-eyeballs WebSocket dialing (RFC 8305, simplified).
//!
//! `tokio_tungstenite::connect_async` tries resolved addresses SEQUENTIALLY
//! with no per-address bound, so a network that advertises IPv6 but blackholes
//! it (captive portals, airplane wifi) hangs the first SYN until the caller's
//! whole-dial timeout — and the retry repeats the identical order, so the
//! socket never comes up at all. Browsers (and this codebase's reqwest stack
//! via hyper-util) race address families and connect fine on the same network;
//! every WebSocket dial goes through here so they behave the same way:
//! resolve, interleave v6/v4, start one TCP attempt every [`STAGGER`] (or
//! immediately when the previous attempt fails), first connected stream wins
//! and the losers are dropped mid-flight.
//!
//! The winner also gets `TCP_NODELAY` — the protocols upstairs exchange
//! back-to-back small frames (join → eph-join, ack → push) that Nagle would
//! pointlessly pair with delayed ACKs.

use std::collections::VecDeque;
use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use futures::stream::{FuturesUnordered, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::error::{Error as WsError, UrlError};
use tokio_tungstenite::tungstenite::{handshake::client::Request, protocol::WebSocketConfig};

/// Delay before starting the next address attempt while one is still pending
/// (RFC 8305 §5's "Connection Attempt Delay"; 250ms is its recommended value).
const STAGGER: Duration = Duration::from_millis(250);

pub type WsStream = crate::socket::Connection<MaybeTlsStream<crate::socket::ProgressIo<TcpStream>>>;

/// Dial `url` (ws/wss) with happy-eyeballs TCP racing, then run TLS + the
/// WebSocket handshake on the winning stream. A success also broadcasts
/// [`crate::wake::notify_online`] so sibling sockets waiting out a reconnect
/// backoff redial immediately instead of sleeping through the recovery.
pub async fn connect_ws(url: &str) -> Result<WsStream, WsError> {
    connect_ws_request(url.into_client_request()?, None).await
}

/// The routed dialer also accepts explicit headers and framing limits.
pub async fn connect_ws_request(
    request: Request,
    config: Option<WebSocketConfig>,
) -> Result<WsStream, WsError> {
    // Bound DNS, all TCP attempts, TLS and HTTP upgrade together. Some
    // callers (notably the host relay) have no outer connection deadline.
    tokio::time::timeout(
        Duration::from_secs(20),
        connect_routed(request, config, crate::edge_route::shared()),
    )
    .await
    .map_err(|_| dial_timeout())?
}

fn dial_timeout() -> WsError {
    WsError::Io(io::Error::new(
        io::ErrorKind::TimedOut,
        "WebSocket dial timed out",
    ))
}

async fn connect_routed(
    request: Request,
    config: Option<WebSocketConfig>,
    router: &std::sync::Arc<crate::edge_route::Router>,
) -> Result<WsStream, WsError> {
    let url = reqwest::Url::parse(&request.uri().to_string()).ok();
    let Some(url) = url.filter(|url| router.index(url).is_some()) else {
        return connect_ws_inner(request, config).await;
    };
    let first = router.select().await;
    let mut index = first;
    for attempt in 0..2 {
        let mut routed = request.clone();
        let url = router.rewrite(&url, index);
        *routed.uri_mut() = url
            .as_str()
            .parse()
            .map_err(|_| WsError::Url(UrlError::NoHostName))?;
        // IntoClientRequest populated Host before the origin was selected.
        let host = routed
            .uri()
            .authority()
            .ok_or(WsError::Url(UrlError::NoHostName))?
            .as_str()
            .to_owned();
        routed.headers_mut().insert("host", host.parse()?);
        let result = tokio::time::timeout(Duration::from_secs(8), connect_ws_inner(routed, config))
            .await
            .unwrap_or_else(|_| Err(dial_timeout()));
        match result {
            Ok(connection) => {
                router.succeeded(index);
                connection.progress.set_route(router.clone(), index);
                return Ok(connection);
            }
            Err(error) => {
                let transport = match &error {
                    WsError::Http(response) => {
                        matches!(response.status().as_u16(), 408 | 502 | 503 | 504)
                    }
                    WsError::Io(_) | WsError::Tls(_) | WsError::Protocol(_) => true,
                    _ => false,
                };
                if !transport {
                    return Err(error);
                }
                router.failed(index);
                let next = router.select().await;
                if attempt == 1 || next == index || next == first {
                    return Err(error);
                }
                index = next;
            }
        }
    }
    unreachable!("bounded dial attempts")
}

async fn connect_ws_inner(
    request: Request,
    config: Option<WebSocketConfig>,
) -> Result<WsStream, WsError> {
    let uri = request.uri();
    let host = uri
        .host()
        .ok_or(WsError::Url(UrlError::NoHostName))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();
    let port = uri.port_u16().unwrap_or(match uri.scheme_str() {
        Some("wss") => 443,
        _ => 80,
    });
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
        .await?
        .collect();
    let stream = race_connect(interleave_families(addrs))
        .await
        .map_err(|err| io::Error::new(err.kind(), format!("{host}:{port}: {err}")))?;
    // Best-effort: a socket that works without NODELAY beats no socket.
    let _ = stream.set_nodelay(true);
    let (stream, progress) = crate::socket::ProgressIo::new(stream);
    let (ws, _response) =
        tokio_tungstenite::client_async_tls_with_config(request, stream, config, None).await?;
    crate::wake::notify_online();
    Ok(crate::socket::Connection {
        socket: ws,
        progress,
    })
}

/// Race TCP connects over an already-ordered address list: one new attempt per
/// [`STAGGER`] tick (immediately on a failure), first success wins, losers are
/// dropped mid-flight.
async fn race_connect(mut queue: VecDeque<SocketAddr>) -> io::Result<TcpStream> {
    let Some(first) = queue.pop_front() else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no addresses resolved",
        ));
    };
    let mut pending = FuturesUnordered::new();
    pending.push(TcpStream::connect(first));
    let stagger = tokio::time::sleep(STAGGER);
    tokio::pin!(stagger);

    loop {
        tokio::select! {
            res = pending.next(), if !pending.is_empty() => match res {
                Some(Ok(stream)) => return Ok(stream),
                Some(Err(err)) => {
                    // A failure advances the schedule immediately (§5).
                    match queue.pop_front() {
                        Some(addr) => {
                            pending.push(TcpStream::connect(addr));
                            stagger.as_mut().reset(tokio::time::Instant::now() + STAGGER);
                        }
                        // Last attempt standing: its error is the verdict.
                        None if pending.is_empty() => return Err(err),
                        None => {}
                    }
                }
                None => unreachable!("guarded on !pending.is_empty()"),
            },
            _ = stagger.as_mut(), if !queue.is_empty() => {
                if let Some(addr) = queue.pop_front() {
                    pending.push(TcpStream::connect(addr));
                }
                stagger.as_mut().reset(tokio::time::Instant::now() + STAGGER);
            }
        }
    }
}

/// Alternate address families starting with the resolver's first pick,
/// preserving resolver order within each family (RFC 8305 §4).
fn interleave_families(addrs: Vec<SocketAddr>) -> VecDeque<SocketAddr> {
    let first_is_v6 = addrs.first().is_some_and(SocketAddr::is_ipv6);
    let (preferred, other): (Vec<_>, Vec<_>) =
        addrs.into_iter().partition(|a| a.is_ipv6() == first_is_v6);
    let mut out = VecDeque::with_capacity(preferred.len() + other.len());
    let (mut preferred, mut other) = (preferred.into_iter(), other.into_iter());
    loop {
        match (preferred.next(), other.next()) {
            (None, None) => break,
            (a, b) => {
                out.extend(a);
                out.extend(b);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::SinkExt;
    use tokio::sync::mpsc;
    use tokio_tungstenite::tungstenite::{Message, http::Response};

    async fn entrance(
        status: Option<u16>,
    ) -> (
        String,
        mpsc::UnboundedReceiver<Request>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.unwrap();
                        let tx = tx.clone();
                        tasks.spawn(async move {
                            let socket = tokio_tungstenite::accept_hdr_async(stream, move |request: &Request, mut response: tokio_tungstenite::tungstenite::handshake::server::Response| {
                                if request.uri().path() == "/health" {
                                    let body = r#"{"ok":true,"auth":"workos"}"#;
                                    return Err(Response::builder().status(200).header("content-length", body.len()).body(Some(body.to_owned())).unwrap());
                                }
                                tx.send(request.clone()).unwrap();
                                if let Some(protocol) = request.headers().get("sec-websocket-protocol") {
                                    response.headers_mut().insert("sec-websocket-protocol", protocol.clone());
                                }
                                match status {
                                    Some(status) => Err(Response::builder().status(status).body(None).unwrap()),
                                    None => Ok(response),
                                }
                            }).await;
                            if let Ok(mut socket) = socket {
                                let _ = socket.send(Message::Text("x".repeat(2048))).await;
                                std::future::pending::<()>().await;
                            }
                        });
                    }
                    _ = tasks.join_next(), if !tasks.is_empty() => {}
                }
            }
        });
        (origin, rx, task)
    }

    #[tokio::test]
    async fn websocket_fails_over_sequentially_preserving_credentials_query_and_limits() {
        let (a, mut seen_a, task_a) = entrance(Some(503)).await;
        let (b, mut seen_b, task_b) = entrance(None).await;
        let router = crate::edge_route::Router::new([&a, &b]);
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("network-route.json"),
            serde_json::json!({"origin": a}).to_string(),
        )
        .unwrap();
        router.configure_cache(dir.path());
        let mut request = format!(
            "{}/preview/org/ws?device=abc&token=test",
            a.replace("http:", "ws:")
        )
        .into_client_request()
        .unwrap();
        request
            .headers_mut()
            .insert("authorization", "Bearer test-bearer".parse().unwrap());
        request
            .headers_mut()
            .insert("sec-websocket-protocol", "zerun-test".parse().unwrap());
        let mut config = WebSocketConfig::default();
        config.max_message_size = Some(1024);
        config.max_frame_size = Some(1024);
        let mut socket = connect_routed(request, Some(config), &router)
            .await
            .unwrap();
        let first = seen_a.recv().await.unwrap();
        let second = seen_b.recv().await.unwrap();
        assert_eq!(first.uri().path_and_query(), second.uri().path_and_query());
        assert_eq!(second.headers()["authorization"], "Bearer test-bearer");
        assert_eq!(second.headers()["sec-websocket-protocol"], "zerun-test");
        assert_eq!(second.headers()["host"], b.trim_start_matches("http://"));
        assert!(matches!(
            socket.socket.next().await.unwrap(),
            Err(WsError::Capacity(_))
        ));
        task_a.abort();
        task_b.abort();
    }

    #[tokio::test]
    async fn websocket_auth_rejection_does_not_try_the_other_entrance() {
        let (a, mut seen_a, task_a) = entrance(Some(401)).await;
        let (b, mut seen_b, task_b) = entrance(Some(401)).await;
        let router = crate::edge_route::Router::new([&a, &b]);
        let request = format!("{}/ws", a.replace("http:", "ws:"))
            .into_client_request()
            .unwrap();
        let error = connect_routed(request, None, &router).await.err().unwrap();
        assert!(matches!(error, WsError::Http(response) if response.status() == 401));
        let hits = usize::from(seen_a.try_recv().is_ok()) + usize::from(seen_b.try_recv().is_ok());
        assert_eq!(hits, 1);
        task_a.abort();
        task_b.abort();
    }

    fn addr(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    #[tokio::test]
    async fn stalled_upgrade_has_an_overall_deadline() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/", listener.local_addr().unwrap());
        let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            accepted_tx.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        let mut dial = tokio::spawn(async move { connect_ws(&url).await });
        tokio::time::timeout(Duration::from_secs(5), accepted_rx)
            .await
            .unwrap()
            .unwrap();
        // TCP is established. The peer never answers the HTTP upgrade.
        tokio::time::pause();
        let result = tokio::time::timeout(Duration::from_secs(21), &mut dial).await;
        server.abort();
        dial.abort();
        let err = result
            .expect("dial remained stuck after 21 seconds")
            .unwrap()
            .err()
            .expect("dial unexpectedly succeeded");
        assert!(matches!(err, WsError::Io(ref e) if e.kind() == io::ErrorKind::TimedOut));
    }

    #[test]
    fn interleaves_families_from_resolver_first_pick() {
        let ordered = interleave_families(vec![
            addr("[2001:db8::1]:443"),
            addr("[2001:db8::2]:443"),
            addr("192.0.2.1:443"),
            addr("192.0.2.2:443"),
            addr("192.0.2.3:443"),
        ]);
        let got: Vec<_> = ordered.into_iter().collect();
        assert_eq!(
            got,
            vec![
                addr("[2001:db8::1]:443"),
                addr("192.0.2.1:443"),
                addr("[2001:db8::2]:443"),
                addr("192.0.2.2:443"),
                addr("192.0.2.3:443"),
            ]
        );
    }

    #[test]
    fn single_family_passes_through() {
        let ordered = interleave_families(vec![addr("192.0.2.1:80"), addr("192.0.2.2:80")]);
        let got: Vec<_> = ordered.into_iter().collect();
        assert_eq!(got, vec![addr("192.0.2.1:80"), addr("192.0.2.2:80")]);
    }

    #[tokio::test]
    async fn blackholed_first_address_does_not_block_the_working_one() {
        // A listener that accepts (the "IPv4 works" side) behind an RFC 5737
        // TEST-NET address that never answers (the blackholed side). The race
        // must connect via the listener in ~STAGGER instead of hanging on the
        // black hole the way a sequential dial does.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let queue: VecDeque<SocketAddr> = vec![
            addr("192.0.2.1:9"),
            format!("127.0.0.1:{port}").parse().unwrap(),
        ]
        .into();
        let started = std::time::Instant::now();
        let stream = tokio::time::timeout(Duration::from_secs(5), race_connect(queue))
            .await
            .expect("raced connect hung on the blackholed address")
            .unwrap();
        assert_eq!(stream.peer_addr().unwrap().port(), port);
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "raced connect took {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn all_failures_surface_the_last_error() {
        // Two closed ports on loopback: both refuse, the error must surface
        // rather than the loop hanging or panicking.
        let a = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let b = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (pa, pb) = (
            a.local_addr().unwrap().port(),
            b.local_addr().unwrap().port(),
        );
        drop((a, b));
        let queue: VecDeque<SocketAddr> = vec![
            format!("127.0.0.1:{pa}").parse().unwrap(),
            format!("127.0.0.1:{pb}").parse().unwrap(),
        ]
        .into();
        let err = tokio::time::timeout(Duration::from_secs(5), race_connect(queue))
            .await
            .expect("failed race must resolve promptly")
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::ConnectionRefused);
    }
}
