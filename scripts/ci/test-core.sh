#!/usr/bin/env bash
# The Linux Rust tests that used to run as separate `cargo test` invocations in
# session-sync-regressions, cursor-compatibility and preview-tests, as ONE
# build and ONE nextest run (separate invocations recompiled the same
# dependency graph with different feature sets).
#
# Coverage:
#   zeron-harness  every test binary (cursor-compatibility ran `-p zeron-harness`
#                  plus `--test pi_rpc` with native-fixture; native-fixture only
#                  adds the pi_rpc fixture binaries, so it is on for the whole build)
#   zeron-preview  every test binary (preview-tests)
#   zeron-doc      lib + persistence and memory regressions + attachments_roundtrip
#   zeron-sync, zeron-update   lib only (session-sync-regressions)
#   zeron-term-link   terminal transport library
#   zeron-visual, zeron-visual-gpu   script sandbox and renderer libraries
#   zeron-engine   lib + the integration tests named below. Engine has many other
#                  integration binaries (live agents etc.) that no workflow ran, so
#                  they are listed instead of globbed to avoid compiling them.
# Only the visual publication case from engine/e2e runs here; it checks the
# RPC, upload, and live-turn document path together.
# nextest does not run doctests; these crates have none (their doc comments
# contain no Rust code blocks).
# Extra arguments are passed to nextest (the nightly job adds `--release`).
set -euo pipefail
cd "$(dirname "$0")/../.."

tests=()
for f in crates/harness/tests/*.rs crates/preview/tests/*.rs crates/doc/tests/*.rs; do
  tests+=(--test "$(basename "$f" .rs)")
done
for t in session_publication restart_resume runtime_survival side_chats codex_subagents claude_subagents local_profiles message_queue pi_resume device_routing e2e osc7501 transcript_search usage_insights; do
  tests+=(--test "$t")
done

exec cargo nextest run --config-file scripts/ci/nextest.toml --locked --no-fail-fast \
  -p zeron-harness -p zeron-engine -p zeron-sync -p zeron-update -p zeron-doc -p zeron-preview \
  -p zeron-visual -p zeron-visual-gpu -p zeron-term-link \
  --features zeron-harness/native-fixture \
  -E 'not binary_id(zeron-engine::e2e) | test(=publish_visual_lands_in_the_live_turn)' \
  --lib --bins "${tests[@]}" "$@"
