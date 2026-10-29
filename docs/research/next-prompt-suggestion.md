# Next prompt suggestion feasibility

Investigated on 2026-10-29 against Zerun `cbd0b280`, local Claude Code
2.1.308, codex-cli 0.165.0, and the published Claude Agent TypeScript SDK
0.3.312 declarations. This investigation preceded the implementation described
in [Native next-prompt suggestions](../native-prompt-suggestions.md).

Implementation follow-up: a real Codex 0.165.0 request returned `-32601`,
`thread/prediction/request is not implemented yet`; Codex is therefore not
enabled. A real Claude 2.1.308 probe returned suggestions after the second and
third completed turns. The original research and proposed design below are
retained as the evidence leading to that decision; they are not a list of
shipped functionality.

## Result

**Claude has a documented native headless integration for this feature.**
Codex exposes a native experimental prediction request/result contract that
is a promising second integration, but its generation behavior has not been
verified with a real model request. This is stronger evidence than the
auto-recap investigation: a terminal-only limitation must not be inferred for
prompt suggestions.

The intended interaction is a proposed next **user** message in an empty
composer. Accepting it populates an editable draft; sending is a separate user
action. It is not an assistant response, an automatic follow-up turn, or a
slash-command completion.

## Claude Code: verified interface

The [interactive-mode documentation](https://code.claude.com/docs/en/interactive-mode#prompt-suggestions)
explicitly supports print mode with `--prompt-suggestions`, `--output-format
stream-json`, and `--verbose`. Local `claude --help` confirms the flag and
describes its predicted-next-user-prompt output in print/SDK mode.

The [TypeScript SDK reference](https://platform.claude.com/docs/en/agent-sdk/typescript#sdkpromptsuggestionmessage)
documents `promptSuggestions: true` and this message shape:

```json
{
  "type": "prompt_suggestion",
  "suggestion": "Run the tests for the changes",
  "uuid": "<message UUID>",
  "session_id": "<native session ID>"
}
```

This is a schema illustration, not a recorded model result. The published
[`sdk.d.ts` for 0.3.312](https://unpkg.com/@anthropic-ai/claude-agent-sdk@0.3.312/sdk.d.ts)
adds important delivery semantics:

- At most one suggestion per turn, arriving **after `result`**. Consumers must
  continue reading the stream after turn completion.
- Some turns produce no suggestion: first turn, API errors, plan mode,
  provider settings/environment disabling suggestions, and usage-limit
  conditions are documented examples. The interactive documentation also
  describes cold-cache and short-conversation suppression. Absence is not a
  protocol failure and must not hold the task open.
- The native generator reuses the conversation prompt cache. The documentation
  says this is an extra short request to the session's model and counts toward
  usage limits/API costs. It is not free and a fixed cost cannot be promised.
- Respect `promptSuggestionEnabled: false` and
  `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION=false`; do not force the environment to
  true to override the user's provider preferences.

The message carries a session ID and its own UUID, **not a source turn ID**.
Late delivery after a new user message/steer needs conservative invalidation;
the UUID must not be treated as a turn identifier. Rapid consecutive turns
and background-agent wake turns need a real capture before relying on any
stronger ordering guarantee.

The minimum supporting CLI version was not established. Version 2.1.308
advertises the flag; older installations need capability detection or a
verified version gate. Do not add the flag unconditionally to every version.

## Codex: verified experimental schema, execution still unverified

The [official app-server guide](https://developers.openai.com/codex/app-server/)
does not describe the prediction operation in the fetched reference. Local
0.165.0 schema generation supplies the concrete contract:

```json
{
  "id": 42,
  "method": "thread/prediction/request",
  "params": { "threadId": "<thread>", "sourceTurnId": "<completed turn>" }
}
```

The response has an empty object schema. A separate notification carries the
result and repeats both IDs:

```json
{
  "method": "thread/prediction/updated",
  "params": {
    "threadId": "<thread>",
    "sourceTurnId": "<completed turn>",
    "result": { "type": "completed", "text": "<prediction or null>" }
  }
}
```

`text` is optional and nullable. Another result variant is `{ "type": "failed" }`.
Treat no text or a failed prediction as no suggestion, not a failed task.
The request appears only when generating with `--experimental`; the result
notification also appears in the non-experimental schema. Zerun already
negotiates `experimentalApi: true`, but that alone does not prove a particular
server/model/account can generate predictions.

The schema also contains `thread/fork`'s `experimentalPredictionMode`, which
inherits the loaded parent's effective tools/config/history. Do not replace
the dedicated request with an ordinary fork-and-prompt operation, or assume
that this fork option disables tools. Actual prediction semantics, eligibility,
resource use, late-result behavior, and failure isolation require a live test.

ACP v1/v2 schemas and Pi's RPC command reference inspected in the auto-recap
investigation expose no corresponding standard prediction operation. This
does not exclude provider extensions. During implementation, the published
declarations for `@cursor/sdk@1.0.32` (148 declaration files) and
`@muse-code/sdk@1.4.14` (19 declaration files) were also inspected; no declared
prompt-suggestion/prediction API was found. This is not a claim about private
terminal UI features. OpenCode and Oh My Pi have no verified native suggestion
interface in this investigation.

## Changes Zerun would need

1. **Native adapter support.** Gate the Claude launch flag by provider version
   and the feature preference; parse `prompt_suggestion` in
   `crates/harness/src/claude/wire.rs`. Currently it becomes `Frame::Other` and
   the normalizer discards it. For Codex, issue the dedicated request at the
   appropriate completed-turn boundary and handle its notification, with
   capability/error fallback for older servers. The current driver ignores
   that unknown notification.
2. **Separate suggestion state.** Add a normalized suggestion event and store
   it outside transcript messages, with chat/runtime/source identity. Handle
   it before the parked-session tail filter in `crates/engine/src/sessions.rs`.
   Do not map it to `TextDelta`: that can wake an idle session as if the agent
   resumed work. Do not delay `Done` while waiting for a suggestion.
3. **Host/client delivery.** Carry optional suggestion state over the existing
   host/client state path, including remote execution. Scope it to the source
   conversation and invalidate it on a new submitted message, session reset,
   or superseding turn. Deduplicate native messages; ignore stale results.
4. **Composer display.** `crates/ui/src/composer.rs` has `set_ghost`, but its
   current renderer deliberately requires a non-empty draft and is used by
   palette completion. Add a distinct empty-composer suggestion path, with
   proper wrapping, accessible text, and no draft mutation on arrival.
   Hide/dismiss it on typing, paste, IME composition, or incompatible completion
   popups. Never overwrite an existing draft.
5. **Explicit adoption.** Tab (and optionally right arrow) accepts into the
   draft; Enter sends only an actual draft. An unaccepted suggestion alone
   must never be sent. Resolve key precedence with existing completion and
   focus navigation. Mobile needs a visible accept action if added there.
6. **Preference and lifecycle.** Expose an enable/disable preference for
   supported providers, explain extra usage briefly, and clear any visible
   suggestion when disabled. Failure, timeout, or no suggestion must leave the
   original task and its queue/status intact. Older hosts must degrade to no
   suggestion rather than a broken chat.

Recommended implementation order: Claude first, with real post-result captures
and stale-delivery tests; then Codex after validating the experimental request
with an isolated test conversation. No independently generated fallback is
needed to start with Claude.

## Initial investigation: validation and limits

Read current official documentation and the published TypeScript declarations;
checked local CLI help, generated both Codex schema variants, and inspected
Zerun's adapter, parked-event handling, and composer code. Reproduction:

```sh
claude --version
claude --help
codex --version
codex app-server generate-json-schema --experimental --out /tmp/zerun-recap-codex-schema
codex app-server generate-json-schema --out /tmp/zerun-suggestion-codex-stable
```

No model requests, real suggestion captures, UI changes, or end-to-end tests
were performed. Required implementation coverage includes post-`result`
delivery, missing suggestions, duplicate/late messages, stale turn IDs, rapid
steering, native errors, existing drafts, IME/paste, shortcut conflicts,
disablement, old CLI/host compatibility, and unchanged task completion/status.
