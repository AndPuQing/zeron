use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Server {
    origin: String,
    requests: Arc<Mutex<Vec<String>>>,
    probes: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn server(
    status: u16,
    drop_response: bool,
    health_delay: Duration,
    health_body: &'static str,
) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let probes = Arc::new(AtomicUsize::new(0));
    let seen = requests.clone();
    let health = probes.clone();
    let task = tokio::spawn(async move {
        let mut tasks = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let (mut stream, _) = accepted.unwrap();
                    let seen = seen.clone();
                    let health = health.clone();
                    tasks.spawn(async move {
                        let mut bytes = Vec::new();
                        let header_end = loop {
                            let mut buf = [0; 1024];
                            let n = stream.read(&mut buf).await.unwrap();
                            if n == 0 { return; }
                            bytes.extend_from_slice(&buf[..n]);
                            if let Some(end) = bytes.windows(4).position(|p| p == b"\r\n\r\n") { break end + 4; }
                        };
                        let headers = String::from_utf8_lossy(&bytes[..header_end]);
                        let length = headers.lines().find_map(|line| line.to_lowercase().strip_prefix("content-length:").map(|n| n.trim().parse::<usize>().unwrap())).unwrap_or(0);
                        while bytes.len() < header_end + length {
                            let mut buf = [0; 1024];
                            let n = stream.read(&mut buf).await.unwrap();
                            if n == 0 { return; }
                            bytes.extend_from_slice(&buf[..n]);
                        }
                        let request = String::from_utf8(bytes).unwrap();
                        let probe = request.starts_with("GET /health ");
                        let (status, body) = if probe {
                            // Probes must never copy a request's credentials.
                            assert!(!request.to_lowercase().contains("authorization:"));
                            health.fetch_add(1, Ordering::SeqCst);
                            tokio::time::sleep(health_delay).await;
                            (200, health_body)
                        } else {
                            lock(&seen).push(request);
                            if drop_response { return; }
                            (status, "payload")
                        };
                        let response = format!("HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                        let _ = stream.write_all(response.as_bytes()).await;
                    });
                }
                _ = tasks.join_next(), if !tasks.is_empty() => {}
            }
        }
    });
    Server {
        origin,
        requests,
        probes,
        task,
    }
}

const HEALTH: &str = r#"{"ok":true,"auth":"workos"}"#;

fn ready(a: &Server, b: &Server) -> Arc<Router> {
    let router = Router::new([&a.origin, &b.origin]);
    let mut state = lock(&router.state);
    state.initialized = true;
    state.checked = Some(Instant::now());
    drop(state);
    router
}

#[tokio::test]
async fn cold_start_uses_first_verified_entrance_without_waiting_for_blackhole() {
    let a = server(200, false, Duration::from_secs(20), HEALTH).await;
    let b = server(200, false, Duration::ZERO, HEALTH).await;
    let router = Router::new([&a.origin, &b.origin]);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), router.select())
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn captive_portal_is_not_a_healthy_entrance() {
    let a = server(200, false, Duration::ZERO, "<html>sign in</html>").await;
    let b = server(200, false, Duration::from_millis(20), HEALTH).await;
    let router = Router::new([&a.origin, &b.origin]);
    assert_eq!(router.select().await, 1);
}

