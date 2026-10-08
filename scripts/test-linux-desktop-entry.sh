#!/usr/bin/env bash
# Offline check that both Linux installers (the curl one in edge/src/install.sh
# and the install.sh scripts/package-linux.sh puts in the tarball) write a
# launcher entry with absolute paths plus the icon, idempotently, under a
# throwaway HOME. Nothing touches the network, systemd, or the real home.
#
# Usage: scripts/test-linux-desktop-entry.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
VERSION=9.9.9
fail() { echo "FAIL: $*" >&2; exit 1; }

# A fake release: the tarball layout package-linux.sh produces, minus the real
# binary. `uname` is shimmed so the curl installer also runs on a macOS dev box.
PKG="zerun-$VERSION-linux-x86_64"
mkdir -p "$WORK/site/releases" "$WORK/pkg/$PKG" "$WORK/shim"
printf '#!/bin/sh\nexit 0\n' >"$WORK/pkg/$PKG/zerun"
chmod 755 "$WORK/pkg/$PKG/zerun"
cp "$ROOT/dist/zeron.desktop" "$WORK/pkg/$PKG/zerun.desktop"
printf 'not-really-a-png' >"$WORK/pkg/$PKG/zerun.png"
echo "$VERSION" >"$WORK/site/releases/latest.txt"
tar -czf "$WORK/site/releases/$PKG.tar.gz" -C "$WORK/pkg" "$PKG"
printf '#!/bin/sh\ncase "$1" in -s) echo Linux ;; -m) echo x86_64 ;; *) exec /usr/bin/uname "$@" ;; esac\n' >"$WORK/shim/uname"
chmod 755 "$WORK/shim/uname"
cat >"$WORK/shim/systemctl" <<'SH'
#!/bin/sh
printf '%s\n' "$*" >>"$HOME/systemctl.log"
SH
printf '#!/bin/sh\nexit 0\n' >"$WORK/shim/loginctl"
chmod 755 "$WORK/shim/systemctl" "$WORK/shim/loginctl"

# The tarball's install.sh lives in a heredoc inside package-linux.sh.
sed -n "/<<'INSTALL'/,/^INSTALL\$/p" "$ROOT/scripts/package-linux.sh" | sed '1d;$d' \
  | sed "s/__VERSION__/$VERSION/" >"$WORK/pkg/$PKG/install.sh"
chmod 755 "$WORK/pkg/$PKG/install.sh"

# The two copies of install_desktop_entry must stay identical.
fn_body() { sed -n '/^install_desktop_entry() {$/,/^}$/p' "$1"; }
[ -n "$(fn_body "$ROOT/edge/src/install.sh")" ] || fail "install_desktop_entry not found"
[ "$(fn_body "$ROOT/edge/src/install.sh")" = "$(fn_body "$ROOT/scripts/package-linux.sh")" ] \
  || fail "install_desktop_entry differs between edge/src/install.sh and scripts/package-linux.sh"

# run_curl HOME [ENV=VALUE ...] / run_tarball HOME [ENV=VALUE ...]
# Each run gets its own TMPDIR, which must be empty again afterwards (the curl
# installer's EXIT trap removes its download dir).
mkdir -p "$WORK/tmp"
run_curl() {
  local home="$1"; shift
  env -i HOME="$home" USER=tester PATH="$WORK/shim:/usr/bin:/bin" TMPDIR="$WORK/tmp" "$@" \
    ZERUN_BASE_URL="file://$WORK/site" sh "$ROOT/edge/src/install.sh" >"$WORK/out.log" 2>&1 \
    || { cat "$WORK/out.log" >&2; fail "curl installer exited non-zero"; }
  [ -z "$(ls -A "$WORK/tmp")" ] || fail "curl installer left files in TMPDIR: $(ls -A "$WORK/tmp")"
}
run_tarball() {
  local home="$1"; shift
  env -i HOME="$home" USER=tester PATH="/usr/bin:/bin" TMPDIR="$WORK/tmp" "$@" \
    bash "$WORK/pkg/$PKG/install.sh" >"$WORK/out.log" 2>&1 \
    || { cat "$WORK/out.log" >&2; fail "tarball installer exited non-zero"; }
  [ -z "$(ls -A "$WORK/tmp")" ] || fail "tarball installer left files in TMPDIR: $(ls -A "$WORK/tmp")"
}

