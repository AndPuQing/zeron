#!/usr/bin/env bash
# Package the existing application executable for CLI/headless use, independently
# signed from the app bundle. No build or GUI session is required here.
# Usage: scripts/package-macos-headless.sh BINARY OUTPUT_DIR VERSION
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:?binary required}"
OUT="${2:?output directory required}"
VERSION="${3:?version required}"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo 'invalid release version' >&2; exit 1; }
[[ "$(uname -s)" == Darwin ]] || { echo 'macOS packaging requires macOS' >&2; exit 1; }
ARCH="$(uname -m)"
NAME="zerun-$VERSION-macos-$ARCH"
mkdir -p "$OUT"
STAGE="$(mktemp -d "$OUT/.headless.XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT
PAYLOAD="$STAGE/$NAME"
mkdir -p "$PAYLOAD/licenses/fonts"
install -m 755 "$BINARY" "$PAYLOAD/zerun"
install -m 644 "$ROOT/LICENSE" "$ROOT/THIRD_PARTY_NOTICES.md" "$PAYLOAD/"
cp "$ROOT/crates/ui/assets/fonts/licenses/"* "$PAYLOAD/licenses/fonts/"
cp "$ROOT/crates/voice/NOTICE.md" "$PAYLOAD/licenses/parakeet-v3.txt"
# Bundle signatures bind their Info.plist. A copied bundle executable must be
# signed as a standalone program before it can run outside Zerun.app.
if [[ -n "${CODESIGN_IDENTITY:-}" ]]; then
  codesign --entitlements "$ROOT/dist/macos/Zerun.entitlements" --force --options runtime --timestamp \
    --sign "$CODESIGN_IDENTITY" --identifier work.puqing.zerun.cli "$PAYLOAD/zerun"
else
  codesign --entitlements "$ROOT/dist/macos/Zerun.entitlements" --force \
    --sign - --identifier work.puqing.zerun.cli "$PAYLOAD/zerun"
fi
codesign --verify --strict "$PAYLOAD/zerun"
[[ "$("$PAYLOAD/zerun" --version)" == "zerun $VERSION" ]] \
  || { echo 'headless executable version mismatch' >&2; exit 1; }
if [[ -n "${NOTARY_KEY_PATH:-}" && -n "${NOTARY_KEY_ID:-}" && -n "${NOTARY_ISSUER_ID:-}" ]]; then
  # A standalone Mach-O cannot carry a stapled ticket. Notarize its own
  # signature so online Gatekeeper checks can validate a quarantined download.
  ditto -c -k --keepParent "$PAYLOAD/zerun" "$STAGE/notarize.zip"
  xcrun notarytool submit "$STAGE/notarize.zip" \
    --key "$NOTARY_KEY_PATH" --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER_ID" \
    --wait --output-format json >"$STAGE/notary-result.json"
  python3 -c 'import json,sys; r=json.load(open(sys.argv[1])); assert r.get("status") == "Accepted", "CLI notarization was not accepted"' "$STAGE/notary-result.json"
fi
tar -czf "$STAGE/$NAME.tar.gz" -C "$STAGE" "$NAME"
mv -f "$STAGE/$NAME.tar.gz" "$OUT/$NAME.tar.gz"
echo "packaged: $OUT/$NAME.tar.gz"
