# Per-provider agent environment configuration

## Problem and scope

Agent processes currently inherit the engine's environment, with a few
provider-specific and process-ownership adjustments. Users cannot persist
different overrides for each provider through Providers settings. Configuring
one engine-wide proxy or API key therefore cannot express different provider
connections on the same device.

The scope is one configuration per **execution device and provider**.
The desktop Providers page manages it, including through its existing target
device selector. Headless hosts use the same store and may be configured from
another trusted desktop. Agent execution remains cross-platform.

This feature covers all registered production providers, with
actual ACP launches as required acceptance cases. The current protocol mix is:

| Provider | Current process/protocol |
| --- | --- |
| Devin, Grok, Hermes, Antigravity | ACP over stdio |
| Claude Code | Native stream-json |
| Codex | Native app-server JSON-RPC |
| Cursor | Pinned SDK through the Node shim |
| OpenCode | Native HTTP/SSE server |
| Pi | Native JSONL RPC |

No provider is converted to ACP for this feature. Mock remains test
infrastructure. The first delivery exposes desktop editing; mobile may use
configured hosts without adding mobile settings or changing UniFFI exports.
Project-specific and conversation-specific overrides are separate future work.

## User behavior

Each provider's expanded Settings card adds an **Environment variables**
section. Its heading identifies the selected device. The editor supports:

- Add or replace a named value using single-line inputs.
- Remove a variable from the child environment explicitly.
- Delete an override to return to the inherited default.
- Save or discard a draft, with validation attached to the affected row.

An empty string is a valid value; it is different from removing the variable.
Values are literal UTF-8 strings. There is no shell execution, `$VAR` expansion,
command substitution, `.env` sourcing, or automatic repository-file import.

Existing values are masked and are not fetched with the listing. Replacing a
value requires entering the replacement. An explicit **Show value** action
may fetch one selected value and displays it only until the editor closes,
the target device changes, or its engine connection is replaced. The editor
does not copy values automatically or put them in persistent UI drafts.

Save reports persistence failure or a concurrent-edit conflict. It does not
claim success before the host commits the configuration. Changing devices
while a request is pending cannot install that response into the new device's
editor. An unavailable host preserves the draft and exposes retry.

The section explains that saved changes apply to newly started agent
processes. It identifies sessions still using an earlier configuration.
Saving never interrupts a turn, discards queued messages, or ends a voice call.

## Ownership and storage

The owning engine stores `agent-environment.json` under its device data root,
alongside provider enablement. The file is independent of `harness-prefs.json`:
that existing file has best-effort persistence and is not suitable for the
acknowledged, private writes required here.

Persisted shape, with illustrative values only:

```json
{
  "schemaVersion": 1,
  "providers": {
    "grok": {
      "revision": "<opaque revision token>",
      "entries": {
        "HTTPS_PROXY": {
          "action": "set",
          "value": "http://127.0.0.1:7897"
        },
        "EXAMPLE_TOKEN": {
          "action": "unset"
        }
      }
    }
  }
}
```

The configuration is device-local, shared by that device's workspace profiles
like provider installation and credentials. It does not enter the registry,
session documents, durable commands, journals, attachment storage, Edge, or
version control. Sign-in, sign-out, or workspace imports do not copy it.

Writes validate a candidate under a serialized writer, persist a complete
same-directory temporary file, sync it, and atomically replace the destination
before publishing the new in-memory snapshot. Unix permissions are `0600`
from temporary-file creation; Windows uses an owner-restricted ACL on the
private file and its replacements. The store follows the repository's
private-file credential persistence approach; the JSON is not encrypted.

Missing files mean no overrides. Existing malformed files, unsupported schema
versions, or failed reads produce an explicit configuration error, preserve
the original bytes, and block affected launches rather than silently using
different credentials. Store failures do not invalidate the last committed
in-memory state. Engine boot and unrelated UI remain available for diagnosis.

Each changed provider receives a fresh opaque revision token. A no-op save
does not change it. Clearing all entries retains the revision to prevent stale
editors from confusing a cleared configuration with an untouched one. The
file is engine-managed; direct edits are not the management interface.

## Validation and precedence

Limits, enforced by the engine as well as the editor:

| Item | Rule |
| --- | --- |
| Name | `[A-Za-z_][A-Za-z0-9_]*`, at most 128 bytes |
| Value | UTF-8, no NUL; at most 16 KiB per value |
| Entries | At most 64 per provider |
| Encoded override payload | At most 64 KiB per provider |
| Duplicate keys | Reject within one mutation |
| Unix name comparison | Case-sensitive |
| Windows name comparison | Case-insensitive, matching native command preparation |

