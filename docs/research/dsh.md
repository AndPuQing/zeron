# DeepSeek Harness (`dsh`) over ACP (2026-10)

## Decision

- Register **DeepSeek Harness** (`HarnessId::Dsh`, display name "DeepSeek
  Harness") in the shared `AcpHarness` family. zeron boots
  `dsh --profile acp-plus`, falling back to the shipped `acp` profile until the
  extended bundle is installed.
- `acp-plus` is the user-maintained, out-of-tree package
  [`dsh-acp-plus`](https://github.com/AndPuQing/dsh-acp-plus), installed with
  `dsh plugin --profile acp-plus add github:AndPuQing/dsh-acp-plus`. The
  shipped `acp` profile lacks `session/load`, steering, and elicitation;
  acp-plus adds them.
- **No managed install.** zeron never installs or updates dsh or its profiles:
  the Settings row keeps `can_install = false` and offers no Install action.
  It shows the documented npm hint and — notify-only — whether npm's `latest`
  dist-tag is newer than the installed binary. Updating is the user's call.
- Wire-first model catalog: models come from the session's advertised config
  options at runtime; no static dsh list is compiled in.

## Verified wire facts (dsh 0.2.0-rc.2, 2026-10)

- `dsh --version` → `0.2.0-rc.2`; npm dist-tags at the time were
  `latest = 0.2.0-rc.2` and `alpha = 0.2.1-alpha.1`.
- `$DSH_HOME` (default `~/.dsh`) holds `.credentials.yaml`,
  `cordis.patch.yml`, and `profiles/<name>/{package.json,cordis.patch.yml}`.
  `dsh <name>` abbreviates `--profile <name>`; the shipped `acp` profile
  auto-initializes on first use.
- dsh-acp-plus advertises the mid-turn steering extension in the `initialize`
  result via `_meta.steering` (`{supported, idleBehavior}`) with no client
  capability gate — the existing `steering_supported()` runtime check picks it
  up unchanged; steering mode is step-boundary.
- Model options arrive as ACP `SessionConfigSelectGroup` entries (provider
  groups), and each value is the raw provider-scoped JSON tuple dsh persists,
  e.g. `"[\"deepseek-official\",\"deepseek-v4-pro\"]"`.
- Reasoning is a flat `thought_level` select with values `off/low/high/max`.
- Elicitation (questions beyond permission prompts) is gated on
  `clientCapabilities.elicitation.form` — deferred to Phase 2.

## P0: grouped select options

ACP lets a `select` advertise `options` as *groups*
(`{group, name, options: [{value, name}]}`). zeron previously read only the flat
shape, so dsh's model select parsed to nothing: the picker came up empty and
`validate_config_model_selection` hard-failed every run. `select_choices()`
(`crates/harness/src/acp/mod.rs`) now flattens groups at every consumer — the
effort-ladder derivation, `model_select`, `trait_from_config_option`,
`validate_config_model_selection`, `config_option_sets`, and
`effort_variant_id` (the effort-in-model-id run path). Any grouped ACP agent
would have hit this, not just dsh.

## Mapping

| zeron | dsh wire |
| --- | --- |
| Model id | The exact advertised tuple, persisted verbatim and re-sent on `session/set_config_option` (exact-match round-trip; the family fallback never matches a tuple). |
| `ReasoningLevel::Minimal` … `Max` | `off` / `low` / `high` / `max`; `off` and `none` parse back as Minimal. Saved levels clamp against the advertised ladder (`dsh_effort_values`). |
| Steering | `_session/steering` at step boundaries when advertised (`_meta.steering`); zeron's own step-boundary queue otherwise. |
| Skills | `$DSH_HOME/skills` and `$DSH_AGENTS_HOME` (default `~/.agents/skills`) user-global; `.agents/skills` and `.dsh/skills` project-side. |
| Model context files | `DSH_HOME` (default `~/.dsh`): `.credentials.yaml`, `cordis.patch.yml`, `profiles/<profile>/package.json`, `profiles/<profile>/cordis.patch.yml`. |
| Executable | `dsh` from PATH, the login shell's PATH, `~/.bun/bin`, `~/.local/bin`, `~/.npm-global/bin`, `/opt/homebrew/bin`, `/usr/local/bin`; `DSH_EXECUTABLE` overrides. |

Profile resolution: `DSH_PROFILE` (explicit override) →
`$DSH_HOME/profiles/acp-plus` exists → `acp-plus` → shipped `acp`.

## Phase 1 (this change)

- `HarnessId::Dsh`, registry descriptor, and `AcpHarness::dsh()` spec
  (steering, ladder, skills, install paths, install hint).
- Grouped-select flattening (the P0 above).
- Model-context files, skill dirs, harness-update row (npm `latest`,
  notify-only), harness slug for the accounts plumbing. No sign-in surface:
  dsh authenticates itself and reports `authMethods: []`.
- UI: picker icon (official DeepSeek mark; desktop SVG and iOS `DshMark`),
  settings row and install hint, skill-completion list, update labels.
- Tests: unit (`dsh_profile_and_effort_ladder`,
  `dsh_provider_grouped_model_options_flow_through`) and integration against
  `crates/harness/tests/fixtures/fake-dsh-acp.sh` — grouped discovery, tuple
  round-trip, Minimal → `off`, descriptor surface.

## Phase 2 (deferred)

- Client capability `elicitation.form` so dsh-acp-plus questions ride
  `requestInput` instead of being declined.
- Friendlier model labels/ids for the raw provider tuples in the picker and
  transcripts.
- Plan-mode surfacing for dsh's plan updates.
- A read-only dsh row in Accounts/usage surfaces, if requested.
- Turn-end hardening beyond acp-plus's own `session/prompt` advertisement.
