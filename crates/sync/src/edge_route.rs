//! Automatic routing between two entrances to the same production backend.
//! Probes never carry credentials. Writes are sent once; only GET/HEAD may be
//! retried here. Protocol outboxes own replay and deduplication of mutations.

use std::{
    future::Future,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use futures::{StreamExt, stream::FuturesUnordered};
use reqwest::{Method, Request, Response, StatusCode, Url};
use tokio::{sync::Mutex as AsyncMutex, time::Instant};

pub const DIRECT_ORIGIN: &str = "https://edge.550w.host";
pub const RELAY_ORIGIN: &str = "https://edge.puqing.work";
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const RECHECK: Duration = Duration::from_secs(60);
const COOLDOWN: Duration = Duration::from_secs(30);
const MIN_HOLD: Duration = Duration::from_secs(120);

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct State {
    selected: usize,
    initialized: bool,
    runtime: Option<tokio::runtime::Handle>,
    checked: Option<Instant>,
    switched: Instant,
    blocked: [Option<Instant>; 2],
    challenger: Option<usize>,
    generation: u64,
    cache: Option<PathBuf>,
    persisted: Option<usize>,
}

pub(crate) struct Router {
    origins: [Url; 2],
    state: Mutex<State>,
    probe_gate: Arc<AsyncMutex<()>>,
    probe_client: reqwest::Client,
}

impl Router {
    pub(crate) fn new(origins: [&str; 2]) -> Arc<Self> {
        Arc::new(Self {
            origins: origins.map(|s| Url::parse(s).expect("edge origin")),
            state: Mutex::new(State {
                selected: 0,
                initialized: false,
                runtime: None,
                checked: None,
                switched: Instant::now(),
                blocked: [None, None],
                challenger: None,
                generation: 0,
                cache: None,
                persisted: None,
            }),
            probe_gate: Arc::new(AsyncMutex::new(())),
            probe_client: reqwest::Client::builder()
                .connect_timeout(PROBE_TIMEOUT)
                .timeout(PROBE_TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("edge probe client"),
        })
    }

    pub(crate) fn index(&self, url: &Url) -> Option<usize> {
        if !url.username().is_empty() || url.password().is_some() {
            return None;
        }
        self.origins.iter().position(|origin| {
            url.host_str() == origin.host_str()
                && url.port_or_known_default() == origin.port_or_known_default()
                && (url.scheme() == origin.scheme()
                    || (url.scheme() == "wss" && origin.scheme() == "https")
                    || (url.scheme() == "ws" && origin.scheme() == "http"))
        })
    }

    pub(crate) fn rewrite(&self, url: &Url, index: usize) -> Url {
        let mut result = url.clone();
        let origin = &self.origins[index];
        result.set_host(origin.host_str()).expect("edge host");
        result.set_port(origin.port()).expect("edge port");
        result
    }

    pub(crate) async fn select(self: &Arc<Self>) -> usize {
        let (initialized, stale, selected) = {
            let mut state = lock(&self.state);
            state.runtime = tokio::runtime::Handle::try_current().ok();
            (
                state.initialized,
                state.checked.is_none_or(|at| at.elapsed() >= RECHECK),
                state.selected,
            )
        };
        if !stale {
            return selected;
        }
        if !initialized {
            let _guard = self.probe_gate.lock().await;
            let needs_probe = !lock(&self.state).initialized;
            if needs_probe {
                self.probe(true).await;
            }
        } else if let Ok(guard) = self.probe_gate.clone().try_lock_owned() {
            let router = self.clone();
            tokio::spawn(async move {
                let _guard = guard;
                router.probe(false).await;
            });
        }
        lock(&self.state).selected
    }

    fn invalidate(&self) -> Option<tokio::runtime::Handle> {
        let mut state = lock(&self.state);
        state.checked = None;
        state.blocked = [None, None];
        state.challenger = None;
        state.generation += 1;
        state.runtime.clone()
    }

    async fn sample(&self, index: usize) -> Option<Duration> {
        let started = Instant::now();
        let mut response = self
            .probe_client
            .get(self.origins[index].join("health").ok()?)
            .send()
            .await
            .ok()?;
        if response.status() != StatusCode::OK {
            return None;
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.ok()? {
            if bytes.len() + chunk.len() > 1024 {
                return None;
            }
            bytes.extend_from_slice(&chunk);
        }
        let body: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        (body.get("ok")?.as_bool()? && body.get("auth")?.as_str()? == "workos")
            .then(|| started.elapsed())
    }

    async fn probe(&self, initial: bool) {
        let (generation, blocked) = {
            let mut state = lock(&self.state);
            state.checked = Some(Instant::now());
            (state.generation, state.blocked)
        };
        let mut pending = FuturesUnordered::new();
        for (index, until) in blocked.into_iter().enumerate() {
            if until.is_none_or(|at| at <= Instant::now()) {
                pending.push(async move { (index, self.sample(index).await) });
            }
        }
        let mut samples = [None, None];
        while let Some((index, result)) = pending.next().await {
            samples[index] = result;
            // Cold starts need only the first verified entrance. Cancel the
            // slower probe rather than wait for a blackholed route's timeout.
            if initial && result.is_some() {
                break;
            }
        }
        self.apply_samples(generation, initial, samples);
    }

    fn apply_samples(&self, generation: u64, initial: bool, samples: [Option<Duration>; 2]) {
        let selected = {
            let mut state = lock(&self.state);
            if state.generation != generation {
                return;
            }
            state.initialized = true;
            let current = state.selected;
            let other = 1 - current;
            let better = match (samples[current], samples[other]) {
                (None, Some(_)) => true,
                (Some(a), Some(b)) => {
                    a.saturating_sub(b) >= Duration::from_millis(100)
                        && b.as_secs_f64() <= a.as_secs_f64() * 0.7
                }
                _ => false,
            };
            let switch = better
                && (initial
                    || samples[current].is_none()
                    || (state.challenger == Some(other) && state.switched.elapsed() >= MIN_HOLD));
            state.challenger = better.then_some(other);
            if switch {
                state.selected = other;
                state.switched = Instant::now();
                state.challenger = None;
            }
            samples[state.selected].map(|_| state.selected)
        };
        if let Some(index) = selected {
            self.persist(index);
        }
    }

    pub(crate) fn failed(&self, index: usize) {
        let mut state = lock(&self.state);
        let now = Instant::now();
        state.blocked[index] = Some(now + COOLDOWN);
        state.generation += 1;
        state.checked = Some(now);
        state.challenger = None;
        if state.selected == index && state.blocked[1 - index].is_none_or(|at| at <= now) {
            state.selected = 1 - index;
            state.switched = now;
            tracing::info!(origin = %self.origins[state.selected], "edge route changed after transport failure");
        }
    }

    pub(crate) fn succeeded(&self, index: usize) {
        let selected = {
            let mut state = lock(&self.state);
            if state.selected == index {
                state.blocked[index] = None;
            }
            state.selected == index
        };
        if selected {
            self.persist(index);
        }
    }

    fn persist(&self, index: usize) {
        let mut state = lock(&self.state);
        if state.selected != index || state.persisted == Some(index) {
            return;
        }
        let Some(path) = state.cache.clone() else {
            return;
        };
        let origin = self.origins[index]
            .as_str()
            .trim_end_matches('/')
            .to_owned();
        // This is a performance hint, never credentials or correctness state.
        // A truncated hint is ignored and a cold start probes both entrances.
        // Tiny writes are serialized with selection, so an older success
        // cannot overwrite a newer hint. Failure simply disables the hint.
        let bytes = serde_json::to_vec(&serde_json::json!({"origin":origin})).expect("route hint");
        if std::fs::write(path, bytes).is_ok() {
            state.persisted = Some(index);
        }
    }

    async fn send(
        self: &Arc<Self>,
        client: reqwest::Client,
        mut request: Request,
    ) -> Result<Response, reqwest::Error> {
        let original = request.url().clone();
        let index = self.select().await;
        *request.url_mut() = self.rewrite(&original, index);
        request.headers_mut().remove(reqwest::header::HOST);
        let retry = matches!(*request.method(), Method::GET | Method::HEAD)
            .then(|| request.try_clone())
            .flatten();
        let result = client.execute(request).await;
        if failed_response(&result) {
            self.failed(index);
            if let Some(mut retry) = retry {
                let next = lock(&self.state).selected;
                if next != index {
                    *retry.url_mut() = self.rewrite(&original, next);
                    let result = client.execute(retry).await;
                    if failed_response(&result) {
                        self.failed(next);
                    } else {
                        self.succeeded(next);
                    }
                    return result;
                }
            }
        } else {
            self.succeeded(index);
        }
        result
    }
}

fn failed_response(result: &Result<Response, reqwest::Error>) -> bool {
    match result {
        Err(error) => error.is_connect() || error.is_timeout() || error.is_request(),
        Ok(response) => matches!(response.status().as_u16(), 408 | 502 | 503 | 504),
    }
}

pub(crate) fn shared() -> &'static Arc<Router> {
    static ROUTER: OnceLock<Arc<Router>> = OnceLock::new();
    ROUTER.get_or_init(|| Router::new([DIRECT_ORIGIN, RELAY_ORIGIN]))
}

/// Restore a last-known route hint without changing custom/development edges.
pub fn configure_cache(edge: &str, data_dir: &Path) {
    let Ok(url) = Url::parse(edge) else {
        return;
    };
    if shared().index(&url).is_none() {
        return;
    }
    shared().configure_cache(data_dir);
}

impl Router {
    pub(crate) fn configure_cache(&self, data_dir: &Path) {
        let path = data_dir.join("network-route.json");
        let hint = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|value| value.get("origin")?.as_str().map(str::to_owned))
            .and_then(|value| Url::parse(&value).ok())
            .and_then(|url| self.origins.iter().position(|origin| &url == origin));
        let mut state = lock(&self.state);
        if state.cache.is_none() {
            state.cache = Some(path);
            if !state.initialized
                && let Some(index) = hint
            {
                state.selected = index;
                state.initialized = true;
                state.persisted = Some(index);
            }
        }
    }
}

/// A network change invalidates measurements, not healthy live connections.
pub fn network_changed() {
    let runtime = shared().invalidate();
    if !crate::wake::path_is_offline()
        && let Some(runtime) = runtime
    {
        runtime.spawn(async {
            shared().select().await;
        });
    }
}

/// The current hint, useful to native downloaders. It does not perform I/O.
pub fn current_origin(edge: &str) -> String {
    if Url::parse(edge)
        .ok()
        .is_some_and(|url| shared().index(&url).is_some())
    {
        shared().origins[lock(&shared().state).selected]
            .as_str()
            .trim_end_matches('/')
            .to_owned()
    } else {
        edge.to_owned()
    }
}

/// Resolve an entrance for native HTTP downloaders using the same probes.
pub async fn resolve_origin(edge: &str) -> String {
    if Url::parse(edge)
        .ok()
        .is_some_and(|url| shared().index(&url).is_some())
    {
        shared().select().await;
    }
    current_origin(edge)
}

/// Native downloaders report transport failures, never HTTP auth errors or
/// local validation failures. The URL is checked against the origin allowlist.
pub fn report_transport_failure(url: &str) {
    if let Ok(url) = Url::parse(url)
        && let Some(index) = shared().index(&url)
    {
        shared().failed(index);
    }
}

/// Opt production HTTP requests into automatic routing. No authentication
/// requests or mutations are duplicated, even after an ambiguous timeout.
pub trait RequestExt {
    fn send_routed(self) -> impl Future<Output = Result<Response, reqwest::Error>> + Send;
}

impl RequestExt for reqwest::RequestBuilder {
    async fn send_routed(self) -> Result<Response, reqwest::Error> {
        let (client, request) = self.build_split();
        let request = request?;
        if shared().index(request.url()).is_some() {
            shared().send(client, request).await
        } else {
            client.execute(request).await
        }
    }
}

#[cfg(test)]
mod tests;
