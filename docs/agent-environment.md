# Agent environment settings

On desktop, open **Settings → Providers**, choose the execution device, and
expand the provider you want to configure. **Environment variables** supports
different proxy settings, API credentials and feature flags for each provider
on each device, including providers that are currently disabled.

1. Select **Add variable**, enter its name and literal value, then **Save**.
   Leaving a new variable's value blank saves an empty string.
2. Use **Edit** to enter a replacement value. **Done** closes the form while
   keeping the draft; **Save** commits pending changes. Opening and closing
   **Edit** without changing the input keeps the saved value.
3. Open a row's **…** menu for **Show value**, **Hide value**, **Unset in child**
   and **Restore inheritance**. **Unset in child** removes the variable from
   the child environment. **Restore inheritance** deletes the override so the
   engine's inherited value can apply again. Save these changes to apply them.
4. Use **Discard** to discard pending changes. Save or discard before closing
   the editor or switching devices: drafts are kept only in the open editor.
   If another editor changed the settings, review the preserved draft against
   the refreshed metadata and save again. If a save acknowledgement is lost,
   open the section's **…** menu and select **Reload status** before explicitly
   retrying; the app does not automatically replay the write.

For example, configure different `HTTPS_PROXY` values for Grok and Devin.
Each provider CLI determines which variables it recognizes. A configured `PATH`
replaces the child's composed PATH; it does not change how the engine finds
the agent CLI.
Credential variables can take precedence over a provider's saved CLI account.
The account usage section continues to describe that saved account.

Values are literal strings: `$HOME`, backticks and shell expressions are not
expanded. Values use a single-line input and are masked in the editor.
**Show value** fetches one saved value; hiding it, closing the editor, changing
devices or replacing the engine connection clears the revealed value.
Provider output and diagnostics are not filtered by matching configured values.

The engine validates names and size limits when saving. Names must match
`[A-Za-z_][A-Za-z0-9_]*` and fit within 128 bytes. Values must contain no NUL
and fit within 16 KiB. Each provider allows at most 64 entries and 64 KiB of
JSON-encoded entries. Identity/configuration directories and engine process
controls are reserved; the editor reports the
reason if you try to change one. For example, `HOME` and `CODEX_HOME` retain
their existing application-level configuration.

Saved settings apply to provider runs, native resume, title generation,
provider discovery and provider-owned login subprocesses. Active turns,
steering, input answers, subagents and voice calls keep their captured
configuration. In a session still using an earlier revision, the next ordinary
send waits for that runtime to finish its turn, pending work and voice activity
before starting with the latest settings. Saving does not interrupt the active
task or wait for other sessions to finish.

Settings belong to the execution device. Editing a remote device requires
that device to be online and support environment settings; a failed remote
save never falls back to the local device. The host stores these settings as
plaintext JSON protected by operating-system file permissions under its
application data directory. The configuration file is shared across that
device's workspace profiles and is excluded from conversation and workspace
sync. Mobile can use agents on configured hosts.
