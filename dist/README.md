# Packaging

The desktop product is Zerun; the Rust workspace package stays `zeron`.
Release builds require the native toolchain and system libraries for each host.
All scripts write artifacts under `target/package/`.

## App icons

The approved orange portrait and its masks live in `dist/artwork`. Generate
the desktop, mobile and web app icons together using Python and Pillow:

```sh
uv run --with pillow==13.0.0 python scripts/make-app-icons.py
uv run --with pillow==13.0.0 python scripts/make-app-icons.py --check
```

The generator keeps the macOS tile's existing alpha mask and shadow, produces
all Windows ICO sizes, and supplies square opaque iOS light, dark and grayscale
tinted variants. Android gets density-specific adaptive portrait and monochrome
layers within the launcher safe region; the launcher owns the final shape.
The website's light/dark favicons and Apple touch icon share these assets.
No model request or mobile SDK is needed to regenerate the icons.

## Linux

```sh
scripts/package-linux.sh
PROFILE=debug scripts/package-linux.sh
```

Produces `zerun-<version>-linux-<arch>.tar.gz` with the `zerun` executable,
desktop entry, icon, licenses, and `install.sh`. Installation uses
`~/.zerun/app/<version>` behind a `current` symlink, links
`~/.local/bin/zerun`, and writes `zerun.desktop` and `zerun.png` under
`$XDG_DATA_HOME` (default `~/.local/share`). Launcher paths follow `current`
across updates. The curl installer also manages the `zerun.service` user unit.

## macOS

```sh
scripts/package-macos.sh
```

Produces `zerun-<version>-macos-<arch>.dmg` and
`zerun-<version>-macos-<arch>-app.tar.gz`. Both contain `Zerun.app`, with
bundle identifier `work.puqing.zerun` and executable `Contents/MacOS/zerun`.
The tarball is the in-app update payload. Install the app in Applications.

Set `CODESIGN_IDENTITY` to a Developer ID Application identity to sign it;
otherwise the script signs ad hoc. Set `NOTARY_KEY_PATH`, `NOTARY_KEY_ID`, and
`NOTARY_ISSUER_ID` for notarization and stapling. These require an Apple
Developer account. Build on macOS; GPUI requires Metal.

## Windows

Install Inno Setup 6, then run:

```powershell
./scripts/package-windows.ps1 -ReleasesUrl https://github.com/AndPuQing/zeron/releases/latest/download
```

Produces `zerun-<version>-windows-<arch>-setup.exe`, a portable `.zip`, and a
bare `.exe` for updates. The installer uses `%LOCALAPPDATA%\Programs\Zerun`,
a dedicated uninstall AppId, a Zerun Start menu entry, and the `zerun-dev://`
link handler. User data lives separately in `%LOCALAPPDATA%\Zerun`.

The installer and portable ZIP include `zerun-update.json`, identifying
`work.puqing.zerun` and its release feed. Keep it beside `zerun.exe` to enable
in-app updates. See [Windows development](../docs/reference/windows-development.md)
for toolchain requirements and packaging checks.
