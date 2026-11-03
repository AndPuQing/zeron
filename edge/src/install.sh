#!/bin/sh
# Zerun (native) headless installer.
#
#   curl -fsSL https://edge.550w.host/install.sh | sh
#
# Installs the native binary (Linux requires the system ALSA runtime) to
# ~/.zerun/app and puts `zerun` on PATH. Linux also gets a launcher entry
# under $XDG_DATA_HOME (default ~/.local/share) and a systemd user service;
# macOS gets a login LaunchAgent. Both start local-only. Signing in is optional
# and enables sync after a restart. Re-running upgrades in place and preserves
# ~/.zerun state.
#
# The binary ships with production endpoints baked in: no ZERON_EDGE_URL or
# client-id configuration needed. Linux overrides go in ~/.zerun/env; on macOS
# export overrides before running `zerun daemon install` to capture them.
set -eu

BASE="${ZERUN_BASE_URL:-https://edge.550w.host}"

# --- platform ---------------------------------------------------------------
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
  Linux) plat=linux ;;
  Darwin) plat=macos ;;
  *)
    echo "zerun install: unsupported OS '$os' — supported platforms are Linux and macOS." >&2
    exit 1
    ;;
esac
case "$plat:$arch" in
  linux:x86_64 | linux:amd64) arch=x86_64 ;;
  linux:aarch64 | linux:arm64) arch=aarch64 ;;
  macos:arm64 | macos:aarch64) arch=arm64 ;;
  macos:x86_64)
    # A Rosetta shell reports x86_64 on an Apple Silicon machine.
    if [ "$(sysctl -n hw.optional.arm64 2>/dev/null || true)" = 1 ]; then
      arch=arm64
    else
      echo "zerun install: macOS release binaries currently require Apple Silicon." >&2
      exit 1
    fi
    ;;
  *)
    echo "zerun install: unsupported architecture '$arch' on $plat." >&2
    exit 1
    ;;
esac

# --- download ----------------------------------------------------------------
ver="$(curl -fsSL "$BASE/releases/latest.txt" | tr -d '[:space:]')"
[ -n "$ver" ] || { echo "zerun install: could not resolve latest version" >&2; exit 1; }
file="zerun-$ver-$plat-$arch.tar.gz"
data_root="$HOME/.zerun"
app_root="$data_root/app"
dest="$app_root/$ver"

if [ -x "$dest/zerun" ]; then
  echo "zerun $ver already downloaded — relinking."
else
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  echo "downloading zerun $ver ($plat-$arch)…"
  curl -fSL --progress-bar "$BASE/releases/$file" -o "$tmp/$file"
  mkdir -p "$dest"
  tar -xzf "$tmp/$file" -C "$dest" --strip-components=1
fi

# Probe before changing a working installation or starting its service. The
# desktop and headless modes share one binary, including CPAL's ALSA linkage.
if ! "$dest/zerun" --version >/dev/null; then
  echo "zerun install: the downloaded executable could not start; see the loader error above." >&2
  if [ "$plat" = linux ]; then
    echo "Install the missing runtime libraries (including ALSA, libasound.so.2), then rerun this installer." >&2
  else
    echo "Check the macOS version, architecture and executable signature, then rerun this installer." >&2
  fi
  exit 1
fi

ln -sfn "$dest" "$app_root/current"
mkdir -p "$HOME/.local/bin"
ln -sf "$app_root/current/zerun" "$HOME/.local/bin/zerun"

