# Agent environment settings

Open **Settings → Providers**, choose the execution device, and expand the
provider you want to configure. **Environment variables** supports different
proxy settings, API credentials and feature flags for each provider on each
device, including providers that are currently disabled.

1. Select **Add variable**, enter its name and literal value, then **Save**.
2. Use **Replace** to change an existing value. **Show value** explicitly
   fetches that one saved value; **Hide value**, closing the editor or changing
   devices clears it from the editor.
3. Use **Remove from child** to omit a variable from the agent's environment.
   Use **Restore inheritance** to delete the override and use the engine's
   inherited value again. Saving an empty value sets an empty string.
4. Use **Cancel** to discard a draft. If another editor changed the settings,
   review the preserved draft against the refreshed metadata and save again.
   If a save acknowledgement is lost, **Reload status** before explicitly
   retrying; the app does not automatically replay the write.

For example, set `HTTPS_PROXY` independently for Grok and Devin to send their
agent traffic through different proxies. A configured `PATH` replaces the
child's composed PATH; it does not change how the engine finds the agent CLI.
Credential variables can take precedence over a provider's saved CLI account.
The account usage section continues to describe that saved account.

Values are literal strings: `$HOME`, backticks and shell expressions are not
expanded. **Edit multiline (visible)** preserves line breaks and shows the
replacement while editing. Values otherwise remain masked. **Sensitive: on**
removes known values from agent launch, discovery and login diagnostics.

The engine validates names and size limits when saving. Names use letters,
digits and underscores and cannot begin with a digit. Identity/configuration
directories and engine process controls are reserved; the editor reports the
reason if you try to change one. For example, `HOME` and `CODEX_HOME` retain
their existing application-level configuration.

Saved settings apply to provider runs, native resume, title generation,
provider discovery and provider-owned login subprocesses. Active turns,
steering, input answers, subagents and voice calls keep their captured
configuration. The next ordinary send waits for a safe boundary and starts
with the latest settings. Saving does not interrupt the active task.

Settings belong to the execution device. Editing a remote device requires
that device to be online and support environment settings; a failed remote
save never falls back to the local device. The host stores these settings
privately under its application data directory. They are shared across that
device's workspace profiles and are not copied into conversations or sync
documents. The initial editor is available on desktop; mobile can use agents
on configured hosts.
