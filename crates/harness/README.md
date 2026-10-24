# Explicit agent installation

Settings → Providers → **Install** runs on the selected device, as its user. Merely
listing or detecting agents never invokes these CLI installers. Commands download
the vendor's current release, except for the pinned Antigravity archive.

| Agent / documentation | Unix command (Linux and macOS) | Native Windows command |
| --- | --- | --- |
| [Claude Code](https://code.claude.com/docs/en/setup) | `curl -fsSL https://claude.ai/install.sh \| bash` | `irm https://claude.ai/install.ps1 \| iex` |
| [Codex](https://github.com/openai/codex/blob/main/README.md) | `curl -fsSL https://chatgpt.com/codex/install.sh \| sh` | `irm https://chatgpt.com/codex/install.ps1 \| iex` |
| [GitHub Copilot](https://docs.github.com/copilot/how-tos/copilot-cli) | `curl -fsSL https://gh.io/copilot-install \| bash` | `npm.cmd install -g @github/copilot` |
| [Cursor](https://cursor.com/docs/cli/installation) | `curl https://cursor.com/install -fsS \| bash` | `irm 'https://cursor.com/install?win32=true' \| iex` |
| [OpenCode](https://opencode.ai/docs/) | `curl -fsSL https://opencode.ai/install \| bash` | `npm.cmd install -g @opencode/cli` |
| [Pi](https://pi.dev/docs/latest) | `curl -fsSL https://pi.dev/install.sh \| sh` | `npm.cmd install -g --ignore-scripts @earendil-works/pi-coding-agent` |
| [Grok](https://docs.x.ai/developers/release-notes) | `curl -fsSL https://x.ai/cli/install.sh \| bash` | `npm.cmd install -g @xai-official/grok` |
| [Hermes](https://hermes-agent.nousresearch.com/docs/getting-started/installation) | `curl -fsSL https://hermes-agent.nousresearch.com/install.sh \| bash` | `irm https://hermes-agent.nousresearch.com/install.ps1 \| iex` |
| [Devin](https://cli.devin.ai/) | macOS, when brew resolves: `brew install --cask devin-cli`; otherwise `curl -fsSL https://cli.devin.ai/install.sh \| bash` | `irm https://static.devin.ai/cli/setup.ps1 \| iex` |
| [Muse Code](https://dev.meta.ai/docs/muse-code/) | `curl -fsSL https://dev.meta.ai/install.sh \| sh` | `irm https://dev.meta.ai/install.ps1 \| iex` |
| Antigravity | Existing verified 1.1.1 archive in `acp::antigravity_archive`; no shell command | Same archive path on supported architectures |

Unix shell commands run through `sh -c`, with PATH augmented from the user's login
shell and npm toolchain. Native Windows `irm` commands run through
`powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command`.
Windows npm commands use the resolved npm launcher (normally `npm.cmd`).

If shell prerequisites are absent, these documented npm alternatives are used
when npm resolves: `npm install -g @openai/codex`,
`npm install -g @github/copilot`,
`npm install -g @opencode/cli`,
`npm install -g --ignore-scripts @earendil-works/pi-coding-agent`, and
`npm install -g @xai-official/grok`. Codex also falls back to npm on Windows when
PowerShell is unavailable. Grok's npm route is documented in its
[enterprise guide](https://docs.x.ai/build/enterprise); its
[published package metadata](https://registry.npmjs.org/@xai-official/grok/latest)
includes Windows binaries. No action is offered when none of the documented
methods' prerequisites resolves, or for Mock. The row then keeps a manual hint;
shell hints on Windows can be used in WSL.

DeepSeek Harness (`dsh`) is deliberately unmanaged: no Install action is
offered and Zerun never runs its updater. Settings shows the documented hint
(`npm install -g @deepseek-ai/dsh`, then
`dsh plugin --profile acp-plus add github:AndPuQing/dsh-acp-plus`) and reports
npm's `latest` dist-tag for awareness only. See
[docs/research/dsh.md](../../docs/research/dsh.md).

Install processes receive `CI=1`, `NONINTERACTIVE=1`, `TERM=dumb`, no stdin/TTY,
and no inherited `ZERON_*` or nested Claude environment markers. Output is bounded
and credentials are redacted before errors reach Settings. Cancel, timeout
(15 minutes), or dropping the request kills the owned process tree. There is no
automatic elevation: an installer that needs sudo or an interactive prompt may
fail. Cancelling does not undo files an installer already wrote.

After installation, executable version caches for the agent are invalidated and
the normal installed probe runs again. A missing binary produces PATH guidance;
the running app's login-shell snapshot is not refreshed. Authentication remains a
separate step. Hermes may additionally require its documented ACP Python extra
for agent sessions; the Install action checks CLI detection only.

## Muse Code

`muse::MuseHarness` drives `muse serve` over the Muse Session Protocol (MSP):
newline-delimited JSON-RPC 2.0 on stdio, the wire Muse's own TypeScript SDK
(`@muse-code/sdk`) speaks. Every state-changing method is a command carrying a
client-minted UUIDv7 `commandId`; a fresh turn's id is its `turn/start` command
id. A runtime is one host with one session (`session/start`, or
`session/resume` by id); turns end on `turn/completed`, including the wake turns
Muse starts when background subagents finish. Later prompts are
`turn/start` with `ifBusy: "steer"`, stops are `turn/interrupt`, approvals are
answered from the host's own choices, and questions reach the user through
`request_input`. Models (`model/list`) and titles run on a read-only host
(`--disable-shell --disable-write --no-session-log`). The host's environment
drops an inherited `META_API_KEY` and sets `MUSE_NO_AUTO_UPDATE=1`.

`tests/muse.rs` replays sessions recorded from a real Muse Code 1.4.x host
(`tests/fixtures/muse/*.ndjson`, through `fake-muse.py`).

## Verification

The `installer-fixture` feature exposes an explicit command injection seam for
integration tests. The engine's command environment override is compiled only
for unit tests, never production. Tests exercise successful detection, failure,
missing binaries, cancellation, duplicate requests, process descendants, bounded
redaction, platform prerequisites, and version-cache invalidation.

The opt-in engine test `rpc::tests::installer_rpc_npm` runs the documented Codex
npm alternative in a disposable prefix and isolated home, through `InstallHarness`.
It asserts that the returned Settings descriptor changes from uninstalled/disabled
to installed/enabled. It never runs a vendor shell installer. Set `TMPDIR` to a
scratch directory before running it with `--ignored --nocapture`.

The iOS app adds no Install action. `HarnessDescriptor.can_install` already has
`#[serde(default)]` (`crates/engine/src/registry.rs`); iOS's `WireHarness: Decodable`
in `apps/ios/Zeron/Sync/WorkspaceStore.swift` ignores unrecognized keys, including
`canInstall`. `HarnessCatalog` continues consuming the existing mapped fields.

## GitHub Copilot CLI

Install a current Copilot CLI with native ACP support, then run `copilot login`
on the device that executes your sessions. Enable **GitHub Copilot** in
Settings → Providers and select it when creating a conversation. Remote desktop
and mobile clients use the account on that execution device. Zerun uses the
CLI's existing login; account switching remains in Copilot CLI.

The driver launches `copilot --acp`, streams text and tool
results, accepts images, restores native sessions and supports stopping a turn
without losing the session. Follow-up prompts wait for the current turn to end.
The CLI supplies the available models and their config options; **Auto** is the
fallback when discovery is unavailable. `COPILOT_EXECUTABLE` can select a custom
binary. Updates use Settings' provider update action or `copilot update`.

## Saved session imports

The desktop and mobile import chooser supports nine production harnesses: Codex,
Claude Code, GitHub Copilot, Cursor, Grok, Hermes, DeepSeek Harness, Pi, and OpenCode. Discovery runs on the selected
execution device using its existing installation. Imports create independent native copies and archived
Zerun conversations; continuation resumes the copy. Sources reported as running
and unavailable working directories cannot be imported; native running-status
coverage is listed below.

| Harness | Discovery and copy |
| --- | --- |
| Codex | Native app-server history and fork APIs. |
| Claude Code | Native project logs under the selected Claude config directory. |
| GitHub Copilot | Local CLI sessions under `$COPILOT_HOME` (default `~/.copilot`), discovered and forked by the native headless RPC. Requires a CLI version with `sessions.fork` (verified with 1.0.98). Remote and detached sessions are excluded. |
| Cursor | The pinned 1.0.32 SDK: default SQLite stores under `~/.cursor/projects/*/sdk-agent-store` and Zerun-owned JSONL stores. The public SDK copies checkpoint blobs, runs and events into an independent store and resumes the new agent ID. Cursor CLI `.cursor/chats`, cloud sessions and arbitrary custom SDK stores are not scanned. `ZERUN_CURSOR_HISTORY_DIR` overrides the default `.cursor` root. |
| Grok | Format-1 native session directories under `$GROK_HOME/sessions` (default `~/.grok/sessions`), retaining model history, updates, plans and compaction files with a new native ID. Verified export and ACP restore with 1.0.4. Open sessions in the native active-session index are excluded. |
| Hermes | Native ACP `session/list`, `session/fork` and `session/load`, using `$HERMES_HOME` (default `~/.hermes`). Requires the ACP extra and a version advertising all three capabilities (verified with 0.18.2). The native API currently exposes ACP-origin sessions only; CLI/gateway history is excluded. ACP supplies no cross-process running-status signal; forks capture the persisted native context. |
| DeepSeek Harness | The default `$DSH_HOME/sessions` store, using installed dsh persistence packages for plain or zstd logs. Requires Node.js and an ACP profile with `session/load` support, such as `acp-plus`, to continue imported copies. Custom persistence roots are not scanned. |
| Pi | Native v2/v3 tree logs under `$PI_CODING_AGENT_DIR/sessions` (default `~/.pi/agent/sessions`) and an absolute global `sessionDir`. The copy retains all branches, signatures, compaction, and extension records; the transcript follows the current branch. Relative or project-specific custom stores are not discovered globally. |
| OpenCode | Native HTTP list, full transcript export, and fork APIs on both 1.18 and 2.x. The native server owns all database writes. |

Devin and Antigravity remain unavailable in the import chooser: their integrated
native interfaces can list and restore sessions but provide no verified public
independent-copy operation. Private DAG/trajectory databases are not rewritten
to manufacture a new identity. Mock is a test harness, not an import source.

Cursor snapshots a complete SDK store before opening it, so source migrations
never run in place; stores larger than 64 MiB are rejected. Grok copies a complete
session directory with the same bound. Copies reject symlinks and unsupported
formats.

Display transcripts omit attachments and unsupported blocks with a notice;
native copies retain provider-owned context. Source and display size limits
reject oversized imports instead of silently truncating conversation context.

The `saved::tests` tests cover transcript replay, busy sessions, source isolation,
Pi trees and both OpenCode HTTP generations.
Optional native compatibility tests use synthetic stores without model requests:

```bash
ZERON_DSH_TEST_PACKAGES=/path/to/.dsh/profiles \
  cargo test -p zeron-harness --lib acp::dsh_saved -- --ignored
ZERON_PI_TEST_MODULE=/path/to/pi/dist/core/session-manager.js \
  cargo test -p zeron-harness --lib pi::saved -- --ignored
ZERON_OPENCODE_TEST_BINARY=/path/to/opencode \
  cargo test -p zeron-harness --lib opencode::saved -- --ignored
ZERON_COPILOT_TEST_BINARY=/path/to/copilot \
  cargo test -p zeron-harness --lib copilot_saved -- --ignored
ZERON_CURSOR_TEST_MODULE=/path/to/@cursor/sdk/dist/esm/index.js \
  cargo test -p zeron-harness --lib cursor::saved -- --ignored
ZERON_GROK_TEST_BINARY=/path/to/grok \
  cargo test -p zeron-harness --lib grok_saved -- --ignored
ZERON_HERMES_TEST_BINARY=/path/to/hermes \
ZERON_HERMES_TEST_SOURCE=/path/to/hermes-agent \
ZERON_HERMES_TEST_PYTHON=/path/to/hermes-agent/venv/bin/python \
  cargo test -p zeron-harness --lib acp::saved -- --ignored
```
