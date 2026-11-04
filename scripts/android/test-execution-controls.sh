#!/usr/bin/env bash
# Runs only in CI against the APKs from the Android build job.
set -euo pipefail
output="${RUNNER_TEMP:?}/execution-android"
mkdir -p "$output"
emulator_pid=""
cleanup() {
  timeout 10 adb logcat -d > "$output/logcat.txt" 2>/dev/null || true
  timeout 10 adb exec-out run-as work.puqing.zerun.android tar -cf - -C cache execution-screenshots \
    | tar -xf - -C "$output" 2>/dev/null || true
  if [[ -n "$emulator_pid" ]]; then kill "$emulator_pid" 2>/dev/null || true; fi
}
trap cleanup EXIT
printf 'no\n' | avdmanager create avd -n execution-controls -k 'system-images;android-35;google_apis;x86_64' --force
# Recent emulator defaults reserve 12 GiB for userdata, more than the free
# disk on this runner. This disposable test device only needs the two APKs.
python3 - "${ANDROID_AVD_HOME:-$HOME/.android/avd}/execution-controls.avd/config.ini" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
lines = [line for line in path.read_text().splitlines() if line.partition("=")[0].strip() != "disk.dataPartition.size"]
path.write_text("\n".join(lines + ["disk.dataPartition.size=4G"]) + "\n")
PY
"$ANDROID_HOME/emulator/emulator" -avd execution-controls -no-window -no-audio \
  -no-boot-anim -no-snapshot -gpu swiftshader_indirect -memory 2048 -cores 2 \
  > "$output/emulator.log" 2>&1 &
emulator_pid=$!
adb start-server
booted=false
for attempt in $(seq 1 120); do
  if ! kill -0 "$emulator_pid" 2>/dev/null; then
    tail -40 "$output/emulator.log" >&2
    echo 'Emulator exited before boot' >&2
    exit 1
  fi
  if [[ "$(timeout 3 adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" == 1 ]]; then
    booted=true
    break
  fi
  sleep 2
done
[[ "$booted" == true ]] || { echo 'Emulator did not boot' >&2; exit 1; }
adb shell input keyevent 82
adb shell settings put global window_animation_scale 0
adb shell settings put global transition_animation_scale 0
adb shell settings put global animator_duration_scale 0
adb install -r "$RUNNER_TEMP/android-apks/app-debug.apk"
adb install -r "$RUNNER_TEMP/android-apks/app-debug-androidTest.apk"
adb shell am instrument -w -e class sh.zeron.android.ui.ExecutionPolicyFlowTest \
  work.puqing.zerun.android.test/androidx.test.runner.AndroidJUnitRunner | tee "$output/test-results.txt"
# A class can gain tests; require a non-empty successful JUnit run.
grep -Eq '^[[:space:]]*OK \([1-9][0-9]* tests?\)[[:space:]]*$' "$output/test-results.txt"
adb shell am instrument -w -e class sh.zeron.android.transcript.DocumentPreviewTest \
  work.puqing.zerun.android.test/androidx.test.runner.AndroidJUnitRunner | tee "$output/document-results.txt"
grep -Eq '^[[:space:]]*OK \([1-9][0-9]* tests?\)[[:space:]]*$' "$output/document-results.txt"
adb shell am instrument -w -e class sh.zeron.android.ui.DocumentReaderAppearanceTest \
  work.puqing.zerun.android.test/androidx.test.runner.AndroidJUnitRunner | tee "$output/document-appearance-results.txt"
grep -Eq '^[[:space:]]*OK \([1-9][0-9]* tests?\)[[:space:]]*$' "$output/document-appearance-results.txt"
