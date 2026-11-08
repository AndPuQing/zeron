#!/usr/bin/env bash
# Zerun.app's beta: the zgx interface (apps/zgx, built with its `beta`
# feature) as the Zerun app itself — the same bundle id, name, signing,
# entitlements and `zerun-dev://` scheme as scripts/package-macos.sh's — with
# the engine it's given inside, built from the same commit, so a tester keeps
# Zerun.app's data and device:
#
#   target/package-beta/Zerun.app
#     Contents/MacOS/zerun           the zgx interface and `zerun`'s command
#                                    line (crates/cli; the engine's commands
#                                    run in the helper beside it)
#     Contents/MacOS/Zerun Engine    the engine, signed here
#     Contents/Resources/ui, plugins-builtin, zerun.icns, licenses
#   target/package-beta/zerun-<beta>-macos-<arch>.dmg
#   target/package-beta/zerun-<beta>-macos-<arch>-app.tar.gz  (the updater's)
#
# Usage: BETA_VERSION=0.3.30-beta.1 ZGX_BETA_ENGINE=<engine> scripts/package-zgx-beta.sh
#   BETA_VERSION        `<workspace version>-beta.<n>`; the workspace version
#                       is the next release's; both binaries must match it
#   ZGX_BETA_ENGINE     the engine: a built binary, a Zerun.app, its app tarball, or
#                       its `Zerun Engine` binary
#   ZGX_BETA_ZGX_BIN    the interface, built already (with --features beta and
#                       ZERUN_BETA_VERSION); otherwise built here
#   ZGX_TARGET_DIR      where to build it (apps/zgx/target)
#   ZGX_BETA_DMG=0      no dmg
#   CARGO_FLAGS         extra cargo flags (CI: --locked)
# Signing and notarization are scripts/package-macos.sh's: CODESIGN_IDENTITY,
# NOTARY_KEY_PATH + NOTARY_KEY_ID + NOTARY_ISSUER_ID.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
command -v cargo >/dev/null 2>&1 || PATH="$HOME/.cargo/bin:$PATH"
die() { echo "$*" >&2; exit 1; }

WORKSPACE="$(grep -m1 '^version' "$ROOT/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')"
BETA_VERSION="${BETA_VERSION:-}"
[[ "$BETA_VERSION" =~ ^([0-9]+\.[0-9]+\.[0-9]+)-beta\.([0-9]+)$ ]] ||
  die "BETA_VERSION must be <x.y.z>-beta.<n>, not '$BETA_VERSION'"
CORE="${BASH_REMATCH[1]}"
[[ "$CORE" == "$WORKSPACE" ]] || die "BETA_VERSION $BETA_VERSION isn't a beta of the workspace's $WORKSPACE"
ARCH="$(uname -m)"
OUT_DIR="$ROOT/target/package-beta"
APP="$OUT_DIR/Zerun.app"
DMG="$OUT_DIR/zerun-$BETA_VERSION-macos-$ARCH.dmg"
APP_TARBALL="$OUT_DIR/zerun-$BETA_VERSION-macos-$ARCH-app.tar.gz"
ENTITLEMENTS="$ROOT/dist/macos/Zerun.entitlements"
read -r -a CARGO_EXTRA <<<"${CARGO_FLAGS:-}"
mkdir -p "$OUT_DIR"

# The engine: the binary given, or the one in a bundle or tarball.
ENGINE_SRC="${ZGX_BETA_ENGINE:-}"
[[ -n "$ENGINE_SRC" ]] || die "ZGX_BETA_ENGINE: a built engine, a Zerun.app, its app tarball or its Zerun Engine"
ENGINE_BIN="$ENGINE_SRC"
if [[ "$ENGINE_SRC" == *.tar.gz ]]; then
  UNPACK="$OUT_DIR/.stable"
  rm -rf "$UNPACK" && mkdir -p "$UNPACK"
  tar -xzf "$ENGINE_SRC" -C "$UNPACK"
  ENGINE_SRC="$UNPACK/Zerun.app"
