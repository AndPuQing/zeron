#!/bin/sh
# Copies example plugins into a Zerun data dir's plugins folder and signs
# them as that engine's account (`zerun plugin sign`), for a scratch engine:
#
#   ./install.sh <data-dir> <port> [plugin ...]     (default: all examples)
#
# The engine must be running (its IPC on <port>, its token in <data-dir>).
# Stable ports and profiles are refused.
# `focus` is meant to be signed by another account: install it on that
# account's scratch engine, then copy its signed folder over.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
data=${1:?usage: install.sh <data-dir> <port> [plugin ...]}
port=${2:?usage: install.sh <data-dir> <port> [plugin ...]}
shift 2
case "$port" in 27654|27655) echo "refusing a stable engine's port" >&2; exit 1 ;; esac
case "$(cd "$data" && pwd -P)" in
  "$HOME/.zerun"|"$HOME/.zerun/"*|"$HOME/.zeron"|"$HOME/.zeron/"*|"$HOME/.comet-native"|"$HOME/.comet-native/"*)
    echo "refusing a stable engine's data" >&2; exit 1 ;;
esac
[ $# -gt 0 ] || set -- checkout-status compact-sidebar focus notes-tool ping session-status settings-demo
zerun=${ZERUN_BIN:-$root/target/debug/zerun}
mkdir -p "$data/plugins"
for id in "$@"; do
  case "$id" in checkout-status|compact-sidebar|focus|notes-tool|ping|session-status|settings-demo) ;;
    *) echo "unknown example plugin: $id" >&2; exit 1 ;;
  esac
  rm -rf "$data/plugins/$id"
  cp -R "$here/$id" "$data/plugins/$id"
  cp "$root/apps/zgx/plugins-sdk/zeron.luau" "$data/plugins/$id/zeron.luau"
done
sleep 1
for id in "$@"; do
  env -u ZERON_IPC_TOKEN ZERUN_IPC_PORT="$port" ZERUN_DATA_DIR="$data" ZERON_IPC_TOKEN_FILE="$data/ipc-token" "$zerun" plugin sign "$id"
done
env -u ZERON_IPC_TOKEN ZERUN_IPC_PORT="$port" ZERUN_DATA_DIR="$data" ZERON_IPC_TOKEN_FILE="$data/ipc-token" "$zerun" plugin list
