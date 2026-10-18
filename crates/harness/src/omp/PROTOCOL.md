OMP RPC compatibility contract

The native driver targets Oh My Pi (`omp`, `@oh-my-pi/pi-coding-agent`) and
speaks its own JSONL RPC (`omp --mode rpc`). Requests and events are JSONL,
never JSON-RPC. Responses and lifecycle events retain stdout order. OMP is a
Pi-derived product, not a Pi installation: it has its own store (`~/.omp`),
session format and framing, and this driver never reuses Pi's discovery or
session index.

Handshake. The first stdout frame is `ready`
(`supportedProtocolVersions: [1, 2]`, `maxFrameBytes: 1 MiB`,
`maxReassembledFrameBytes: 64 MiB`). The driver requires the advertised
version list to contain 1 and then sends `negotiate_protocol` with version 2.
Under v2 an oversized logical frame is emitted as an ordered `rpc_chunk`
sequence (256 KiB base64 payloads); the transport reassembles it before the
driver sees a frame, and a mismatched, interrupted or oversized sequence is a
protocol failure rather than silently dropped bytes. v1 remains a working
fallback and is used when negotiation fails.

Terminal. Every accepted `prompt` is acknowledged by its `response` and then
receives exactly one `prompt_result` correlated by the same `id`:
`completed`, `aborted` or `error` (with `error.message`, `provider`, `model`,
`httpStatus`, `retryable`), plus `sessionSettled`. `prompt_result` is the
authoritative turn terminator — unlike Pi there is no ACK/`get_state` barrier.
`session_settled` follows when no background work can wake the session; the
driver does not wait for it before completing the engine turn, matching the
other native drivers that tear the runtime down at the prompt terminal. A
rejected prompt command gets a `success: false` response and no
`prompt_result`, so the response itself terminates the turn with the error.

Events. Before the terminal, OMP emits `agent_start`, `turn_start`,
`message_start`, `message_update` (with Pi-shaped
`assistantMessageEvent.text_delta`/`thinking_delta`), `message_end`,
`tool_execution_start`/`tool_execution_end`, `turn_end` and `agent_end`, plus
OMP-only `available_commands_update`, `notice` and
`auto_compaction_start`/`auto_compaction_end`. Unknown event types are ignored
so a newer OMP cannot break an older driver; error and warning `notice` frames
surface as errors, informational ones do not enter the transcript.

Steering. Zeron's pending steers belong together at the next model step, so the
driver selects `set_steering_mode: "all"`. That command is session-scoped in
OMP (`persist: false`), so it never overwrites the user's machine-global
configuration. Each steer uses the dedicated `steer` command; its consumption
is the following user `message_start`, which emits the `Steered` boundary. A
changed model or thinking level is applied on the live process before the
message it was sent with. Interruption sends `abort`, then reads until the
`prompt_result` (status `aborted`) or `session_settled`.

Session identity is refreshed from `get_state` (`sessionId`, absolute
`sessionFile`, model, `autoCompactionEnabled`). Resume passes the verified
absolute file to `--resume`; a session that can no longer be resolved starts
fresh with a visible "without the previous context" notice, like the other
harnesses.

Identity boundaries. The private UUID -> file index is
`<agent>/zeron-omp-sessions`, a DEDICATED namespace beside Pi's
`<agent>/zeron-sessions`, so an explicitly shared `PI_CODING_AGENT_DIR` (both
products honor that variable) can never make them read or write each other's
records. Native stores differ by default too (`~/.pi/agent/sessions` vs
`~/.omp/agent/sessions`). If a user deliberately points both products at one
native directory — `PI_CODING_AGENT_DIR` or `PI_CODING_AGENT_SESSION_DIR` —
OMP accepts a file only when its head carries OMP markers (the `title` slot, a
per-entry `runtime` identity, `model_change.model`, or
`thinking_level_change.configured`), so Pi's `modelId`/`provider` files are
never listed by the OMP importer, resolved for resume, or parsed as OMP
history. Files are copied or renamed across products remain outside that
guarantee and are reported as an identity error rather than guessed at.

Sources inspected: `@oh-my-pi/pi-coding-agent@18.11.3` (`src/modes/rpc/*`,
`src/session/session-entries.ts`) and a live `omp/mode rpc` probe on
2026-10-18 capturing the handshake, catalog and a real prompt turn.
