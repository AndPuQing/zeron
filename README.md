# Zerun

Control your coding agents (Claude Code, Codex, GitHub Copilot, Cursor, Devin, Grok, Hermes, Pi, OpenCode, Antigravity, DeepSeek Harness) locally by default, with optional multi-device sync.

*English | [简体中文](README.zh-CN.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | [Español](README.es.md)*

![Zerun desktop app](docs/media/readme/app-screenshot.jpg)

## Desktop app

Download the latest release for your platform from [GitHub Releases](https://github.com/AndPuQing/zeron/releases/latest):

- **macOS** — `zerun-<version>-macos-arm64.dmg`
- **Windows** — `zerun-<version>-windows-x86_64-setup.exe`
- **Linux** — `zerun-<version>-linux-<arch>.tar.gz`, then run its `install.sh`

No account or network connection is needed; sessions stay on your device. The app updates itself.

Desktop installs use the name **Zerun** and the `zerun` executable. They can
coexist with Zeron: app data lives in `~/.zerun` on macOS/Linux and
`%LOCALAPPDATA%\Zerun` on Windows. `ZERUN_DATA_DIR` overrides this location.
The default local IPC port is 27655 (`ZERUN_IPC_PORT`); the sign-in callback
port is 27642 (`ZERUN_CALLBACK_PORT`).

An optional [experimental desktop](apps/zgx/README.md) can be built with
`scripts/experimental-desktop.sh`. It runs in a separate local profile and
requires its matching engine; stable downloads keep the existing desktop.

## Android

Download `zerun-<version>-android.apk` from [GitHub Releases](https://github.com/AndPuQing/zeron/releases/latest).
The signed release APK supports Android 10 and later on arm64. Open the demo
without an account, or sign in to follow your synced sessions. Settings →
**Check for updates** downloads a verified release and opens Android's installer.
See [Android setup and updates](apps/android/README.md).

## Import existing sessions

Choose **Import** on Sessions to bring in saved Codex, Claude Code, GitHub Copilot,
Cursor, Grok, Hermes, DeepSeek Harness, Pi, or OpenCode conversations from an execution device.
Select sessions under **Not imported**; copies appear in **Archived** and can continue with their
original harness. Existing source histories stay unchanged. See
[native format support](crates/harness/README.md#saved-session-imports).

## Headless (CLI)

For machines that run agents without a desktop window, such as a Linux VPS or an Apple Silicon Mac. The CLI uses the same engine as the desktop app:

```bash
curl -fsSL https://edge.550w.host/install.sh | sh
zerun status
```

On Linux, the installer starts a systemd user service that survives reboots.
On macOS, it installs a standalone signed CLI and starts a user LaunchAgent:
the engine starts at login, runs with the screen locked, and stops at logout.
An SSH-only Mac without a GUI login can run `zerun headless` directly, for
example inside tmux; automatic startup before login is not included.
Intel Mac binaries are not currently published.

Already have the macOS desktop app? Its executable also supports headless mode:

```bash
/Applications/Zerun.app/Contents/MacOS/zerun headless
```

Use one engine per data directory and IPC port. An already-running desktop
background engine can serve remote devices; do not start another engine for the
same profile. The standalone CLI install uses `~/.zerun/app/current` for updates
and does not replace `Zerun.app`. On macOS, export any `ZERON_*`/`ZERUN_*` overrides
before `zerun daemon install`; the service captures its supported settings and
PATH so installed coding-agent CLIs remain discoverable.

```bash
zerun status      # local/synced mode and engine status
zerun update      # update to the latest release
zerun daemon start|stop|restart|status
```

## Multi-device sync (optional)

Sign in to start an agent on one device and follow or drive it from another:

```bash
zerun daemon stop
zerun login        # or: zerun logout to return to local-only
zerun daemon start
```

Devices signed in to the same account can read and write each other's workspace files, so only sign in devices you trust. Existing local sessions are never uploaded.

## Sponsors

Thank you to [The Context Company](https://www.thecontextcompany.com/) for sponsoring Zeron. You can help fund Zeron's development too by [becoming a sponsor on GitHub](https://github.com/sponsors/zeronsh).

---

Developing or curious how it works? [Ask DeepWiki](https://deepwiki.com/zeronsh/zeron) or check out [ARCHITECTURE.md](ARCHITECTURE.md).

Licensed under the [MIT License](LICENSE).
