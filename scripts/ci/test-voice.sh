#!/usr/bin/env bash
# Build the offline voice suites together so Cargo unifies their dependency
# features once. Keep the former cargo-test selections, including native AEC3.
set -euo pipefail
cd "$(dirname "$0")/../.."

filter='package(zeron-voice-media) | package(zeron-orb) | package(zeron-voice-session)
  | package(zeron-audio) | package(zeron-harness) | package(zeron-mcp)
  | (package(zeron-mobile) & (test(voice) | test(orb)))
  | (package(zeron-proto) & test(voice))
  | (package(zeron-doc) & test(voice))
  | (package(zeron-engine) & (binary_id(zeron-engine::voice_runtime)
      | binary_id(zeron-engine::device_routing) | test(voice)))
  | binary_id(zeron-rpc::device_room)'

exec cargo nextest run --config-file scripts/ci/nextest.toml --locked --no-fail-fast \
  -p zeron-voice-media -p zeron-orb -p zeron-voice-session -p zeron-audio \
  -p zeron-mobile -p zeron-proto -p zeron-harness -p zeron-doc \
  -p zeron-engine -p zeron-rpc -p zeron-mcp \
  --features zeron-audio/aec --lib \
  --test codex --test acp --test voice_runtime --test device_routing --test device_room \
  -E "$filter" "$@"
