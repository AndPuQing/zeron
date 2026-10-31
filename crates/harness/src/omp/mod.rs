//! Native Oh My Pi (OMP) JSONL RPC driver. See PROTOCOL.md for the handshake
//! and `prompt_result` terminal contract. OMP shares much of Pi's wire shape but
//! is a distinct product with its own store (`~/.omp`), session format and
//! framing, so this driver never reuses Pi's discovery or session index.
mod catalog;
mod normalize;
mod rpc;
mod saved;
mod sessions;
mod ui;

use crate::{
    Harness, HarnessError, RunControls,
    process::{Command, Stdio, owned::Child},
};
use async_trait::async_trait;
use futures::{StreamExt, stream::BoxStream};
use normalize::{Normalizer, string};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::mpsc,
};
use zeron_proto::{
    AgentEvent, DoneStatus, HarnessId, Model, ReasoningLevel, RunRequest, SteeringMode,
};

pub struct OmpHarness {
    environment: std::sync::Arc<crate::environment::EnvironmentSnapshot>,
    models_cache: crate::catalog::Catalog,
    workspace_commands: crate::skills::CommandDiscovery,
    session_store: Option<PathBuf>,
    agent_dir: Option<PathBuf>,
    executable: Option<PathBuf>,
    interrupt_grace: Duration,
    kill_grace: Duration,
}
impl Default for OmpHarness {
    fn default() -> Self {
        Self {
            environment: Default::default(),
            executable: None,
            session_store: None,
            agent_dir: None,
            models_cache: Default::default(),
            workspace_commands: Default::default(),
            interrupt_grace: Duration::from_secs(2),
            kill_grace: Duration::from_secs(3),
        }
    }
}
impl OmpHarness {
    pub fn new() -> Self {
        Self::default()
    }
    /// Bind a fresh harness instance to one immutable provider configuration.
    pub fn with_environment(
        mut self,
        environment: std::sync::Arc<crate::environment::EnvironmentSnapshot>,
    ) -> Self {
        self.environment = environment;
        self
    }
    pub fn with_executable(mut self, path: impl Into<PathBuf>) -> Self {
        self.executable = Some(path.into());
        self
    }
    pub fn with_session_store(mut self, path: impl Into<PathBuf>) -> Self {
        self.session_store = Some(path.into());
        self
    }
    /// OMP's settings/credentials directory (`PI_CODING_AGENT_DIR`) for the
    /// child and for Zeron's own reads of it; defaults to the inherited one.
    pub fn with_agent_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.agent_dir = Some(path.into());
        self
    }
    pub fn with_graces(mut self, interrupt: Duration, kill: Duration) -> Self {
        self.interrupt_grace = interrupt;
        self.kill_grace = kill;
        self
    }
    pub fn resolve_executable(&self) -> Result<PathBuf, HarnessError> {
        if let Some(path) = self
            .executable
            .clone()
            .or_else(|| std::env::var_os("OMP_EXECUTABLE").map(PathBuf::from))
        {
            return crate::executable::validate_native_override(&path);
        }
        let home = crate::executable::home_or_current_dir();
        crate::executable::find_on_paths(
            "omp",
            vec![
                home.join(".local/bin/omp"),
                home.join(".local/share/pnpm/bin/omp"),
                home.join(".npm-global/bin/omp"),
                home.join(".bun/bin/omp"),
                PathBuf::from("/opt/homebrew/bin/omp"),
                PathBuf::from("/usr/local/bin/omp"),
            ],
        )
        .ok_or_else(|| {
            HarnessError::NotInstalled(
                "OMP CLI: install Oh My Pi or set OMP_EXECUTABLE (the omp acp subcommand is not used)"
                    .into(),
            )
        })
    }
    async fn probe(&self, cwd: &Path, models: bool) -> Result<Value, HarnessError> {
        let mut process = self.spawn(cwd, &["--no-session".into()], None, &Default::default())?;
        let (mut tx, rx) = tokio::sync::oneshot::channel();
        let grace = self.kill_grace;
        tokio::spawn(async move {
            let result = tokio::select! {
                result=tokio::time::timeout(Duration::from_secs(60),async {
                    let mut backlog = vec![];
                    process.handshake(&mut backlog).await?;
                    if models {Ok(serde_json::to_value(catalog::models(&mut process).await?).unwrap())}
                    else {process.query(json!({"type":"get_available_commands"}),&mut backlog).await}
                })=>result.unwrap_or_else(|_|Err(HarnessError::Protocol("OMP discovery timed out".into()))),
                _=tx.closed()=>Err(HarnessError::Protocol("OMP discovery cancelled".into())),
            };
            process.shutdown(grace).await;
            let _ = tx.send(result);
        });
        rx.await
            .map_err(|_| HarnessError::Protocol("OMP discovery task failed".into()))?
    }
    fn spawn(
        &self,
        cwd: &Path,
        args: &[String],
        mcp: Option<&zeron_proto::McpServer>,
        run_env: &std::collections::BTreeMap<String, String>,
    ) -> Result<Process, HarnessError> {
        let exe = self.resolve_executable()?;
        if self.executable.is_none() {
            if let Some(version) = crate::executable::binary_version(&exe) {
                // OMP's protocol revision (ready/negotiate/prompt_result) is
                // what the driver actually depends on; this floor only rejects
                // installs old enough to predate the Pi fork's RPC line.
                if version < semver::Version::new(18, 0, 0) {
                    return Err(HarnessError::Protocol(format!(
                        "OMP {version} is unsupported; update OMP to 18.0.0 or newer"
                    )));
                }
            }
        }
        let mut cmd = Command::new(&exe);
        // Process group plus the same env scrubbing the ACP launch applied.
        crate::process::owned::configure(&mut cmd);
        crate::compose_child_path(&mut cmd, &exe);
        self.environment.apply(&mut cmd);
        if let Some(dir) = &self.agent_dir {
            cmd.env("PI_CODING_AGENT_DIR", dir);
        }
        // Host-resolved run environment (a Cloud device's credential or
        // account home) applies last, as on the other drivers. The harness's
        // own identity (`PI_CODING_AGENT_DIR`) is set above and kept.
        crate::apply_run_env(&mut cmd, run_env);
        cmd.args(["--mode", "rpc"])
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let scratch = mcp
            .map(|config| {
                crate::extension_mcp::configure(&crate::extension_mcp::OMP, &mut cmd, config)
            })
            .transpose()?;
        let mut child = Child::new(cmd.spawn()?);
        let tail = crate::StderrTail::default();
        let mut lines = BufReader::new(child.stderr.take().expect("piped stderr")).lines();
        let stderr = tail.clone();
        let stderr_task = tokio::spawn(async move {
            while let Ok(Some(line)) = lines.next_line().await {
                stderr.push(&line);
            }
            stderr.close();
        });
        let transport = rpc::Transport::new(
            child.stdin.take().expect("piped stdin"),
            child.stdout.take().expect("piped stdout"),
        );
        Ok(Process {
            child,
            _scratch: scratch,
            transport,
            tail,
            stderr_task,
            dialogs: Default::default(),
            exit_deadline: None,
        })
    }
}
struct Process {
    child: Child,
    _scratch: Option<crate::scratch::ScratchDir>,
    transport: rpc::Transport,
    tail: crate::StderrTail,
    stderr_task: tokio::task::JoinHandle<()>,
    dialogs: ui::Dialogs,
    exit_deadline: Option<tokio::time::Instant>,
}
impl Drop for Process {
    fn drop(&mut self) {
        self.stderr_task.abort();
    }
}
impl Process {
    async fn next(&mut self) -> Result<Value, HarnessError> {
        loop {
            tokio::select! {
                // Drain buffered output before reporting process death. A descendant
                // may retain stdout, so EOF alone cannot govern liveness.
                biased;
                _ = async {
                    match self.exit_deadline {
                        Some(deadline) => tokio::time::sleep_until(deadline).await,
                        None => std::future::pending().await,
                    }
                } => return Err(HarnessError::Protocol("OMP exited".into())),
                status = self.child.wait(), if self.exit_deadline.is_none() => {
                    status?;
                    self.exit_deadline = Some(tokio::time::Instant::now() + Duration::from_millis(100));
                }
                frame = self.transport.incoming.recv() => return frame.unwrap_or_else(|| Err(HarnessError::Protocol("OMP disconnected".into()))),
            }
        }
    }
    /// Consume the `ready` frame, then opt into protocol v2 so oversized logical
    /// frames arrive as `rpc_chunk` sequences the transport reassembles.
    async fn handshake(&mut self, backlog: &mut Vec<Value>) -> Result<Value, HarnessError> {
        let ready = loop {
            let frame = self.next().await?;
            if frame["type"] == "ready" {
                break frame;
            }
            backlog.push(frame);
        };
        if !ready["supportedProtocolVersions"]
            .as_array()
            .is_some_and(|versions| versions.iter().any(|v| v == 1))
        {
            return Err(HarnessError::Protocol(
                "OMP did not advertise a supported RPC protocol version".into(),
            ));
        }
        let id = self
            .transport
            .client
            .request(json!({"type":"negotiate_protocol","protocolVersion":2}))?;
        loop {
            let frame = self.next().await?;
            if frame["type"] == "response" && frame["id"] == id {
                // v1 remains a working fallback; v2 only changes framing.
                let _ = response_data(frame);
                break;
            }
            if frame["type"] == "extension_ui_request" {
                self.dialogs.request(self.transport.client.clone(), &frame);
            } else {
                backlog.push(frame);
            }
        }
        Ok(ready)
    }
    async fn query(
        &mut self,
        command: Value,
        backlog: &mut Vec<Value>,
    ) -> Result<Value, HarnessError> {
        let id = self.transport.client.request(command)?;
        loop {
            let frame = self.next().await?;
            if frame["type"] == "response" && frame["id"] == id {
                return response_data(frame);
            }
            if frame["type"] == "extension_ui_request" {
                self.dialogs.request(self.transport.client.clone(), &frame);
            } else {
                backlog.push(frame);
            }
        }
    }
    async fn shutdown(&mut self, grace: Duration) {
        self.dialogs.cancel();
        self.child.shutdown(grace).await;
    }
}
/// Whether `request` picks a thinking level (an effort, or thinking off).
fn thinking_selected(request: &RunRequest) -> bool {
    request.reasoning.is_some()
        || request
            .model_options
            .get("omp_thinking")
            .is_some_and(|v| v == "off")
}
fn response_data(frame: Value) -> Result<Value, HarnessError> {
    if frame["success"] == true {
        Ok(frame["data"].clone())
    } else {
        Err(HarnessError::Protocol(format!(
            "OMP {}: {}",
            string(&frame, "command"),
            frame["error"].as_str().unwrap_or("command failed")
        )))
    }
}
#[async_trait]
impl Harness for OmpHarness {
    fn saved_session_store_id(&self) -> Result<String, HarnessError> {
        self.saved_store_id()
    }
    async fn saved_sessions(
        &self,
        cursor: Option<&str>,
    ) -> Result<crate::saved_sessions::SavedSessionPage, HarnessError> {
        self.list_saved(cursor).await
    }
    async fn saved_session_history(
        &self,
        session: &crate::saved_sessions::SavedSession,
    ) -> Result<crate::saved_sessions::SavedSessionHistory, HarnessError> {
        self.read_saved(session).await
    }
    async fn copy_saved_session(
        &self,
        session: &crate::saved_sessions::SavedSession,
    ) -> Result<crate::saved_sessions::SavedSession, HarnessError> {
        self.copy_saved(session).await
    }

