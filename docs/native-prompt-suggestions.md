# Native next-prompt suggestions

The desktop composer displays a provider's predicted next user message in faint
text when the conversation is idle and the draft is empty. Press **Tab** or
**Right arrow** to copy it into the draft, edit it if needed, then send normally.
**Enter does not send an unaccepted suggestion.** Escape dismisses it; typing,
pasting, or composing with an IME also dismisses it. Existing drafts, attachments,
review comments, question answers, and queue edits take precedence.

## Provider support

| Harness | Support |
| --- | --- |
| Claude Code 2.1.308 or newer | Uses native `--prompt-suggestions` and `prompt_suggestion` stream-json messages. 2.1.308 is the oldest locally verified version, not a claim about when the vendor introduced the flag. |
| Older or unidentified Claude Code | No flag is added, so an unsupported option cannot prevent the conversation from starting. |
| Codex | Not enabled. The installed 0.165.0 schema includes `thread/prediction/request`, but a real request returned JSON-RPC `-32601`: `thread/prediction/request is not implemented yet`. |
| Other harnesses | No verified native interface is integrated; no suggestion generation is requested. |

There is no generic model-generated fallback. Claude's own disabled preferences
and usage guards remain in force. Its native generator makes an additional short
model request using the conversation's prompt cache and consumes provider usage.
Not every turn has a suggestion: short conversations, cold caches, plan mode,
errors, and usage limits can suppress one. Missing suggestions do not delay or
fail the original task. Title-generation runs never request suggestions.

Suggestions are optional session metadata, separate from conversation messages.
They can arrive after the task finishes without reopening it, changing the
completion marker, or creating another assistant message. The host ties them to
the completed transcript entry and synchronizes them through the document and
transcript watch, so a desktop client can display suggestions from a remote
execution device. New user messages clear the metadata. New turns, mismatched
sessions, duplicates, and invalid output are discarded or hidden.

The native mobile composers do not render this desktop affordance yet. The
shared Rust client carries the optional metadata; old clients and hosts remain
compatible.

## Evidence and verification

The [interface investigation](research/next-prompt-suggestion.md) links the
vendor contracts. A 2026-10-29 isolated Claude Code 2.1.308 stream-json probe
with tools disabled produced suggestions after the second and third `result`
messages. Both used exactly `type`, `suggestion`, `uuid`, and `session_id`, as
documented. The scratch conversation was not persisted. No existing work
conversation or provider preferences were changed.

Regression coverage includes post-result delivery, plan-mode suppression, stale
delivery during a follow-up, metadata replication and clearing, metadata-only
watch updates/reconnects, unchanged idle status/transcript, and composer preview,
adoption, undo, typing, IME, and wrapping.

Verified locally: all 46 non-ignored Claude adapter integration tests and eight
focused tests across the harness, document, engine, and desktop UI crates pass.
The four existing opt-in live integration tests remain ignored; the isolated
native probes described above were run separately. Test builds used
`CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0` to
keep generated artifacts within local disk capacity.
The six existing transcript-watch regression tests also pass, and both the
`zerun` desktop binary and `zerun-engine` binary build successfully.
