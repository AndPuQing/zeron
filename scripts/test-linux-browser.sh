#!/usr/bin/env bash
# Run the real shell browser fixture with native mouse input on X11 and Wayland.
# Captures stay outside the repository and can be uploaded as PR attachments.
set -euo pipefail
command -v xprop >/dev/null || {
  echo "xprop is required (install x11-utils)" >&2
  exit 2
}
output="${1:?usage: test-linux-browser.sh OUTPUT_DIRECTORY}"
binary="${BROWSER_FIXTURE_BINARY:-target/release/examples/browser-fixture}"
mkdir -p "$output"
output="$(realpath "$output")"
runtime="$(mktemp -d)"
chmod 700 "$runtime"
processes=()
cleanup() {
  for pid in "${processes[@]}"; do kill "$pid" 2>/dev/null || true; done
  rm -rf "$runtime"
}
trap cleanup EXIT
Xvfb -displayfd 3 -screen 0 1280x800x24 -ac -nolisten tcp 3>"$runtime/display" >"$output/xvfb.log" 2>&1 &
xvfb_pid=$!
processes+=("$xvfb_pid")
display_number=
for _ in $(seq 1 300); do
  if IFS= read -r display_number <"$runtime/display"; then break; fi
  if ! kill -0 "$xvfb_pid" 2>/dev/null; then
    echo "Xvfb exited before publishing a display" >&2
    cat "$output/xvfb.log" >&2
    exit 1
  fi
  sleep .1
done
if [[ ! "$display_number" =~ ^[0-9]+$ ]]; then
  echo "Xvfb did not publish a valid display within 30 seconds" >&2
  cat "$output/xvfb.log" >&2
  exit 1
fi
export DISPLAY=":$display_number"
export XDG_RUNTIME_DIR="$runtime"
export ZERON_BROWSER_NATIVE_POINTER=1
openbox >"$output/openbox.log" 2>&1 &
openbox_pid=$!
processes+=("$openbox_pid")
# Do not launch the fixture while the WM is still grabbing/setting up: the
# storage test (xvfb-run, no WM) passes, while this script previously started
# the fixture immediately after openbox. Wait for the EWMH WM check window to
# be published and named, then the 10s window-visibility budget is meaningful.
wm_ready=
for _ in $(seq 1 100); do
  if ! kill -0 "$openbox_pid" 2>/dev/null; then
    echo "openbox exited before becoming ready" >&2
    cat "$output/openbox.log" >&2 || true
    exit 1
  fi
  wm_window="$(xprop -root -notype _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -oE '0x[0-9a-fA-F]+' | head -1 || true)"
  if [[ -n "$wm_window" && "$wm_window" != "0x0" ]] &&
     xprop -id "$wm_window" -notype _NET_WM_NAME 2>/dev/null | grep -q '='; then
    wm_ready=1
    break
  fi
  sleep .1
done
if [[ "$wm_ready" != 1 ]]; then
  echo "openbox did not publish _NET_SUPPORTING_WM_CHECK within 10 seconds" >&2
  xprop -root -notype _NET_SUPPORTING_WM_CHECK 2>/dev/null >&2 || true
  cat "$output/openbox.log" >&2 || true
  exit 1
fi
record_fixture() {
  local mode="$1" capture_window="${2:-}" fixture_pid video_pid
  mkdir -p "$output/$mode"
  "$binary" "$output/$mode" >"$output/$mode/fixture.log" 2>&1 &
  fixture_pid=$!
  processes+=("$fixture_pid")
  if [ -z "$capture_window" ]; then
    for _ in $(seq 1 150); do
      capture_window="$(xdotool search --onlyvisible --pid "$fixture_pid" 2>/dev/null | head -1 || true)"
      [ -n "$capture_window" ] && break
      if ! kill -0 "$fixture_pid" 2>/dev/null; then
        cat "$output/$mode/fixture.log"
        # Keep the native window tree with the failing artifact: a mapped but
        # unfindable window and a never-mapped window need different fixes.
        xwininfo -root -tree >"$output/$mode/xwininfo.log" 2>&1 || true
        xprop -root -notype _NET_SUPPORTING_WM_CHECK >"$output/$mode/wm-check.log" 2>&1 || true
        return 1
      fi
      sleep .1
    done
  fi
  if [ -z "$capture_window" ]; then
    echo "fixture window not visible to the runner after 15 seconds" >&2
    xwininfo -root -tree >"$output/$mode/xwininfo.log" 2>&1 || true
    xprop -root -notype _NET_SUPPORTING_WM_CHECK >"$output/$mode/wm-check.log" 2>&1 || true
    return 1
  fi
  local codec="${BROWSER_FIXTURE_VIDEO_CODEC:-libx264}"
  local -a encoding=(-c:v "$codec")
  if [ "$codec" = libx264 ]; then
    encoding+=(-preset veryfast -crf 20)
  else
    encoding+=(-q:v 4)
  fi
  ffmpeg -nostdin -loglevel error -y -f x11grab -framerate 30 -window_id "$capture_window" -i "$DISPLAY" \
    "${encoding[@]}" -pix_fmt yuv420p -movflags +faststart \
    "$output/$mode/browser.mp4" >"$output/$mode/video.log" 2>&1 &
  video_pid=$!
  processes+=("$video_pid")
  local result=0
  wait "$fixture_pid" || result=$?
  kill -INT "$video_pid" 2>/dev/null || true
  wait "$video_pid" || true # X11 grab may end when the fixture closes its window.
  cat "$output/$mode/fixture.log"
  [ "$result" -eq 0 ]
  test -f "$output/$mode/result.txt"
  test -f "$output/$mode/linux-results.json"
  ffprobe -v error -select_streams v:0 -show_entries stream=width,height -of json "$output/$mode/browser.mp4"
}
export WAYLAND_DISPLAY= GDK_BACKEND=x11
record_fixture x11
export WAYLAND_DISPLAY=zeron-browser-test GDK_BACKEND=wayland
weston --backend=x11 --renderer=pixman --shell=kiosk-shell.so --socket="$WAYLAND_DISPLAY" \
  --idle-time=0 --width=1000 --height=680 >"$output/weston.log" 2>&1 &
processes+=("$!")
for _ in $(seq 1 100); do
  [ -S "$runtime/$WAYLAND_DISPLAY" ] && break
  sleep .1
done
export ZERON_BROWSER_CAPTURE_WINDOW="$(xdotool search --class weston | head -1)"
record_fixture wayland "$ZERON_BROWSER_CAPTURE_WINDOW"
