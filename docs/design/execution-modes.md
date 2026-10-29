# Permission and Plan completion on top of dev

Baseline: `185075c5` (2026-10-29). Dev already supplies the desktop access menu,
`ChatConfig.access/plan`, `RunRequest.access/plan`, harness capability declarations,
native permissions/Plan and staged approvals. Those implementations remain the
foundation; this PR does not add a second mode system or new Muse integration.
OpenCode 2.x support from dev is retained.

## Remaining behavior implemented here

- Android and iOS expose permissions and Plan in the composer and model picker.
  Their bindings round-trip the existing access/plan fields instead of resetting
  them. Choices use the host's advertised harness capabilities; an unknown or old
  host offers Full access only. Demo fixtures have an explicit synthetic catalog.
- Model changes retain policy. Menu actions patch just their own field against
  the latest config. Config-only synchronization refreshes open composers.
- Unsupported selections stay selected and fail admission instead of falling
  back to Full access or disabling Plan. Full access remains the legacy default.
- `execution-policy-v1` identifies hosts that implement the snapshot/ordering
  contract. Restricted/Plan messages to older hosts are rejected before sending.
- `ExecutionPolicy` is a command snapshot/helper over the existing access/plan
  fields, not another chat or run configuration. Run, Steer and queued messages
  retain their submission-time restrictions through edits, promotion and restart.
  Admission combines envelope and current registry restrictions using the stricter
  access and logical OR for Plan, because the documents sync independently.
- Changing policy waits until the current turn ends. The following turn replaces
  the runtime, including its frozen MCP policy; selecting a mode never kills work.
- Model-config RPCs that omit access or plan preserve each omitted field. Current
  clients explicitly serialize both fields, including Full access and Plan off.
  Legacy clients writing directly to the CRDT cannot be made field-aware.
- MCP delegation inherits the launching runtime's restrictions and Plan. It
  cannot address a broader-policy conversation or answer restricted approvals
  on the user's behalf. Existing dev delegation depth rules remain intact.
- OpenCode permission overlays preserve inherited path/agent denials for both
  protocol versions. Native Plan and approval behavior stays owned by dev.
- `/plan`, `/permissions` and `/approvals` open shared client controls; Codex's
  hidden `/plan` command no longer maintains a separate collaboration setting.
  Inline arguments are not a separate way to change the selected policy.

These are provider permission controls, not a universal OS sandbox. Real
provider enforcement remains dependent on each native protocol implementation.

## Validation

Regression coverage targets delayed registry delivery, queued snapshots, active
turn boundaries, frozen MCP delegation, mobile field updates and OpenCode denial
preservation. Mobile builds, binding checks and UI captures run only in CI.
No mobile SDK installation or local mobile compilation is required.
