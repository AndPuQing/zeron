# DeepSeek Harness (`dsh`) over ACP

Zerun connects to DeepSeek Harness through `AcpHarness::dsh()`. Install the
CLI and extended ACP profile yourself:

```sh
npm install -g @deepseek-ai/dsh
dsh plugin --profile acp-plus add github:AndPuQing/dsh-acp-plus
```

Configure credentials in dsh before selecting **DeepSeek Harness** in Zerun.
Zerun uses dsh's authentication and does not provide a separate sign-in flow.
Settings reports available CLI updates; apply them with
`npm install -g @deepseek-ai/dsh@latest`.

## Profiles and configuration

Zerun starts `dsh --profile acp-plus` when
`$DSH_HOME/profiles/acp-plus` exists, otherwise it starts the bundled `acp`
profile. Set `DSH_PROFILE` to choose a different profile, or `DSH_EXECUTABLE`
to override executable discovery. `DSH_HOME` defaults to `~/.dsh`.

The extended profile provides session loading, mid-turn steering, and form
elicitation. Steering is used at step boundaries when the agent advertises
`_meta.steering`; `ask_user_question` rides ACP `elicitation/create` and
surfaces in Zerun's question wizard. The bundled `acp` profile has fewer
capabilities.

Skills are discovered from `$DSH_HOME/skills`, `$DSH_AGENTS_HOME/skills`
(`DSH_AGENTS_HOME` defaults to `~/.agents`), and the project's `.agents/skills`
and `.dsh/skills` directories.

## Protocol mapping

The model catalog comes from session config options. ACP select options may
be flat or grouped by provider; `select_choices()` flattens them for model
discovery, trait controls, validation, and config updates. Preserve each
advertised model value exactly, including JSON tuples such as
`["deepseek-official","deepseek-v4-pro"]`.

The `thought_level` values `off`, `low`, `high`, and `max` map to
`ReasoningLevel::Minimal`, `Low`, `High`, and `Max`.

Model-discovery cache invalidation tracks `.credentials.yaml` and
`cordis.patch.yml` below `DSH_HOME`, plus `package.json` and `cordis.patch.yml`
in the selected profile directory.

Model, command, and skill discovery creates short-lived ACP sessions. Zerun
adds a process-local `--patch` overlay that redirects session logs and JSON
workspace/projection storage to a private temporary directory and disables
the SQLite session search index. The directory is removed after the probe
exits, including on errors and timeouts. Discovery uses the standard base
profile's storage entry IDs; custom profiles should retain those IDs.
Profiles, credentials, and model settings still come from `DSH_HOME`; real
conversations use the profile's normal persistent storage.

Empty unnamed sessions created by older Zerun versions remain in dsh's
history. Archive confirmed probe sessions in dsh's Web UI; a missing title
alone does not distinguish a probe from a user's unfinished conversation.
