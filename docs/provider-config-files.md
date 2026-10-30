# Provider configuration files

On desktop, open **Settings → Providers**, choose a computer and expand an agent.
The **Configuration files** section shows its user-global settings paths. Use
**Open** for an existing local file or **Copy path** to use its path elsewhere.
Refresh after creating a configuration or changing the provider's environment.
Files are opened with the operating system's default file association.

Remote computers resolve their own paths; only copying the path is offered for
remote files. Cloud's managed accounts have no local configuration shortcut.
Older hosts show an upgrade message. This feature does not add a mobile editor.

Discovery returns labels, absolute paths and existence flags, never file contents.
It does not create files, migrate settings or include dedicated credential stores.
Project-level settings and launch-time overrides can still take precedence over
these user-global settings. Existing processes may need a new session/restart to
pick up edits; Zerun does not restart an active agent automatically.

| Agent | Default files / source |
| --- | --- |
| Claude Code | `~/.claude/settings.json` (`CLAUDE_CONFIG_DIR`) |
| Codex | `~/.codex/config.toml` (`CODEX_HOME`) |
| Copilot | `~/.copilot/config.json` (`COPILOT_HOME`) |
| Grok | `~/.grok/config.toml` (`GROK_HOME`) |
| Hermes | `~/.hermes/config.yaml` (`HERMES_HOME`) |
| fx | `~/.fx/settings.json` |
| Pi | `~/.pi/agent/settings.json`, `models.json` (`PI_CODING_AGENT_DIR`) |
| OMP | `~/.omp/agent/config.yml`, `models.yml` (`PI_CODING_AGENT_DIR`) |
| OpenCode | `$XDG_CONFIG_HOME/opencode/opencode.json` and `.jsonc`, plus `OPENCODE_CONFIG` / `OPENCODE_CONFIG_DIR` overrides |
| Antigravity | `$GEMINI_HOME/antigravity-acp/settings.json`, using the adapter's path expansion and home-anchored launch directory |
| dsh | `$DSH_HOME/cordis.patch.yml` and `profiles/<active ACP profile>/cordis.patch.yml`; profile follows the adapter's `--profile` selection |
| Cursor | `~/.cursor/cli-config.json`; on Linux an explicit `XDG_CONFIG_HOME` selects `cursor/cli-config.json` under it |
| Devin | No verified user configuration file; explicit unavailable explanation |

Unset environment roots use the native defaults. Relative overrides whose
meaning depends on a session's working directory are not guessed. Cursor's file
is its CLI configuration; Zerun uses the SDK and can override individual options.
Cursor path reference: https://cursor.com/docs/cli/reference/configuration . Other
paths follow the native adapters' configuration/catalog/saved-session resolvers.
Muse is intentionally unsupported.
