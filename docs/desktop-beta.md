# Desktop Beta

The desktop Beta packages the experimental interface with an engine built
from the same commit. It replaces the installed Zerun desktop and keeps its
account, device identity and conversations. Source development previews
remain separate.

Download a Beta from [Zerun Releases](https://github.com/AndPuQing/zeron/releases?q=beta&expanded=true):

- macOS 13 or later, Apple Silicon: open the DMG and move Zerun to Applications.
- Linux x86_64 or aarch64: extract the tarball and run its `install.sh`.
  Packages are built on Ubuntu 24.04; compatible desktop runtime libraries
  are required. Browser tabs use WebKitGTK 4.1 when available.
- Windows x86_64: run the per-user installer, or extract the entire portable
  ZIP. Keep its `beta` folder and `zerun-update.json` beside `zerun.exe`.

The first launch enables Beta updates. Settings → **About & updates** shows
available updates and **Restart to update**. A downloaded update also installs
when the app quits. To return to stable, turn off **Get beta updates**. Zerun
waits until stable's engine catches up before switching, so this choice may
not immediately replace the app. On Windows, update a Beta from this page;
remote engine update transactions are available for stable packages.

Beta releases are GitHub prereleases and use a separate update feed at
`https://edge.550w.host/releases/beta/manifest.json`. The normal installer,
stable download links and headless updates keep their stable default.

## Building and publishing

The **Desktop Beta** workflow can build selected platforms without publishing.
Set its version to `<workspace version>-beta.<number>`. For publication, push
that version's `beta-v<version>` tag. The workflow builds all platforms, checks
their contents, signs and notarizes macOS packages when signing is configured,
then publishes a GitHub prerelease. It uploads the Beta manifest after all
Beta artifacts, without writing the stable manifest or latest-version pointer.

The build scripts require the Beta interface and matching engine versions.
Use `scripts/package-zgx-beta.sh` on macOS, `scripts/package-linux.sh` on Linux,
or `scripts/package-windows.ps1` on Windows. Set `BETA_VERSION` and
`ZGX_BETA_ENGINE` for the shell scripts; use `-BetaVersion` and `-BetaEngine`
with PowerShell. Build engines with `ZERUN_ENGINE_VERSION=<beta version>`.
Development previews still use `scripts/experimental-desktop.sh`.
