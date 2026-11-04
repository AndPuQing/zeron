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
# manifest.json names the release and the sha256 of each artifact, exactly as
# the built-in updater reads it; the download is never used without a match.
# Whitespace is dropped first so the lookups do not depend on its layout.
manifest="$(curl -fsSL "$BASE/releases/manifest.json")" \
  || { echo "zerun install: could not fetch the release manifest ($BASE/releases/manifest.json)" >&2; exit 1; }
manifest="$(printf '%s' "$manifest" | tr -d '[:space:]')"
ver="$(printf '%s' "$manifest" | sed -n 's/.*"version":"\([^"]*\)".*/\1/p')"
case "$ver" in
  '' | *[!0-9.]*) echo "zerun install: could not resolve the latest version from the release manifest" >&2; exit 1 ;;
esac
file="zerun-$ver-$plat-$arch.tar.gz"
expected_sha="$(printf '%s' "$manifest" | awk -v key="\"$file\":{\"sha256\":\"" '{
  i = index($0, key)
  if (i) { rest = substr($0, i + length(key)); print substr(rest, 1, index(rest, "\"") - 1) }
}' | tr 'A-F' 'a-f')"
case "$expected_sha" in
  *[!0-9a-f]* | '') expected_sha="" ;;
esac
if [ "${#expected_sha}" -ne 64 ]; then
  echo "zerun install: the release manifest has no sha256 for $file — refusing to install it." >&2
  exit 1
fi
sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1"
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1"
  fi | awk '{ print $1 }' | tr 'A-F' 'a-f'
}
data_root="$HOME/.zerun"
app_root="$data_root/app"
dest="$app_root/$ver"

# Probe before changing a working installation or starting its service. The
# desktop and headless modes share one binary, including CPAL's ALSA linkage.
verify_release() {
  if [ "$plat" = macos ] && ! codesign --verify --strict "$1/zerun"; then
    echo "zerun install: the executable's code signature is missing or invalid — refusing to install it." >&2
    return 1
  fi
  if ! "$1/zerun" --version >/dev/null; then
    echo "zerun install: the downloaded executable could not start; see the loader error above." >&2
    if [ "$plat" = linux ]; then
      echo "Install the missing runtime libraries (including ALSA, libasound.so.2), then rerun this installer." >&2
    else
      echo "Check the macOS version, architecture and executable signature, then rerun this installer." >&2
    fi
    return 1
  fi
}

# A version directory only ever holds a release that passed verify_release:
# the download is unpacked and checked beside it, then renamed into place.
# (BSD rm rejects an empty operand, so only name directories that exist.)
stage=""
cleanup() {
  [ -z "${tmp:-}" ] || rm -rf "$tmp"
  [ -z "$stage" ] || rm -rf "$stage"
}
trap cleanup EXIT
if [ -x "$dest/zerun" ] && verify_release "$dest" >/dev/null 2>&1; then
  echo "zerun $ver already downloaded — relinking."
else
  [ ! -e "$dest" ] || echo "zerun $ver: the existing copy failed its check — downloading it again."
  tmp="$(mktemp -d)"
  echo "downloading zerun $ver ($plat-$arch)…"
  curl -fSL --progress-bar "$BASE/releases/$file" -o "$tmp/$file"
  actual_sha="$(sha256_file "$tmp/$file")"
  if [ -z "$actual_sha" ]; then
    echo "zerun install: neither sha256sum nor shasum is available to verify the download." >&2
    exit 1
  fi
  if [ "$actual_sha" != "$expected_sha" ]; then
    echo "zerun install: checksum mismatch for $file (expected $expected_sha, got $actual_sha) — refusing to install it." >&2
    exit 1
  fi
  mkdir -p "$app_root"
  stage="$(mktemp -d "$app_root/.stage-$ver.XXXXXX")"
  tar -xzf "$tmp/$file" -C "$stage" --strip-components=1
  verify_release "$stage" || exit 1
  rm -rf "$dest"
  mv "$stage" "$dest"
  stage=""
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
