# Provider environment configuration

Provider environment settings are scoped to one execution device and provider.
The desktop Providers page selects the owning device and edits its settings,
including for disabled providers. Mobile clients can run agents on configured
hosts. Settings are shared across the host's workspace profiles; project and
conversation overrides are outside this feature.

The configured drivers are Claude Code, Codex, Cursor, Devin, Grok, Hermes,
Antigravity, OpenCode and Pi. Each driver keeps its existing process and
protocol implementation. Mock is test infrastructure.

For editor instructions, see the [user guide](../agent-environment.md).

## Editor and value semantics

The editor uses single-line name/value inputs and compact saved rows. Existing
values are absent from the listing and appear masked. Edit opens a blank
replacement input; changing it stages a replacement. Done closes the form
without saving. Save and Discard appear when changes are pending.

The row menu exposes Show value, Hide value, Unset in child and Restore
inheritance as applicable. The section menu contains Reload status.

| Operation | Result in a new child process |
| --- | --- |
| Set a value | Replace the inherited value with the literal string |
| Set an empty string | Keep the variable with an empty value |
| Unset in child | Remove the variable even if the engine inherited it |
| Restore inheritance | Delete the override and use the normal child environment |

Values are literal UTF-8 strings. There is no shell expansion, command
substitution, `.env` sourcing or repository-file import. The storage/RPC value
format preserves whitespace and line breaks; the desktop editor provides only
a single-line input. Provider output and diagnostics are not filtered by
matching configured values.

Drafts and revealed values live in the open editor, not persistent UI settings.
Closing provider details or changing devices replaces the editor and clears
both. Hide value, reloading metadata or replacing the engine connection also
clears revealed values. A connection failure or concurrent-save conflict keeps
the open editor's draft. A connection generation check rejects stale replies.

## Store and validation

[EnvironmentStore](../../crates/engine/src/agent_environment.rs) loads
`agent-environment.json` from the owning engine's application data directory.
The file is separate from `harness-prefs.json` and contains plaintext JSON:

```json
{
  "schemaVersion": 1,
  "providers": {
    "grok": {
      "revision": "<opaque revision token>",
      "entries": {
        "HTTPS_PROXY": {
          "action": "set",
          "value": "http://proxy.example:8080"
        },
        "EXAMPLE_TOKEN": {
          "action": "unset"
        }
      }
    }
  }
}
```

The file is device-local and is excluded from conversation and workspace sync.
Configured values are not added to `RunRequest`, `ChatConfig` or queued-message
records. The editor sends values in patches and fetches a saved value only
through an explicit reveal request.

A store mutex serializes revision checks and writes. A changed candidate is
validated, written to a same-directory temporary file, synced and atomically
renamed before the in-memory snapshot is published. Unix files use mode `0600`;
Windows files receive an owner-restricted ACL before configuration bytes are
written. A failed write leaves the committed in-memory snapshot unchanged.

A missing file means no overrides. Malformed, oversized or unreadable files,
unsupported schema versions and invalid entries produce a configuration error
without rewriting the file. Provider resolution and environment settings RPCs
report that error. The file is read during engine initialization; after repairing
it, restart the owning engine. Reload status refreshes RPC metadata, not the
file on disk.

Every changed provider gets a fresh opaque revision token. A no-op patch keeps
its revision. Removing all entries retains a revision so an old editor cannot
mistake cleared settings for an untouched provider.

The engine validates the complete candidate configuration. The editor checks
names and individual values before sending a patch. Limits come from
[the shared environment module](../../crates/harness/src/environment.rs):

| Item | Rule |
| --- | --- |
| Name | `[A-Za-z_][A-Za-z0-9_]*`, at most 128 bytes |
| Value | UTF-8, no NUL, at most 16 KiB |
| Entries | At most 64 per provider |
| JSON-encoded entries | At most 64 KiB per provider |
| Patch changes | At most 128, with no duplicate names |
| Name comparison | Case-sensitive on Unix; case-insensitive on Windows |