    fn environment(&self) -> std::sync::Arc<crate::environment::EnvironmentSnapshot> {
        self.environment.clone()
    }

    fn id(&self) -> HarnessId {
        HarnessId::Omp
    }
    fn display_name(&self) -> &str {
        "Oh My Pi"
    }
    fn supports_steering(&self) -> bool {
        true
    }
    fn steering_mode(&self) -> SteeringMode {
        SteeringMode::StepBoundary
    }
    fn reasoning_levels(&self) -> &[ReasoningLevel] {
        &[]
    }
    fn installed(&self) -> bool {
        self.resolve_executable().is_ok()
    }
    fn executable_path(&self) -> Option<PathBuf> {
        self.resolve_executable().ok()
    }
    fn deterministic_turn_end(&self) -> bool {
        true
    }
    /// `set_model` and `set_thinking_level` change the live process. Clearing
    /// either back to the default cannot be expressed that way.
    fn reconfigures_in_place(&self, live: &RunRequest, next: &RunRequest) -> bool {
        let model = |r: &RunRequest| r.model.as_deref().filter(|m| *m != "default").is_some();
        live.cwd == next.cwd
            && (model(next) || !model(live))
            && (thinking_selected(next) || !thinking_selected(live))
    }
    async fn models(&self) -> Result<Vec<Model>, HarnessError> {
        Ok(self.model_catalog(false).await?.models)
    }
    fn model_context(&self) -> Result<Option<crate::ModelContext>, HarnessError> {
        let root = sessions::agent_dir(self.agent_dir.clone());
        crate::model_context::context(
            HarnessId::Omp,
            &self.resolve_executable()?,
            &[
                root.join("models.yml"),
                root.join("config.yml"),
                root.join("auth.json"),
            ],
        )
        .map(|context| Some(self.environment.partition_context(context)))
    }
    async fn model_catalog(&self, force: bool) -> Result<crate::ModelCatalog, HarnessError> {
        self.models_cache
            .get_with_timeout(
                force,
                Duration::from_secs(65),
                || Ok(self.model_context()?.expect("OMP model context").key()),
                || async {
                    let value = self.probe(&std::env::current_dir()?, true).await?;
                    serde_json::from_value(value).map_err(|e| HarnessError::Protocol(e.to_string()))
                },
            )
            .await
    }
    async fn commands(&self) -> Result<Vec<zeron_proto::SlashCommand>, HarnessError> {
        self.commands_for(&std::env::current_dir()?).await
    }
    async fn commands_for(
        &self,
        cwd: &Path,
    ) -> Result<Vec<zeron_proto::SlashCommand>, HarnessError> {
        self.workspace_commands
            .get(cwd, async {
                Ok(catalog::commands(&self.probe(cwd, false).await?))
            })
            .await
    }
    async fn skills(
        &self,
        cwd: &Path,
    ) -> Result<Option<Vec<zeron_proto::invocation::Skill>>, HarnessError> {
        let mut skills = crate::skills::discover(HarnessId::Omp, cwd).await?;
        let commands = self.commands_for(cwd).await?;
        crate::skills::attach_advertised_commands(HarnessId::Omp, &mut skills, &commands);
        Ok(Some(skills))
    }
    fn fallback_models(&self) -> Vec<Model> {
        vec![Model {
            id: "default".into(),
            label: "OMP default".into(),
            description: Some("Model configured in Oh My Pi".into()),
            reasoning_levels: vec![],
            options: vec![],
        }]
    }
    async fn run(
        &self,
        request: RunRequest,
        controls: RunControls,
    ) -> Result<BoxStream<'static, Result<AgentEvent, HarnessError>>, HarnessError> {
        let mut request = request;
        let store = sessions::Store::new(self.session_store.clone(), self.agent_dir.clone());
        let mut lost_context = None;
        let args = match &request.resume {
            Some(id) => match store.resume_args(id, Path::new(&request.cwd)) {
                Ok(args) => args,
                // An imported copy must never fall back to a fresh provider
                // session: the doc would then claim history it does not have.
                // Fail before spawning anything so no new OMP session exists.
                Err(error) if request.require_native_resume => {
                    return Err(HarnessError::Protocol(format!(
                        "This chat was imported from a native OMP session and must resume it; session {id} could not be restored ({error}). No new session was started."
                    )));
                }
                // An ordinary chat that lost its history starts fresh with a
                // visible notice, like the other harnesses.
                Err(error) => {
                    lost_context = Some(format!(
                        "OMP could not restore session {id}; starting a new session without the previous context: {error}"
                    ));
                    vec![]
                }
            },
            None if request.require_native_resume => {
                return Err(HarnessError::Protocol(
                    "This chat was imported from a native OMP session but carries no session id to resume. No new session was started.".into(),
                ));
            }
            None => vec![],
        };
        if lost_context.is_some() {
            request.resume = None;
        }
        let process = self.spawn(
            Path::new(&request.cwd),
            &args,
            request.mcp.as_ref(),
            &request.env,
        )?;
        let (tx, rx) = mpsc::channel(256);
        let kill_grace = self.kill_grace;
        let interrupt_grace = self.interrupt_grace;
        tokio::spawn(async move {
            let mut runner = Runner {
                store,
                interrupted: false,
                initial_pending: true,
                deliveries: VecDeque::new(),
                queued: VecDeque::new(),
                process,
                tx,
                request,
                norm: Normalizer::default(),
                active: true,
                epoch: 0,
                pending: HashMap::new(),
                session: String::new(),
                assistant: uuid::Uuid::new_v4().to_string(),
                lost_context,
            };
            // The lease lives through shutdown even if the consumer drops its stream.
            let RunControls {
                realtime: _,
                execution_lease: _lease,
                request_input,
                mut steering,
                interrupt,
                turn: _,
            } = controls;
            runner.process.dialogs.input = Some(std::sync::Arc::from(request_input));
            let consumer = runner.tx.clone();
            let result = tokio::select! {
                result=runner.run(&mut steering)=>result,
                _=interrupt.cancelled()=>{ runner.interrupt(interrupt_grace).await; Ok(()) },
                _=consumer.closed()=>Ok(()),
            };
            if let Err(error) = result {
                let status = runner.process.child.try_wait().ok().flatten();
                runner.norm.error = Some(if status.is_some() {
                    crate::crash_message("OMP", status, &runner.process.tail)
                } else {
                    error.to_string()
                });
            }
            runner.process.shutdown(kill_grace).await;
            let _ = runner.finish().await;
        });
        Ok(
            futures::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|v| (v, rx)) })
                .boxed(),
        )
    }
}
enum Pending {
    Prompt,
}
struct Delivery;
struct Runner {
    interrupted: bool,
    initial_pending: bool,
    deliveries: VecDeque<Delivery>,
    queued: VecDeque<crate::SteerMessage>,
    store: sessions::Store,
    process: Process,
    tx: mpsc::Sender<Result<AgentEvent, HarnessError>>,
    request: RunRequest,
    norm: Normalizer,
    active: bool,
    epoch: u64,
    pending: HashMap<String, Pending>,
    session: String,
    assistant: String,
    lost_context: Option<String>,
}
impl Runner {
    async fn emit(&self, event: AgentEvent) -> Result<(), HarnessError> {
        self.tx
            .send(Ok(event))
            .await
            .map_err(|_| HarnessError::Protocol("OMP event consumer closed".into()))
    }
    async fn started(&self, model: String) -> Result<(), HarnessError> {
        self.emit(AgentEvent::SessionStarted {
            harness: HarnessId::Omp,
            model,
            tools: vec![],
            cwd: self.request.cwd.clone(),
            session_id: self.session.clone(),
            assistant_message_id: self.assistant.clone(),
        })
        .await
    }
    async fn finish(&mut self) -> Result<(), HarnessError> {
        if !self.active {
            return Ok(());
        }
        self.active = false;
        self.process.dialogs.cancel();
        self.emit(AgentEvent::Done {
            status: if self.interrupted {
                DoneStatus::Interrupted
            } else {
                self.norm.status()
            },
            result: None,
            error: self.norm.error.clone(),
            session_id: (!self.session.is_empty()).then(|| self.session.clone()),
        })
        .await
    }
    fn prompt(&mut self, text: String, images: Value) -> Result<(), HarnessError> {
        self.epoch += 1;
        self.active = true;
        self.norm.reset();
        let id = self
            .process
            .transport
            .client
            .request(json!({"type":"prompt","message":text,"images":images}))?;
        self.pending.insert(id, Pending::Prompt);
        Ok(())
    }
    /// OMP's dedicated `steer` queues at the next model step and acknowledges
    /// immediately; consumption is the following user `message_start`.
    fn steer(&mut self, text: String, images: Value) -> Result<(), HarnessError> {
        self.process
            .transport
            .client
            .send(json!({"type":"steer","message":text,"images":images}))
    }
    async fn bootstrap(&mut self, backlog: &mut Vec<Value>) -> Result<Value, HarnessError> {
        self.process.handshake(backlog).await?;
        // Zeron's pending steers belong together at the next model step. OMP's
        // `set_steering_mode` is session-scoped (`persist: false`), so this can
        // never overwrite a machine-global user preference.
        self.process
            .query(json!({"type":"set_steering_mode","mode":"all"}), backlog)
            .await?;
        let request = self.request.clone();
        self.configure(&request, backlog).await?;
        let commands = self
            .process
            .query(json!({"type":"get_available_commands"}), backlog)
            .await?;
        self.emit(AgentEvent::AvailableCommands {
            commands: catalog::commands(&commands),
        })
        .await?;
        self.process
            .query(json!({"type":"get_state"}), backlog)
            .await
    }
    /// Select `request`'s model and thinking level on the live process.
    async fn configure(
        &mut self,
        request: &RunRequest,
        backlog: &mut Vec<Value>,
    ) -> Result<(), HarnessError> {
        if let Some(model) = request.model.as_deref().filter(|s| *s != "default") {
            let models = self
                .process
                .query(json!({"type":"get_available_models"}), backlog)
                .await?;
            let options = models["models"]
                .as_array()
                .ok_or_else(|| HarnessError::Protocol("OMP returned no models".into()))?;
            let found = options
                .iter()
                .find(|m| format!("{}/{}", string(m, "provider"), string(m, "id")) == model)
                .or_else(|| {
                    let mut matches = options.iter().filter(|m| string(m, "id") == model);
                    let first = matches.next()?;
                    matches.next().is_none().then_some(first)
                })
                .ok_or_else(|| {
                    HarnessError::Protocol(format!("OMP model is not available: {model}"))
                })?;
            self.process
                .query(
                    json!({"type":"set_model","provider":found["provider"],"modelId":found["id"]}),
                    backlog,
                )
                .await?;
        }
        if thinking_selected(request) {
            let supported = self
                .process
                .query(json!({"type":"get_available_thinking_levels"}), backlog)
                .await?;
            let levels = supported["levels"].as_array().cloned().unwrap_or_default();
            let desired = if request
                .model_options
                .get("omp_thinking")
                .is_some_and(|v| v == "off")
            {
                "off".to_string()
            } else {
                serde_json::to_value(request.reasoning)
                    .unwrap()
                    .as_str()
                    .unwrap_or("medium")
                    .to_owned()
            };
            let order = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];
            let rank = order
                .iter()
                .position(|s| *s == desired)
                .unwrap_or(order.len() - 1);
            let effective = order[..=rank]
                .iter()
                .rev()
                .find(|s| levels.iter().any(|v| v == **s))
                .ok_or_else(|| {
                    HarnessError::Protocol("OMP returned no supported thinking level".into())
                })?;
            self.process
                .query(
                    json!({"type":"set_thinking_level","level":effective}),
                    backlog,
                )
                .await?;
        }
        Ok(())
    }
    async fn run(
        &mut self,
        steering: &mut mpsc::Receiver<crate::SteerMessage>,
    ) -> Result<(), HarnessError> {
        let mut backlog = vec![];
        // Extensions and model discovery can start cold; startup gets the same
        // budget the ACP handshake allows.
        let state = tokio::time::timeout(Duration::from_secs(120), self.bootstrap(&mut backlog))
            .await
            .map_err(|_| HarnessError::Protocol("OMP startup timed out".into()))??;
        self.session = string(&state, "sessionId").into();
        if self.session.is_empty() {
            return Err(HarnessError::Protocol(
                "OMP get_state omitted sessionId".into(),
            ));
        }
        if self
            .request
            .resume
            .as_ref()
            .is_some_and(|id| id != &self.session)
        {
            return Err(HarnessError::Protocol(
                "OMP resumed a different session".into(),
            ));
        }
        if let Some(file) = state["sessionFile"].as_str() {
            self.store.remember(&self.session, Path::new(file))?;
        }
        self.norm.window = state["model"]["contextWindow"].as_u64();
        self.started(state["model"]["id"].as_str().unwrap_or("default").into())
            .await?;
        if let Some(message) = self.lost_context.take() {
            self.emit(AgentEvent::Error { message }).await?;
        }
        for frame in backlog {
            self.frame(frame).await?;
        }
        let images = load_images(&self.request.attachments).await;
        self.prompt(self.request.prompt.clone(), images)?;
        let mut open = true;
        loop {
            while self.active {
                let Some(steer) = self.queued.pop_front() else {
                    break;
                };
                // A changed model or thinking level applies on the live
                // process before the message it was sent with.
                if let Some(next) = &steer.config {
                    let mut backlog = vec![];
                    self.configure(next, &mut backlog).await?;
                    self.request.model = next.model.clone();
                    self.request.reasoning = next.reasoning;
                    self.request.model_options = next.model_options.clone();
                    for frame in backlog {
                        self.frame(frame).await?;
                    }
                }
                let images = load_images(&steer.attachments).await;
                self.steer(steer.prompt.clone(), images)?;
                self.deliveries.push_back(Delivery);
            }
            if !self.active {
                self.queued.clear();
                return Ok(());
            }
            tokio::select! {
                incoming=self.process.next()=>self.frame(incoming?).await?,
                steer=steering.recv(),if open=>match steer {Some(steer)=>self.queued.push_back(steer),None=>open=false}
            }
        }
    }
    async fn confirm_delivery(&mut self) -> Result<(), HarnessError> {
        if self.deliveries.pop_front().is_some() {
            let old = std::mem::replace(&mut self.assistant, uuid::Uuid::new_v4().to_string());
            self.emit(AgentEvent::Steered {
                assistant_message_id: Some(old),
                next_assistant_message_id: Some(self.assistant.clone()),
            })
            .await?;
        }
        Ok(())
    }
    async fn interrupt(&mut self, grace: Duration) {
        self.interrupted = true;
        self.process.dialogs.cancel();
        self.queued.clear();
        self.deliveries.clear();
        if !self.active {
            return;
        }
        let Ok(abort) = self
            .process
            .transport
            .client
            .request(json!({"type":"abort"}))
        else {
            return;
        };
        let _ = tokio::time::timeout(grace, async {
            while let Ok(frame) = self.process.next().await {
                let done = frame["id"] == abort
                    || frame["type"] == "prompt_result"
                    || frame["type"] == "session_settled";
                if self.frame(frame).await.is_err() || done {
                    break;
                }
            }
        })
        .await;
    }
    /// Apply a `prompt_result` — OMP's authoritative per-prompt terminal.
    async fn prompt_result(&mut self, frame: &Value) -> Result<(), HarnessError> {
        self.norm.error = None;
        self.norm.aborted = false;
        match string(frame, "status") {
            "aborted" => self.norm.aborted = true,
            "error" => {
                self.norm.error = Some(
                    frame["error"]["message"]
                        .as_str()
                        .filter(|text| !text.is_empty())
                        .unwrap_or("OMP provider request failed")
                        .to_owned(),
                );
            }
            _ => {}
        }
        self.active = true;
        self.finish().await
    }
    async fn frame(&mut self, frame: Value) -> Result<(), HarnessError> {
        match string(&frame, "type") {
            "response" => {
                // A successful `prompt` response only acknowledges admission;
                // the matching `prompt_result` is still owed. A rejected
                // command never gets one, so it terminates here.
                if frame["success"] != true {
                    let id = string(&frame, "id").to_owned();
                    if let Some(Pending::Prompt) = self.pending.remove(&id) {
                        self.norm.error = Some(format!(
                            "OMP {}: {}",
                            string(&frame, "command"),
                            frame["error"].as_str().unwrap_or("command failed")
                        ));
                        self.finish().await?;
                    }
                }
                return Ok(());
            }
            "available_commands_update" => {
                self.emit(AgentEvent::AvailableCommands {
                    commands: catalog::commands(&frame),
                })
                .await?;
                return Ok(());
            }
            "extension_ui_request" => {
                if !self.active {
                    // A background notification must not reopen a completed
                    // engine turn; cancel it explicitly.
                    ui::Dialogs::default().request(self.process.transport.client.clone(), &frame);
                    tracing::debug!(method = %string(&frame, "method"), "OMP UI request outside a run");
                    return Ok(());
                }
                let notify = string(&frame, "method") == "notify";
                let message = string(&frame, "message").to_owned();
                self.process
                    .dialogs
                    .request(self.process.transport.client.clone(), &frame);
                if notify {
                    self.emit(AgentEvent::TextDelta {
                        text: format!("\n{message}\n"),
                    })
                    .await?;
                }
                return Ok(());
            }
            "prompt_result" => {
                let id = string(&frame, "id").to_owned();
                if self.pending.remove(&id).is_some() {
                    self.prompt_result(&frame).await?;
                }
                return Ok(());
            }
            "session_settled" => return Ok(()),
            _ => {}
        }
        if frame["type"] == "agent_start" && !self.active {
            self.epoch += 1;
            self.active = true;
            self.norm.reset();
            self.assistant = uuid::Uuid::new_v4().to_string();
            self.started(
                self.request
                    .model
                    .clone()
                    .unwrap_or_else(|| "default".into()),
            )
            .await?;
        }
        if frame["type"] == "message_start" && frame["message"]["role"] == "user" {
            if self.initial_pending {
                self.initial_pending = false;
            } else {
                self.confirm_delivery().await?;
            }
        }
        for event in self.norm.map(&frame) {
            self.emit(event).await?;
        }
        Ok(())
    }
}
/// Inline attachments as native image blocks, best-effort like the Claude
/// driver: the path refs already ride the prompt text, so an unreadable,
/// oversized or non-inlinable file must not fail the turn.
async fn load_images(paths: &[String]) -> Value {
    use base64::Engine;
    let mut images = vec![];
    for path in paths {
        match tokio::fs::metadata(path).await {
            Ok(meta) if meta.len() <= 20 * 1024 * 1024 => {}
            Ok(_) => {
                tracing::debug!(%path, "OMP attachment over inline cap; path ref only");
                continue;
            }
            Err(error) => {
                tracing::warn!(%path, %error, "OMP attachment unreadable; path ref only");
                continue;
            }
        }
        let bytes = match tokio::fs::read(path).await {
            Ok(bytes) => bytes,
            Err(error) => {
                tracing::warn!(%path, %error, "OMP attachment unreadable; path ref only");
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
            tracing::debug!(%path, "OMP attachment is not an inlinable image; path ref only");
            continue;
        };
        images.push(json!({"type":"image","data":base64::engine::general_purpose::STANDARD.encode(bytes),"mimeType":mime}));
    }
    Value::Array(images)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(resume: Option<&str>, require_native_resume: bool) -> RunRequest {
        RunRequest {
            access: Default::default(),
            plan: false,
            prompt: "p".into(),
            harness: None,
            model: None,
            reasoning: None,
            model_options: Default::default(),
            cwd: "/tmp".into(),
            sandbox: zeron_proto::SandboxLevel::WorkspaceWrite,
            auto_approve: false,
            resume: resume.map(str::to_owned),
            require_native_resume,
            attachments: vec![],
            worktree: None,
            mcp: None,
            env: Default::default(),
        }
    }

    fn controls() -> RunControls {
        let (_steering, steering) = mpsc::channel(1);
        RunControls {
            realtime: None,
            execution_lease: None,
            steering,
            interrupt: crate::CancellationToken::new(),
            request_input: Box::new(|_| {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let _ = tx.send(vec![]);
                rx
            }),
            turn: Default::default(),
        }
    }

    /// An imported copy must never silently start a fresh provider session.
    #[tokio::test]
    async fn imported_sessions_never_fall_back_to_a_new_native_session() {
        let dir = tempfile::tempdir().unwrap();
        let agent = dir.path().join("agent");
        std::fs::create_dir_all(&agent).unwrap();
        // The override binary does not exist: if the driver ever reached
        // `spawn` the returned error would be about the missing executable,
        // so a resume-contract error proves no OMP process was started and no
        // prompt was dispatched. This stays cross-platform.
        let harness = OmpHarness::new()
            .with_executable(dir.path().join("omp-does-not-exist"))
            .with_agent_dir(&agent);

        // (a) The recorded native file is gone.
        let mut gone = request(Some("missing-id"), true);
        gone.cwd = dir.path().display().to_string();
        let Err(error) = harness.run(gone, controls()).await else {
            panic!("strict resume must fail");
        };
        assert!(error.to_string().contains("must resume"), "{error}");
        assert!(!error.to_string().contains("binary"), "{error}");

        // (b) An imported copy with no session id at all.
        let mut bare = request(None, true);
        bare.cwd = dir.path().display().to_string();
        let Err(error) = harness.run(bare, controls()).await else {
            panic!("strict resume must fail");
        };
        assert!(error.to_string().contains("no session id"), "{error}");
        assert!(!error.to_string().contains("binary"), "{error}");
    }

    #[test]
    fn thinking_selection_reads_the_omp_option() {
        let mut request = RunRequest {
            access: Default::default(),
            plan: false,
            prompt: "p".into(),
            harness: None,
            model: None,
            reasoning: None,
            model_options: Default::default(),
            cwd: "/tmp".into(),
            sandbox: zeron_proto::SandboxLevel::WorkspaceWrite,
            auto_approve: false,
            resume: None,
            require_native_resume: false,
            attachments: vec![],
            worktree: None,
            mcp: None,
            env: Default::default(),
        };
        assert!(!thinking_selected(&request));
        request
            .model_options
            .insert("omp_thinking".into(), "off".into());
        assert!(thinking_selected(&request));
    }

    #[tokio::test]
    async fn unsupported_or_missing_attachments_do_not_fail_the_turn() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("a.png");
        std::fs::write(&png, b"\x89PNG\r\n\x1a\nrest").unwrap();
        let svg = dir.path().join("b.svg");
        std::fs::write(&svg, b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>").unwrap();
        let missing = dir.path().join("gone.jpg");
        let images = load_images(&[
            svg.display().to_string(),
            missing.display().to_string(),
            png.display().to_string(),
        ])
        .await;
        let images = images.as_array().unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0]["mimeType"], "image/png");
    }
}