#[tokio::test]
async fn concurrent_cold_requests_share_one_probe() {
    let a = server(200, false, Duration::from_millis(20), HEALTH).await;
    let b = server(200, false, Duration::from_millis(20), HEALTH).await;
    let router = Router::new([&a.origin, &b.origin]);
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..12 {
        let router = router.clone();
        tasks.spawn(async move { router.select().await });
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }
    assert_eq!(a.probes.load(Ordering::SeqCst), 1);
    assert_eq!(b.probes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn get_retries_once_preserving_query_authorization_and_range() {
    let a = server(503, false, Duration::ZERO, HEALTH).await;
    let b = server(206, false, Duration::ZERO, HEALTH).await;
    let router = ready(&a, &b);
    let client = reqwest::Client::new();
    let request = client
        .get(format!("{}/rows?since=42&device=a%2Fb", a.origin))
        .bearer_auth("test-bearer")
        .header("Range", "bytes=10-20")
        .build()
        .unwrap();
    let response = router.send(client, request).await.unwrap();
    assert_eq!(response.status(), 206);
    assert_eq!(response.text().await.unwrap(), "payload");
    assert_eq!(lock(&a.requests).len(), 1);
    let requests = lock(&b.requests);
    assert_eq!(requests.len(), 1);
    let request = requests[0].to_lowercase();
    assert!(request.starts_with("get /rows?since=42&device=a%2fb http/1.1"));
    assert!(request.contains("authorization: bearer test-bearer"));
    assert!(request.contains("range: bytes=10-20"));
    assert!(request.contains(&format!("host: {}", b.origin.trim_start_matches("http://"))));
}

#[tokio::test]
async fn ambiguous_post_is_never_replayed_on_the_other_entrance() {
    let a = server(200, true, Duration::ZERO, HEALTH).await;
    let b = server(200, false, Duration::ZERO, HEALTH).await;
    let router = ready(&a, &b);
    let client = reqwest::Client::new();
    let request = client
        .post(format!("{}/auth/refresh", a.origin))
        .json(&serde_json::json!({"refreshToken":"single-use-test"}))
        .build()
        .unwrap();
    assert!(router.send(client, request).await.is_err());
    assert_eq!(lock(&a.requests).len(), 1);
    assert!(lock(&b.requests).is_empty());
    assert_eq!(lock(&router.state).selected, 1);
}

#[tokio::test]
async fn rejected_credentials_and_rate_limits_do_not_rotate_routes() {
    for status in [400, 401, 403, 404, 429, 500] {
        let a = server(status, false, Duration::ZERO, HEALTH).await;
        let b = server(200, false, Duration::ZERO, HEALTH).await;
        let router = ready(&a, &b);
        let client = reqwest::Client::new();
        let request = client
            .get(format!("{}/auth/orgs", a.origin))
            .build()
            .unwrap();
        assert_eq!(router.send(client, request).await.unwrap().status(), status);
        assert_eq!(lock(&router.state).selected, 0);
        assert!(lock(&b.requests).is_empty());
    }
}

#[tokio::test]
async fn dual_failure_is_bounded_and_old_socket_failures_do_not_bounce_selection() {
    let a = server(503, false, Duration::ZERO, HEALTH).await;
    let b = server(502, false, Duration::ZERO, HEALTH).await;
    let router = ready(&a, &b);
    let client = reqwest::Client::new();
    let request = client.get(format!("{}/rows", a.origin)).build().unwrap();
    assert_eq!(router.send(client, request).await.unwrap().status(), 502);
    assert_eq!(lock(&a.requests).len(), 1);
    assert_eq!(lock(&b.requests).len(), 1);
    for _ in 0..20 {
        router.failed(0);
        router.failed(1);
    }
    assert_eq!(lock(&router.state).selected, 1);
}

#[tokio::test(start_paused = true)]
async fn performance_switch_requires_two_samples_and_minimum_hold() {
    let router = Router::new([DIRECT_ORIGIN, RELAY_ORIGIN]);
    let samples = [
        Some(Duration::from_millis(500)),
        Some(Duration::from_millis(100)),
    ];
    router.apply_samples(0, false, samples);
    assert_eq!(lock(&router.state).selected, 0);
    tokio::time::advance(MIN_HOLD).await;
    router.apply_samples(
        0,
        false,
        [
            Some(Duration::from_millis(150)),
            Some(Duration::from_millis(100)),
        ],
    );
    router.apply_samples(0, false, samples);
    assert_eq!(lock(&router.state).selected, 0);
    router.apply_samples(0, false, samples);
    assert_eq!(lock(&router.state).selected, 1);
    // Measurements started before a failure cannot change its decision.
    router.failed(1);
    router.apply_samples(0, false, samples);
    assert_eq!(lock(&router.state).selected, 0);
}

#[tokio::test]
async fn network_change_discards_old_measurements_and_failure_cooldowns() {
    let router = Router::new([DIRECT_ORIGIN, RELAY_ORIGIN]);
    router.failed(0);
    let old_generation = lock(&router.state).generation;
    router.invalidate();
    router.apply_samples(
        old_generation,
        true,
        [Some(Duration::from_millis(10)), None],
    );
    let state = lock(&router.state);
    assert_eq!(state.selected, 1);
    assert!(state.checked.is_none());
    assert_eq!(state.blocked, [None, None]);
}

#[tokio::test]
async fn cache_only_restores_allowlisted_origins() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("network-route.json");
    for invalid in [
        "broken json",
        r#"{"origin":"https://evil.example"}"#,
        r#"{"origin":"https://edge.puqing.work/sneaky"}"#,
    ] {
        std::fs::write(&path, invalid).unwrap();
        let router = Router::new([DIRECT_ORIGIN, RELAY_ORIGIN]);
        router.configure_cache(dir.path());
        assert!(!lock(&router.state).initialized);
    }
    std::fs::write(
        &path,
        serde_json::json!({"origin": RELAY_ORIGIN}).to_string(),
    )
    .unwrap();
    let router = Router::new([DIRECT_ORIGIN, RELAY_ORIGIN]);
    router.configure_cache(dir.path());
    assert_eq!(lock(&router.state).selected, 1);
    assert!(lock(&router.state).checked.is_none());
    router.failed(1);
    router.succeeded(0);
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains(DIRECT_ORIGIN)
    );
}

#[tokio::test]
async fn custom_origins_and_insecure_lookalikes_are_untouched() {
    let router = Router::new([DIRECT_ORIGIN, RELAY_ORIGIN]);
    for url in [
        "http://edge.550w.host/",
        "https://edge.550w.host:8443/",
        "https://edge.550w.host.evil.example/",
        "https://user@edge.550w.host/",
        "http://localhost:8787/",
    ] {
        assert_eq!(router.index(&Url::parse(url).unwrap()), None);
    }
    let url = Url::parse("wss://edge.550w.host/device/abc?token=test&cursor=12").unwrap();
    assert_eq!(
        router.rewrite(&url, 1).as_str(),
        "wss://edge.puqing.work/device/abc?token=test&cursor=12"
    );
    let local = server(200, false, Duration::ZERO, HEALTH).await;
    let response = reqwest::Client::new()
        .get(format!("{}/rows", local.origin))
        .send_routed()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(local.probes.load(Ordering::SeqCst), 0);
}
