#!/usr/bin/env bash
# Runs only in CI against the APKs from the Android build job.
set -euo pipefail
output="${RUNNER_TEMP:?}/execution-android"
mkdir -p "$output"
emulator_pid=""
cleanup() {
  adb logcat -d > "$output/logcat.txt" 2>/dev/null || true
  adb exec-out run-as work.puqing.zerun.android tar -cf - -C cache execution-screenshots \
    | tar -xf - -C "$output" 2>/dev/null || true
  if [[ -n "$emulator_pid" ]]; then kill "$emulator_pid" 2>/dev/null || true; fi
}
trap cleanup EXIT
printf 'no\n' | avdmanager create avd -n execution-controls -k 'system-images;android-35;google_apis;x86_64' --force
"$ANDROID_HOME/emulator/emulator" -avd execution-controls -no-window -no-audio \
  -no-boot-anim -no-snapshot -gpu swiftshader_indirect -memory 2048 -cores 2 \
  > "$output/emulator.log" 2>&1 &
emulator_pid=$!
adb wait-for-device
booted=false
for attempt in $(seq 1 120); do
  if [[ "$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" == 1 ]]; then
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
grep -q 'OK (1 test)' "$output/test-results.txt"