# --- desktop entry -------------------------------------------------------------
# Launchers list Zerun through a per-user .desktop entry. The one in the tarball
# says `Exec=zerun` and `TryExec=zerun`, which only resolve when ~/.local/bin is
# on the PATH of the desktop session (often not, e.g. a bare Wayland + fuzzel
# setup) and TryExec then hides the entry outright. So write it with absolute
# paths through the `current` symlink, which keeps working across updates. The
# icon is referenced by path too: the only artwork is 1024x1024, a size the
# hicolor theme doesn't index, so a name lookup alone can come up empty.
# (Duplicated in scripts/package-linux.sh's install.sh; keep the two in sync.)
install_desktop_entry() {
  src="$1"
  app="$2"
  [ -f "$src/zerun.desktop" ] && [ -f "$src/zerun.png" ] || return 1
  case "${XDG_DATA_HOME:-}" in
    /*) data_home="$XDG_DATA_HOME" ;;
    *) data_home="$HOME/.local/share" ;;
  esac
  apps_dir="$data_home/applications"
  icon_dir="$data_home/icons/hicolor/1024x1024/apps"
  bin="$app/current/zerun"
  icon="$app/current/zerun.png"
  # Desktop Entry `Exec` quoting: double-quote an argument with reserved
  # characters, backslash-escape ", `, $ and \ inside, then double every
  # backslash again for the file's own string escaping. `%` must be `%%`.
  case "$bin" in
    *[!A-Za-z0-9_./-]*)
      exec_bin="\"$(printf '%s' "$bin" | sed -e 's/\\/\\\\\\\\/g' -e 's/["`$]/\\\\&/g' -e 's/%/%%/g')\""
      ;;
    *) exec_bin="$bin" ;;
  esac
  try_bin="$(printf '%s' "$bin" | sed 's/\\/\\\\/g')"
  icon_val="$(printf '%s' "$icon" | sed 's/\\/\\\\/g')"

  mkdir -p "$apps_dir" "$icon_dir" || return 1
  # Write beside the final name, then rename, so a launcher watching the
  # directory never reads a half-written entry (a leading dot is ignored).
  # Not `tmp`: sh has no `local`, and the curl installer's EXIT trap removes
  # its download dir through `$tmp`.
  entry_tmp="$apps_dir/.zerun.desktop.$$"
  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
      Exec=*) printf 'Exec=%s %%u\n' "$exec_bin" ;;
      TryExec=*) printf 'TryExec=%s\n' "$try_bin" ;;
      Icon=*) printf 'Icon=%s\n' "$icon_val" ;;
      *) printf '%s\n' "$line" ;;
    esac
  done <"$src/zerun.desktop" >"$entry_tmp" || { rm -f "$entry_tmp"; return 1; }
  mv -f "$entry_tmp" "$apps_dir/zerun.desktop" || { rm -f "$entry_tmp"; return 1; }
  cp "$src/zerun.png" "$icon_dir/.zerun.png.$$" \
    && mv -f "$icon_dir/.zerun.png.$$" "$icon_dir/zerun.png" || return 1

  # Best-effort cache refresh; both tools are optional. The icon cache is only
  # refreshed, never created: a user-level hicolor cache nobody else maintains
  # would hide icons other apps later install there, and the entry above
  # references the icon by path anyway.
  command -v update-desktop-database >/dev/null 2>&1 \
    && update-desktop-database "$apps_dir" >/dev/null 2>&1 || true
  [ -f "$data_home/icons/hicolor/icon-theme.cache" ] \
    && command -v gtk-update-icon-cache >/dev/null 2>&1 \
    && gtk-update-icon-cache -q -t -f "$data_home/icons/hicolor" >/dev/null 2>&1 || true
  return 0
}
# A missing desktop entry must never fail an otherwise good install.
if [ "$plat" = linux ]; then
  install_desktop_entry "$dest" "$app_root" \
    || echo "warn: could not install the desktop entry — Zerun won't appear in application launchers"
fi

# --- service -----------------------------------------------------------------
# The daemon is useful before auth: without a saved session it serves the local
# profile. Login only changes which profile the next daemon start selects.

service=manual
if [ "$plat" = macos ]; then
  if launchctl print "gui/$(id -u)" >/dev/null 2>&1; then
    "$app_root/current/zerun" daemon install
    service=launchd
  else
    echo "note: no macOS GUI login session — the CLI is installed, but its LaunchAgent was not started."
    echo "Log in at the Mac, then run: zerun daemon install"
    echo "For an SSH-only session, run zerun headless directly (for example, inside tmux)."
  fi
elif command -v systemctl >/dev/null 2>&1 && [ -n "${XDG_RUNTIME_DIR:-}" ]; then
  mkdir -p "$HOME/.config/systemd/user"
  cat >"$HOME/.config/systemd/user/zerun.service" <<'UNIT'
[Unit]
Description=Zerun native headless engine
After=network-online.target
StartLimitIntervalSec=60
StartLimitBurst=5

[Service]
ExecStart=%h/.zerun/app/current/zerun headless
Restart=on-failure
RestartSec=5
EnvironmentFile=-%h/.zerun/env

[Install]
WantedBy=default.target
UNIT
  systemctl --user daemon-reload
  systemctl --user enable zerun
  systemctl --user restart zerun
  service=running
  # Keep the user manager (and the engine) running without an active login.
  loginctl enable-linger "$USER" 2>/dev/null \
    || sudo -n loginctl enable-linger "$USER" 2>/dev/null \
    || echo "warn: could not enable linger — the engine stops when you log out (run: sudo loginctl enable-linger $USER)"
else
  echo "warn: systemd user session not available — run the engine manually with: zerun headless"
fi

# --- agent CLIs ---------------------------------------------------------------
command -v claude >/dev/null 2>&1 || \
  echo "note: Claude Code CLI not found — install it with: curl -fsSL https://claude.ai/install.sh | bash"

case ":$PATH:" in
  *":$HOME/.local/bin:"*) path_hint="" ;;
  *) path_hint=' (add ~/.local/bin to your PATH)' ;;
esac

echo ""
echo "✓ zerun $ver installed$path_hint"
echo ""
case "$service" in
  launchd)
    echo "the engine is running without a UI (local-only unless sync is enabled)."
    echo "  zerun daemon status              check the service"
    echo "  zerun daemon stop|start|restart   manage the service"
    echo "  zerun update                     update the CLI and restart its service"
    echo "The service starts at login and stops when you log out; locking the screen is fine."
    echo "Optional sync: zerun daemon stop; zerun login; zerun daemon start"
    ;;
  running)
    echo "the engine is running with the new version (local-only unless sync is enabled)."
    echo "  systemctl --user status zerun    check the service"
    echo ""
    echo "optional sync (local sessions stay local):"
    echo "  systemctl --user stop zerun"
    echo "  zerun login"
    echo "  systemctl --user restart zerun"
    ;;
  manual)
    echo "next: run the local-only engine with \`zerun headless\`."
    echo "optional sync: run \`zerun login\` before starting the engine."
    ;;
esac