Names are rejected rather than trimmed or renamed. Reserved names are checked
case-insensitively on every platform. They include identity/configuration roots
such as `HOME`, `CODEX_HOME`, `CLAUDE_CONFIG_DIR`, `XDG_*` and OpenCode
configuration selectors, plus `ZERON_*`, `ZERUN_*`, executable selectors,
temporary-directory controls, nested-agent markers and provider-owned server
controls. The shared validator is the complete policy; rejected saves report
the variable name or validation reason. Windows also validates the complete
child environment against its 32767 UTF-16-unit limit before spawning.

## RPC and remote ownership

The [RPC records](../../crates/proto/src/agent_environment.rs) use capability
`harness-environment-v1`. All three methods accept `harness` and optional
`targetDeviceId` and use the existing authenticated device routing:

| Method | Additional request fields | Reply |
| --- | --- | --- |
| `GetHarnessEnvironment` | None | Revision, names/actions, limits and sessions using previous settings; no values |
| `PatchHarnessEnvironment` | `expectedRevision`, `changes` | `conflict` and current metadata; no values |
| `RevealHarnessEnvironmentValue` | `name`, `expectedRevision` | One value and its revision |

Patch changes have actions `set`, `unset` or `delete`. Set includes `name` and
`value`; unset and delete include `name`. A revision conflict returns fresh
metadata without applying the patch. The editor preserves its draft against
that metadata for review and explicit retry.

A failed save reply requires Reload status before another save. The editor
does not automatically replay an unacknowledged write. A reveal with an old
revision fails; stale responses cannot install values in a replacement editor
or engine connection. An unavailable remote device never falls back to the
local store. Hosts without the capability show an unavailable settings section.

## Process injection and session updates

[HarnessRegistry](../../crates/engine/src/registry.rs) retains provider factories
and caches each configured harness by environment revision. Resolving after a
save creates a harness with the new immutable snapshot. Operations already
holding an older harness keep their captured settings.

Drivers apply that snapshot to their command objects after inherited/login-shell
PATH composition and before engine-owned launch controls. A configured PATH
replaces the child's composed PATH; CLI discovery keeps the engine's executable
lookup policy. Injection does not mutate the engine's global environment.

Provider runs, native resume, title generation, provider discovery subprocesses
and provider CLI login subprocesses receive the snapshot. Credential variables
may take precedence over a saved CLI account; account usage meters continue to
describe that saved account. Installers, CLI update processes, terminal shells,
preview discovery, application-owned HTTP clients and voice media helpers keep
their existing environments.

The revision partitions provider model/cache context and runtime compatibility.
Catalog publication rejects stale discovery results. Desktop model pickers and
composer completion contexts are invalidated for the owning device/provider
when the editor observes a committed revision change.

[Session dispatch](../../crates/engine/src/sessions.rs) keeps a working turn,
awaiting-input turn, subagents, accepted steering and active voice on their
captured environment. For a session still using an earlier revision, an ordinary
send stays queued until that runtime can retire safely: the turn is complete,
subagents and voice are inactive, and accepted steering has been handled. The
next runtime captures the latest revision and follows the provider's existing
resume policy. Saving neither interrupts a turn nor waits for other sessions.

## Maintenance

Keep provider launch and probe paths using the same immutable snapshot when
adding a subprocess. Include provider-owned authentication launches in
[AgentAccounts](../../crates/engine/src/agent_accounts.rs). Keep reserved names
aligned with engine-owned controls and any readers that resolve identity roots.

Relevant tests live beside the store, shared validator, registry, sessions and
[desktop editor](../../crates/ui/src/settings/environment.rs). Subprocess
injection cases are in
[agent_environment.rs](../../crates/harness/tests/agent_environment.rs), and
execution-device routing cases are in
[device_routing.rs](../../crates/engine/tests/device_routing.rs). Use isolated
profiles and synthetic values for regression checks. Report validation results
in the PR or delivery reply; keep credentials, private transcripts and operation
logs out of public test artifacts.