# check HOME DATA_HOME
check() {
  local home="$1" data="$2" entry="$2/applications/zerun.desktop"
  [ -f "$entry" ] || fail "missing $entry"
  [ -f "$data/icons/hicolor/1024x1024/apps/zerun.png" ] || fail "missing hicolor icon"
  # `$(...)` strips nothing needed here: paths in these tests have no newlines.
  grep -qxF "TryExec=$home/.zerun/app/current/zerun" "$entry" || fail "TryExec: $(grep '^TryExec' "$entry")"
  grep -qxF "Icon=$home/.zerun/app/current/zerun.png" "$entry" || fail "Icon: $(grep '^Icon' "$entry")"
  grep -qxF "StartupWMClass=zerun" "$entry" || fail "StartupWMClass changed"
  [ "$(readlink "$home/.local/bin/zerun")" = "$home/.zerun/app/current/zerun" ] || fail "CLI link changed"
  [ "$(grep -c '^\[Desktop Entry\]' "$entry")" = 1 ] || fail "duplicated entry"
  [ "$(grep -c '^Exec=' "$entry")" = 1 ] || fail "Exec lines"
  [ -z "$(find "$data" -name '.zerun*')" ] || fail "temp files left behind"
  # A user-level icon cache is only ever refreshed, never created.
  [ ! -e "$data/icons/hicolor/icon-theme.cache" ] || fail "created a hicolor icon cache"
  if command -v desktop-file-validate >/dev/null 2>&1; then
    desktop-file-validate "$entry" || fail "desktop-file-validate"
  fi
}

for installer in curl tarball; do
  run() { "run_$installer" "$@"; }

  # Default XDG location, then a re-run (how updates are installed) is stable.
  home="$WORK/$installer-a/home"; mkdir -p "$home"
  old_files=(.zeron/app/original .local/bin/zeron .local/share/applications/zeron.desktop .config/systemd/user/zeron.service)
  for file in "${old_files[@]}"; do
    mkdir -p "$(dirname "$home/$file")"
    printf 'other application\n' >"$home/$file"
  done
  run "$home"
  check "$home" "$home/.local/share"
  grep -qxF "Exec=$home/.zerun/app/current/zerun %u" "$home/.local/share/applications/zerun.desktop" \
    || fail "$installer: Exec line"
  before="$(cat "$home/.local/share/applications/zerun.desktop")"
  run "$home"
  check "$home" "$home/.local/share"
  [ "$before" = "$(cat "$home/.local/share/applications/zerun.desktop")" ] || fail "$installer: re-run changed the entry"
  for file in "${old_files[@]}"; do
    [ "$(cat "$home/$file")" = 'other application' ] || fail "$installer: changed $file"
  done

  # XDG_DATA_HOME wins when absolute; a relative value is ignored per the spec.
  home="$WORK/$installer-b/home"; mkdir -p "$home"
  run "$home" XDG_DATA_HOME="$WORK/$installer-b/xdg"
  check "$home" "$WORK/$installer-b/xdg"
  [ ! -e "$home/.local/share/applications" ] || fail "$installer: wrote outside XDG_DATA_HOME"
  home="$WORK/$installer-c/home"; mkdir -p "$home"
  run "$home" XDG_DATA_HOME=relative/dir
  check "$home" "$home/.local/share"

  # A home with a space and characters the Exec key must quote and escape.
  home="$WORK/$installer-d/h o\$me\"x%y"; mkdir -p "$home"
  run "$home"
  check "$home" "$home/.local/share"
  # Spec: quote the argument, `\` before " and $ (doubled again for the file's
  # string escaping), and `%%` for a literal `%`.
  want="Exec=\"$WORK/$installer-d/"'h o\\$me\\"x%%y'"/.zerun/app/current/zerun\" %u"
  grep -qxF "$want" "$home/.local/share/applications/zerun.desktop" \
    || fail "$installer: Exec quoting: $(grep '^Exec=' "$home/.local/share/applications/zerun.desktop")"
  echo "ok: $installer installer"
done

# A systemd session installs and controls only our unit.
home="$WORK/systemd/home"
mkdir -p "$home/.config/systemd/user"
printf 'other application\n' >"$home/.config/systemd/user/zeron.service"
run_curl "$home" XDG_RUNTIME_DIR="$WORK/systemd/runtime"
[ "$(cat "$home/.config/systemd/user/zeron.service")" = 'other application' ] || fail 'changed other service'
grep -qxF 'ExecStart=%h/.zerun/app/current/zerun headless' "$home/.config/systemd/user/zerun.service" || fail 'service executable'
grep -qxF -- '--user enable zerun' "$home/systemctl.log" || fail 'service enable'
grep -qxF -- '--user restart zerun' "$home/systemctl.log" || fail 'service restart'
[ "$(wc -l <"$home/systemctl.log")" -eq 3 ] || fail 'unexpected service commands'
echo 'ok: independent systemd service'
