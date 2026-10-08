#!/usr/bin/env bash
# Build, verify and package the signed ARM64 Android APK.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$(python3 - "$ROOT/Cargo.toml" <<'PY'
import sys
import tomllib
with open(sys.argv[1], "rb") as source:
    print(tomllib.load(source)["workspace"]["package"]["version"])
PY
)"

for name in ZERUN_ANDROID_KEYSTORE ZERUN_ANDROID_STORE_PASSWORD \
    ZERUN_ANDROID_KEY_ALIAS ZERUN_ANDROID_KEY_PASSWORD; do
  [[ -n "${!name:-}" ]] || { echo "Required signing variable $name is not configured." >&2; exit 2; }
done
[[ -f "$ZERUN_ANDROID_KEYSTORE" ]] || { echo "The release keystore file does not exist." >&2; exit 2; }
if [[ "${GITHUB_REF_TYPE:-}" == tag && "${GITHUB_REF_NAME:-}" != "v$VERSION" ]]; then
  echo "Release tag must match workspace version v$VERSION." >&2
  exit 2
fi

SDK_ROOT="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
TOOLS="$SDK_ROOT/build-tools/36.0.0"
[[ -x "$TOOLS/apksigner" && -x "$TOOLS/zipalign" ]] \
  || { echo "Android Build Tools 36.0.0 are required." >&2; exit 2; }
expected="$(tr -d '[:space:]' < "$ROOT/apps/android/release-certificate.sha256")"
keystore_cert="$(keytool -exportcert -keystore "$ZERUN_ANDROID_KEYSTORE" \
  -storepass:env ZERUN_ANDROID_STORE_PASSWORD -alias "$ZERUN_ANDROID_KEY_ALIAS" | sha256sum | cut -d' ' -f1)"
[[ "$keystore_cert" == "$expected" ]] \
  || { echo "Keystore certificate does not match the configured release identity." >&2; exit 3; }

# Release is ARM64-only, even when a developer's debug build uses both ABIs.
export ZERON_ANDROID_ABIS=arm64-v8a
"$ROOT/apps/android/gradlew" -p "$ROOT/apps/android" \
  :app:testDebugUnitTest :app:assembleRelease \
  --no-daemon --no-configuration-cache --console=plain "$@"

APK="$ROOT/apps/android/app/build/outputs/apk/release/app-release.apk"
signature="$("$TOOLS/apksigner" verify --verbose --print-certs "$APK")"
printf '%s\n' "$signature"
actual="$(sed -n 's/^Signer #1 certificate SHA-256 digest: //p' <<<"$signature")"
[[ "$actual" == "$expected" ]] || { echo "APK signer does not match the production key." >&2; exit 3; }
"$TOOLS/zipalign" -c -P 16 4 "$APK"
python3 - "$APK" <<'PY'
import sys
import zipfile
with zipfile.ZipFile(sys.argv[1]) as apk:
    abis = {name.split("/")[1] for name in apk.namelist() if name.startswith("lib/") and name.endswith(".so")}
    if abis != {"arm64-v8a"}:
        raise SystemExit(f"Release APK must contain only arm64-v8a libraries, found: {sorted(abis)}")
    if apk.getinfo("lib/arm64-v8a/libzeron_mobile.so").file_size == 0:
        raise SystemExit("Empty native core for arm64-v8a")
PY

OUT="$ROOT/target/package"
NAME="zerun-$VERSION-android.apk"
mkdir -p "$OUT"
install -m 644 "$APK" "$OUT/$NAME"
(cd "$OUT" && sha256sum "$NAME" > "$NAME.sha256")
echo "packaged: $OUT/$NAME"
