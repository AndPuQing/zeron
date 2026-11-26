#!/usr/bin/env bash
# Package the stable desktop and the standalone macOS CLI.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"

# The CLI tarball keeps the standalone executable used by existing installs.
LUAU_CXXFLAGS=-DNDEBUG cargo build --release --locked -p zeron
CARGO_FLAGS=--locked ZGX_ENGINE="$ROOT/target/release/zerun-engine" scripts/package-zgx-macos.sh
scripts/package-macos-headless.sh "$ROOT/target/release/zerun" "$ROOT/target/package" "$VERSION"
