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