Names are rejected rather than trimmed or silently renamed. Values retain
whitespace and newlines. Native Windows launch additionally validates the
complete inherited-plus-overridden environment against its platform limit;
an oversized environment produces a readable error before spawning.

Process construction follows this order:

1. Resolve the executable through the existing engine policy.
2. Inherit the existing child environment and compose the login-shell PATH.
3. Apply this provider's `set` and `unset` entries to the command object.
4. Apply provider/engine-owned launch controls and nested-agent marker removal.

`PATH` is an explicit replacement when configured, applied after composition.
It does not change engine executable detection. No implementation mutates the
engine's global environment with `std::env::set_var` or a shell wrapper.

The editor receives a documented reserved-name policy. Engine routing
(`ZERON_*`, `ZERUN_*`), nested-agent markers, managed scratch locations,
provider server credentials, and other engine-owned controls cannot be
overridden or removed. Reject them at save time, with the variable name and
reason; do not accept and then silently ignore them.

For this first release, variables selecting identity/configuration roots
(`HOME`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `XDG_*`, `CODEX_HOME`,
`CLAUDE_CONFIG_DIR`, `PI_CODING_AGENT_DIR`, `GROK_HOME`, `HERMES_HOME`,
`GEMINI_HOME`, `HERMES_SHARED_AUTH_DIR`, `OPENCODE_CONFIG`, `OPENCODE_CONFIG_DIR`, and
`OPENCODE_CONFIG_CONTENT`) are reserved. The current account manager, skill
discovery, import readers, and context hashing resolve these from the engine
environment; supporting per-provider roots requires changing those readers
together. Browser launch routing (`BROWSER`) and adapter-owned `PYTHONUNBUFFERED` are reserved as process controls. Provider executable selectors are also reserved and keep their
existing application-level override mechanism.

Network settings, provider credentials and ordinary feature flags remain
configurable. This feature does not transform provider diagnostics or output
by matching configured values.

Credential overrides may take precedence over a provider's saved CLI login.
The editor states that relationship. Existing saved-account usage meters must
not claim to measure an environment-selected API account without evidence from
that provider; preserve their explicit saved-account identity.

## Engine and harness integration

Add an `EnvironmentStore` owned by the engine and a platform-neutral validated
environment type in `zeron-harness`. RPC records and capability identifiers
belong in `zeron-proto`. Secret values stay in ephemeral host-side objects;
do not add them to the serializable `RunRequest` or `ChatConfig`.

The registry retains provider factories after first resolution and binds a
resolved harness to one immutable environment snapshot/revision. Repeated
operations reuse the current instance. Active operations retain their old
instance; obsolete instances are released when their consumers finish. Fixed
fixture harnesses continue to work, and production providers receive the
snapshot through their constructors or an equivalent explicit launch context.

Apply the snapshot at every provider-owned process boundary:

- Coding runs, safe native resume and title generation.
- Model/command/skill probes that launch the provider, including Devin's
  separate `models list` path.
- Provider-owned authentication subprocesses, using the same device settings.

Authentication launches in `AgentAccounts` must capture the binding too; they
currently construct some provider commands independently of the registry.

Installation, npm adapter acquisition, CLI update checks/installers, terminal
shells, preview discovery, the engine's HTTP clients, and voice media helpers
retain their existing environments. Codex's agent/app-server process receives
its configured environment, including when it hosts a voice session; the
client's media helper keeps its deliberate environment allowlist.

Adapter-specific launch controls still go through the existing command and
process-ownership abstractions. Preserve process-group/Job Object cleanup,
stdio isolation, cancellation, browser callback routing and scratch cleanup.

## Configuration changes and persistent sessions

Every operation captures one immutable binding before it starts. Its probes,
process and cache context use that binding throughout.
Saving concurrently cannot produce a mixed environment.

Add the opaque environment revision to the engine's runtime compatibility
identity and catalog/cache identity. Do not hash or log raw configured values
to expose that identity. A newly configured provider cannot reuse models,
command initialization, cooldown state or a background discovery result from
an earlier revision. A stale discovery response cannot overwrite the new
revision's cache or UI catalog.

For persistent coding processes:

- A working turn, awaiting-input turn, active subagent and voice owner retain
  their original environment. Explicit steering and input answers target that
  existing process and keep its configuration.
- A subsequent ordinary send requiring the latest environment waits for a
  safe turn boundary. It must not fall into the current configuration-mismatch
  path that interrupts the live run.
