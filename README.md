# Zerun

Control your coding agents (Claude Code, Codex, Cursor, Devin, Grok, Hermes, Pi, Antigravity) locally by default, with optional multi-device sync.

*English | [简体中文](README.zh-CN.md) | [한국어](README.ko.md) | [日本語](README.ja.md)*

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

## Android

Download `zerun-<version>-android.apk` from [GitHub Releases](https://github.com/AndPuQing/zeron/releases/latest).
The signed APK supports Android 10 and later on arm64 and x86_64. Open the demo
without an account, or sign in to follow your synced sessions. Settings →
**Check for updates** downloads a verified release and opens Android's installer.
See [Android setup and updates](apps/android/README.md).

## Headless (CLI)

For servers and other machines without a display, such as a VPS that keeps agents running after you close your laptop. Linux only:

```bash
curl -fsSL https://edge.550w.host/install.sh | sh
zerun status
```

The installer starts the engine as a background service that survives reboots.

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