fi
[[ -d "$ENGINE_SRC" ]] && ENGINE_BIN="$ENGINE_SRC/Contents/MacOS/Zerun Engine"
[[ -x "$ENGINE_BIN" ]] || die "no engine at $ENGINE_BIN"
ENGINE_VERSION="$("$ENGINE_BIN" --version | sed -n 's/^zerun-engine //p')"
[[ "$ENGINE_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-beta\.[0-9]+)?$ ]] || die "$ENGINE_BIN didn't say its version"
# The interface hands `zerun headless`, `zerun login` and the engine's other
# subcommands to this engine, which must parse them with the same command
# line (crates/cli). An engine from before that runs the background engine
# for any argument instead. (Read, not run: running an old one starts it.)
LC_ALL=C grep -aq "Multi-device controller for coding agents" "$ENGINE_BIN" ||
  die "the $ENGINE_VERSION engine predates the shared command line: the beta needs the shared command line"

[[ "$ENGINE_VERSION" == "$BETA_VERSION" ]] || die "The Beta requires a matching $BETA_VERSION engine"

# The interface: apps/zgx is a workspace of its own (its own lock, patches).
ZGX_BIN="${ZGX_BETA_ZGX_BIN:-}"
if [[ -z "$ZGX_BIN" ]]; then
  target_dir="${ZGX_TARGET_DIR:-$ROOT/apps/zgx/target}"
  [[ "$target_dir" == /* ]] || target_dir="$PWD/$target_dir"
  # (From its folder: cargo reads .cargo/config.toml by where it runs, not by
  # the manifest, and the build's flags are there.)
  (cd "$ROOT/apps/zgx" && ZERUN_BETA_VERSION="$BETA_VERSION" ZERUN_ENGINE_VERSION="$BETA_VERSION" cargo build --release ${CARGO_EXTRA[@]+"${CARGO_EXTRA[@]}"} \
    --features beta --target-dir "$target_dir")
  ZGX_BIN="$target_dir/release/zerun-experimental"
fi
# Only a beta build of this version may go in: the updater stages it by
# `zerun --version`, and a development build is staging's.
[[ "$("$ZGX_BIN" --version)" == "zerun $BETA_VERSION" ]] ||
  die "$ZGX_BIN isn't the $BETA_VERSION beta (it says '$("$ZGX_BIN" --version)')"

rm -rf "$APP" "$DMG" "$APP_TARBALL"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
install -m 755 "$ZGX_BIN" "$APP/Contents/MacOS/zerun"
install -m 755 "$ENGINE_BIN" "$APP/Contents/MacOS/Zerun Engine"
# Zerun.app's Info.plist, as the stable app's, with the beta's versions: the
# system versions stay numeric (the next release's), the full Beta version
# is kept separately, the engine its own (`bundle_engine_version`), and
# zgx's floor macOS 13 (Ghostty's).
PLIST="$APP/Contents/Info.plist"
sed "s/__VERSION__/$CORE/" "$ROOT/dist/macos/Info.plist" >"$PLIST"
plutil -replace ZeronEngineVersion -string "$ENGINE_VERSION" "$PLIST"
plutil -insert ZeronVersion -string "$BETA_VERSION" "$PLIST"
plutil -replace LSMinimumSystemVersion -string "13.0" "$PLIST"
plutil -convert xml1 "$PLIST"
plutil -lint "$PLIST" >/dev/null

# The scripts and Zerun's own plugins (read where they lie: templates and
# plugins live in the data dir). Not zgx's generated types or the tests.
rsync -a --exclude '.zgx/' --exclude '/tests/' --exclude '.lock' --exclude '.DS_Store' \
  "$ROOT/apps/zgx/ui/" "$APP/Contents/Resources/ui/"
rsync -a --exclude '.zgx/' --exclude '.DS_Store' \
  "$ROOT/apps/zgx/plugins-builtin/" "$APP/Contents/Resources/plugins-builtin/"

mkdir -p "$APP/Contents/Resources/licenses/fonts"
cp "$ROOT/crates/ui/assets/fonts/licenses/"* "$ROOT/apps/zgx/fonts/"*-OFL.txt "$APP/Contents/Resources/licenses/fonts/"
cp "$ROOT/THIRD_PARTY_NOTICES.md" "$ROOT/LICENSE" "$APP/Contents/Resources/licenses/"
cp "$ROOT/crates/voice/NOTICE.md" "$APP/Contents/Resources/licenses/parakeet-v3.txt"

# Icon: iconset from the pre-masked macOS icon (squircle + margins + shadow
# baked into dist/macos/icon-1024.png — sips can't alpha-mask, so the mask is
# applied ahead of time; dist/zeron.png stays the full-bleed shared artwork).
ICONSET="$OUT_DIR/zerun.iconset"
rm -rf "$ICONSET" && mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$ROOT/dist/macos/icon-1024.png" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  retina=$((size * 2))
  sips -z "$retina" "$retina" "$ROOT/dist/macos/icon-1024.png" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/zerun.icns"
rm -rf "$ICONSET"


# Signed as scripts/package-macos.sh signs Zerun.app: the engine (its
# identifier), then the bundle.
if [[ -n "${CODESIGN_IDENTITY:-}" ]]; then
  codesign --entitlements "$ENTITLEMENTS" --force --options runtime --timestamp --sign "$CODESIGN_IDENTITY" --identifier work.puqing.zerun.engine "$APP/Contents/MacOS/Zerun Engine"
  codesign --entitlements "$ENTITLEMENTS" --force --options runtime --timestamp --sign "$CODESIGN_IDENTITY" "$APP"
else
  codesign --force --entitlements "$ENTITLEMENTS" --sign - --identifier work.puqing.zerun.engine "$APP/Contents/MacOS/Zerun Engine"
  codesign --force --entitlements "$ENTITLEMENTS" --sign - "$APP"
fi
codesign --verify --strict --verbose=2 "$APP"

notarize() {
  xcrun notarytool submit "$1" \
    --key "$NOTARY_KEY_PATH" --key-id "$NOTARY_KEY_ID" \
    --issuer "$NOTARY_ISSUER_ID" --wait
}
NOTARIZE=false
[[ -n "${NOTARY_KEY_PATH:-}" && -n "${NOTARY_KEY_ID:-}" && -n "${NOTARY_ISSUER_ID:-}" ]] && NOTARIZE=true

if $NOTARIZE; then
  ZIP="$OUT_DIR/zerun-notarize.zip"
  ditto -c -k --keepParent "$APP" "$ZIP"
  notarize "$ZIP"
  rm -f "$ZIP"
  xcrun stapler staple "$APP"
fi

# The updater's artifact (crates/update `stage_mac_app` unpacks Zerun.app).
tar -czf "$APP_TARBALL" -C "$OUT_DIR" Zerun.app
echo "packaged: $APP_TARBALL"
echo "size: $(du -sh "$APP" | cut -f1) ($(du -sk "$APP" | cut -f1) KB) · interface $(du -h "$APP/Contents/MacOS/zerun" | cut -f1) · engine $(du -h "$APP/Contents/MacOS/Zerun Engine" | cut -f1)"

if [[ "${ZGX_BETA_DMG:-1}" == "0" ]]; then
  exit 0
fi

python3 -c 'import dmgbuild' 2>/dev/null ||
  python3 -m pip install --quiet --user dmgbuild 2>/dev/null ||
  python3 -m pip install --quiet --user --break-system-packages dmgbuild

BG_TIFF="$OUT_DIR/dmg-background.tiff"
tiffutil -cathidpicheck "$ROOT/dist/macos/dmg-background.png" \
  "$ROOT/dist/macos/dmg-background@2x.png" -out "$BG_TIFF" >/dev/null 2>&1

APP="$APP" DMG="$DMG" BG_TIFF="$BG_TIFF" python3 - <<'PY'
import os
import dmgbuild

app = os.environ["APP"]
dmgbuild.build_dmg(
    filename=os.environ["DMG"],
    volume_name="Zerun Beta",
    settings={
        "format": "UDZO",
        "files": [app],
        "symlinks": {"Applications": "/Applications"},
        "icon": os.path.join(app, "Contents/Resources/zerun.icns"),
        "background": os.environ["BG_TIFF"],
        "show_status_bar": False,
        "show_tab_view": False,
        "show_toolbar": False,
        "show_pathbar": False,
        "show_sidebar": False,
        "default_view": "icon-view",
        # Window and icon geometry as scripts/package-macos.sh's (the same art).
        "window_rect": ((200, 120), (660, 400)),
        "icon_size": 104,
        "text_size": 12,
        "icon_locations": {"Zerun.app": (165, 195), "Applications": (495, 195)},
    },
)
PY
rm -f "$BG_TIFF"
if $NOTARIZE; then
  notarize "$DMG"
  xcrun stapler staple "$DMG"
fi
echo "packaged: $DMG"
