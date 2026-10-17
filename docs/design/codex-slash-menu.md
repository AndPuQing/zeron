# Codex slash menu

Baseline: `origin/dev` at `6d9db999`. Reference: the official Codex commands
page and app-server documentation, checked 2026-10-16; protocol schemas from
codex-cli 0.161.0 (`generate-json-schema --experimental`).

The menu lists the documented CLI commands and aliases. Native operations use
app-server requests; matching desktop actions use the existing Zeron UI. Commands
that require the Codex terminal UI remain discoverable with an explicit reason
and cannot be submitted as model prompts. This is a versioned catalog, not a
claim that app-server supplies a dynamic slash-command discovery endpoint.

Goals use `thread/goal/{set,get,clear}`, including explicit user provenance.
Only an explicitly supplied token budget is forwarded. Native replies that do
not create a turn produce a readable response and terminal event immediately;
the persistent session remains available for the next message. Unsupported
server methods fail visibly without retrying the command as prompt text.

Keep provider commands and Zeron actions distinct where semantics differ
(Codex `/stop` stops background terminals; `/zeron:stop` stops a turn). Map
client actions before generic workspace aliases are added. Disable terminal-only
entries in the composer while retaining server-side rejection of manual input.

Verify catalog coverage, goal payloads and usage preservation, disabled-command
selection, native reply completion, continued use of a warm session, and the
existing compact/review behavior with synthetic subprocess fixtures. No real
provider request, account mutation or live release is needed for these checks.

Official references:
- https://developers.openai.com/codex/cli/slash-commands/
- https://developers.openai.com/codex/app-server/

## Mobile clients

The iOS and Android session and new-session composers load `ListCommands` and
`ListSkills` from the selected execution host, scoped to the chat or project.
They share the Rust client catalog and UniFFI records, rather than duplicating
Codex's static CLI catalog in Swift/Kotlin. A leading `/` opens a scrollable,
searchable menu with descriptions, argument hints and unavailable reasons.
Selection preserves the remaining draft and does not immediately execute it.
Selected skills retain their canonical identity, including their path and
provider command; same-name skills receive distinct display tokens.

Provider/host/project changes invalidate the open catalog and selected mappings.
Each opening reloads the host catalog, while typing filters it locally. Cancelled
loads cannot replace a later context. Partial catalog failures preserve successful
results and expose retry controls; an unsupported older host reports the missing
RPC instead of showing a fabricated local catalog. `ListCommands` uses the same
100-second cold-agent deadline as the desktop request.

Codex desktop actions are unavailable in the wire catalog by default; the desktop
composer enables only the actions it actually handles. Mobile displays those
entries with the desktop requirement and retains its existing model chips,
navigation and stop button. Native commands such as `/goal` execute on the host.
This does not port Codex's terminal UI or desktop workspace actions to phones.

Validation covers catalog filtering, partial failures, selected-reference
encoding, UTF-16 cursor ranges, completion inside a token, argument preservation,
and unavailable commands. iOS XCTest and Android model regressions are checked
in for the existing platform CI jobs. Generated Swift bindings must match the
Rust facade; Android bindings are generated during its build.
