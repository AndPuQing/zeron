#!/usr/bin/env bash
# Build both workspaces from this checkout, then launch the isolated desktop.
set -euo pipefail
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"
profile=debug
build_flags=(--profile dev)
if [[ ${1:-} == --release ]]; then
  profile=release
  build_flags=(--release)
  shift
fi
cargo build --locked -p zeron --bin zerun-engine --no-default-features "${build_flags[@]}"
engine_target=$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
engine_bin="$engine_target/$profile/zerun-engine"
[[ -f "$engine_bin.exe" ]] && engine_bin="$engine_bin.exe"
cd "$repo_root/apps/zgx"
cargo build --locked --bin zerun-experimental "${build_flags[@]}"
app_target=$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
app_bin="$app_target/$profile/zerun-experimental"
[[ -f "$app_bin.exe" ]] && app_bin="$app_bin.exe"
export ZERUN_ZGX_ENGINE_BIN="$engine_bin"
exec "$app_bin" "$@"