- When the runtime can safely retire, its next turn starts a process with the
  latest revision and the existing provider-native resume policy. If it cannot
  retire safely, retain the queued send and its status rather than losing it.
- An ordinary queued message captures the current environment at actual
  dispatch, not when it was typed. Only the revision/status may appear in
  local runtime diagnostics; secret values never enter the queue.

The configuration save path never waits for all active runs to end and never
holds a shared execution lease while waiting for an exclusive update lease.
Retirement continues using the existing session/update-coordination rules.

### Implementation touchpoints

| Area | Existing code / planned addition |
| --- | --- |
| Shared RPC contracts | `crates/proto/src/workspace.rs`, new environment records module |
| Private configuration | New `crates/engine/src/agent_environment.rs`, assembled in `engine/src/lib.rs` |
| Routing and configured factories | `engine/src/rpc.rs`, `engine/src/registry.rs`, `rpc/src/lib.rs` |
| Process policy and snapshots | New `harness/src/environment.rs`, existing command/process ownership helpers |
| Provider launch and authentication | `harness/src/acp/` and native drivers, `engine/src/agent_accounts.rs` |
| Runtime and cached discovery | `engine/src/sessions.rs`, `engine/src/model_catalogs.rs`, `harness/src/model_context.rs` and provider caches |
| Desktop editor | New `ui/src/settings/environment.rs`, mounted by `settings/harnesses.rs` |

Use these modules to keep the feature out of the already large shell and
composer files.

## RPC and compatibility

Add capability `harness-environment-v1` to `EngineInfo` and device capability
metadata. The three methods below support `targetDeviceId` and are added to
the existing forwardable-method list. They use the current authenticated
device trust boundary, with no Edge persistence or room-protocol change.

| Method | Request | Reply |
| --- | --- | --- |
| `GetHarnessEnvironment` | `harness`, optional target | Revision, entry names/actions, validation policy; no values |
| `PatchHarnessEnvironment` | `harness`, `expectedRevision`, ordered changes, optional target | Committed metadata and revision; no values |
| `RevealHarnessEnvironmentValue` | `harness`, `name`, `expectedRevision`, optional target | One value after explicit user action; revision |

Patch operations are `set(name, value)`, `unset(name)`, and
`delete(name)`. Validate the complete resulting configuration and apply the
patch atomically. Compare revisions under the store writer lock. A conflict
returns current metadata while preserving the editor's draft.

An unacknowledged save is not replayed automatically: refetch the owning
host's metadata and let the user explicitly retry. Remote failure cannot fall
back to writing this device's store. A removed or changed provider revision
invalidates an in-flight reveal. Older hosts display an unavailable settings
section and keep existing launching behavior; version numbers alone do not
imply support.

## Acceptance and verification

The feature is complete only after verifying the behavior below with isolated
profiles and subprocess fixtures:

1. Two providers on one engine receive different values under concurrent
   launches; another provider, the parent process and another engine do not.
2. Real child-process observations distinguish set, empty value, unset, and
   deletion/restored inheritance, including Unicode values and metacharacters.
3. ACP coding launches and model discovery agree, including Devin's separate
   probe. Covered native drivers behave consistently with their own probes.
4. Saved settings survive restart; a failed atomic write, invalid existing
   file, concurrent edit or unsupported schema does not silently change them.
5. Remote reads/writes affect only the targeted device. Older hosts, offline
   targets, stale responses and lost acknowledgements have defined UI states.
6. A save during an active turn does not interrupt it. Explicit steering uses
   its old binding; the next safe process uses the new binding. Pending sends,
   input answers, native resume, subagents and voice ownership remain intact.
7. Changing configuration invalidates model/command caches and fences older
   background results; configured values do not appear in listing metadata.
8. Unix permissions and Windows ACL/environment rules are exercised on their
   respective platforms. Job Object/process-group cancellation still cleans
   up fixture descendants.
9. The settings editor is usable by keyboard, masks values, preserves unsaved
   changes on failure, and clears transient revealed values when dismissed.

Unit tests cover validation, store transactions, revisions and command
application. Integration tests observe child output and actual device relay
routing; runtime tests cover the turn-boundary race. GPUI fixtures cover the
editor and device switching. Tests are added with the relevant implementation
commits rather than only mirroring helper methods.

Required checks include the affected proto, harness, engine, RPC and UI suites;
the existing core CI and platform workflows provide regression coverage.
Live-provider checks, if run, record provider version, chosen model, isolated
profile and result separately. Credentials and private transcripts are never
test artifacts committed to the repository.
