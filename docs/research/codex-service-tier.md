# Codex Fast off and native tier inheritance

Verified locally on 2026-10-28 with `codex-cli 0.165.0`. The installed Zerun
bundle reports 0.3.24 (binaries dated Oct 27); the inspected source was based
on `9d18fecc`. The installed bundle was not replaced or restarted during this
investigation; the version number alone does not identify its source commit.

## Root cause and data flow

The compact lightning button uses `compact_fast_choice()` and `pick_option()`.
Previously, selecting the catalog default removed the entry from `modelOptions`.
`run_model_options()` materialized the catalog default again for interactive
sends, but Codex's `service_tier()` filtered out `default`. Both paths therefore
omitted `serviceTier` from `thread/start`, `thread/resume`, and `turn/start`.
Omission retained native `priority`, including after restarting the app-server.

An explicit Codex choice now persists in both composer defaults and ChatConfig:

| Stored `modelOptions` | Meaning | app-server `serviceTier` |
| --- | --- | --- |
| no `serviceTier` | inherit native/global settings | omitted |
| `{"serviceTier":"fast"}` | Fast | `"fast"` |
| `{"serviceTier":"default"}` | explicitly Standard | `"default"` |

Existing chats flow through `update_chat_config()` → `setChatConfig` → workspace
snapshot; drafts use composer defaults. `composer.rs` sends `run_model_options()`
in RunRequest. Engine-generated requests also clone ChatConfig.model_options.
`RuntimeConfig::route()` now allows Fast → explicit Standard reconfiguration in
place. The Codex adapter sends the explicit tier at thread setup and on every
turn, including ordinary follow-ups and steered requests that start a new turn.
An already-running turn is not interrupted or retroactively repriced.

Other providers retain their previous default-selection behavior. Model,
reasoning, access policy, and sandbox handling are unchanged.

## Actual protocol and wire behavior

Generate the exact installed version's schema with:

```sh
codex --version
codex app-server generate-json-schema --experimental --out /tmp/codex-tier-schema
python3 scripts/codex-service-tier-probe.py
```

`ThreadStartParams`, `ThreadResumeParams`, and `TurnStartParams` allow an optional
string/null `serviceTier`. The schema alone does not specify the clearing
semantics. `TurnStartParams.serviceTier` persists for subsequent turns; the
separate `serviceTierForTurn` has different null semantics and is not used here.

The probe runs the actual binary with a temporary CODEX_HOME, a global
`service_tier = "fast"`, and a loopback Responses endpoint. It observes the HTTP
request bodies, not just returned settings or UI state. No user session is
resumed and no cloud model request is made.

| Parameter | New thread / cold resume | turn/start with native priority |
| --- | --- | --- |
| omitted | inherits global/native priority | retains priority |
| null | clears tier | clears tier |
| `"default"` | clears tier | clears tier |
| `"fast"` | priority | priority |
| `"priority"` | priority | priority |

Clearing emits an HTTP Responses request with **no `service_tier` field** (normal
service), while Fast emits `service_tier: "priority"`. A `thread/resume` against
an already-loaded thread can ignore overrides; explicit `turn/start` is required
and verified. This is why resetting only the process or thread setup is not a fix.
The committed probe asserts these version-specific behaviors and fails if a
future installed CLI differs.

## Historical configurations and recovery

`modelOptions: {}` is ambiguous: it was created by Fast-off, but also by untouched
chats, imported sessions and provider/model changes. No automatic migration or
session JSONL/database rewrite is performed. Unspecified Codex tiers are no
longer filled from the display catalog, preserving inheritance consistently
across interactive sends, engine-generated requests and restarts.

After installing the fixed build, explicitly toggle Fast on then off in an old
chat whose icon already looks off. This records `serviceTier: "default"`; the
next new turn clears native priority, even if the global config still says Fast.
No conversation reset or deletion is needed. The existing two-state UI is kept;
an untouched legacy off-looking icon is not evidence that native inheritance is
Standard. Existing in-flight turns finish under their original settings.

## Regression coverage

- UI: actual lightning choices for drafts and saved chats, explicit Standard
  surviving serialization/catalog loss, and a Fast catalog default not replacing it.
- Workspace: native snapshot export/import preserves inheritance, Fast, Standard.
- Engine: Fast → Standard routes a configuration update to the existing runtime.
- Harness: stateful subprocess fixture checks start/resume/turn parameters,
  Fast → off → follow-up, native priority recovery and restart, and inherited Fast.
- Installed Codex: the loopback probe checks actual HTTP service tiers for new,
  resumed, already-loaded and restarted threads with a conflicting global default.

Initial verification results on 2026-10-28 (before rebasing onto dev):

```text
cargo test -p zeron-harness --test codex                 51 passed, 4 existing ignored
cargo test -p zeron-ui --lib fast                        3 passed
cargo test -p zeron-doc codex_explicit_standard          1 passed
cargo test -p zeron-engine --lib codex_fast_to_standard   1 passed
python3 scripts/codex-service-tier-probe.py              29 HTTP-tier assertions passed
```

Implementation files are `crates/ui/src/pickers.rs` and
`crates/harness/src/codex/mod.rs`; `crates/ui/src/composer.rs` updates the send-path
comment. Regression tests live in the picker module, `crates/doc/src/workspace.rs`,
`crates/engine/src/sessions.rs`, and `crates/harness/tests/codex.rs`, with the
stateful peer in `crates/harness/tests/fixtures/codex-service-tier.py`.

The local offline app build needs `ORT_LIB_PATH` pointing to the cached ONNX
Runtime archive. Its cache key `6934874e…79674c7` matches ort-sys 2.0.0-rc.13's
`aarch64-apple-darwin` CoreML distribution (ONNX Runtime 1.28.0). This is a build
environment setting only; no voice implementation or dependency was changed.

The Dev bundle was built successfully with `scripts/run-macos-dev.sh`
(`ZERON_DEV_BUILD_ONLY=1`) and passed `codesign --verify --deep --strict`.
Before invoking the script, unset inherited `ZERUN_DATA_DIR` and `ZERUN_IPC_PORT`
in the build subprocess so they do not collide with its isolated defaults.
The artifact is `target/macos-dev/Zerun Dev.app` in the fix worktree, with its own
data directory and IPC port 49777. It was not launched. The installed production
app and its running engine remain unchanged and do not yet contain this fix.

After rebasing the isolated fix onto dev `9d18fecc`, verification passed again:
Codex harness 53 passed / 4 existing ignored, UI Fast tests 3 passed, workspace
snapshot 1 passed, engine routing 1 passed. To fit local disk space, the UI and
engine test reruns disabled debug symbols for the respective package only.
The previous Dev app build predates this rebase and was removed to free build
space; no rebased application bundle was built or installed.
