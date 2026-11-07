# Provider update rig

This rig exercises provider CLI updates in an isolated engine process. It checks
that each installation is updated by the package manager or installer that owns
it, and that a failed or interactive update is reported without hanging the
engine.

Every scenario gets its own temporary data directory, home directory, cache,
port, and provider paths. The rig only starts and stops the engine it launched.
Never point `RIG_DIR` at a real Zerun data directory or an existing installation.

## Requirements

- Rust and Python 3
- Node.js, npm, and bun on `PATH`
- Network access for package-install scenarios
- A built Zerun engine and the small `rigclient` helper

## Build

Run these commands from the repository root. Replace the paths in angle
brackets with paths in your checkout.

```sh
cargo build -p zeron --bin zerun-engine
cargo build --manifest-path scripts/provider-update-rig/rigclient/Cargo.toml
```

The helper has its own Cargo package and is not part of the main workspace.

## Run on macOS or Linux

```sh
export RIG_DIR="$(mktemp -d)/provider-update-rig"
export RIGCLIENT="$PWD/scripts/provider-update-rig/rigclient/target/debug/rigclient"
python3 scripts/provider-update-rig/rig.py setup
python3 scripts/provider-update-rig/rig.py run \
  target/debug/zerun-engine after \
  npm_nvm pnpm_global bun_global hermes_channel grok_fails grok_prompt cursor_native
```

The rig keeps provider discovery inside the scenario directory. It disables
providers that are not under test and gives the tested provider a sandboxed
executable. `rig_test.py` tests the Python helpers without starting an engine:

```sh
python3 scripts/provider-update-rig/rig_test.py
```

For a read-only check of a system package installation, run the `system_git`
scenario. It should report a package-manager command and `canApply=false`.

## Windows

`windows-row.ps1` runs one isolated scenario in Windows PowerShell 5.1. Build
`zerun-engine` and `rigclient`, then provide a temporary directory and the
sandboxed launcher path:

```powershell
$rig = Join-Path $env:TEMP "zerun-provider-update-rig"
$row = "scripts\provider-update-rig\windows-row.ps1"
& $row -Label after -Row smoke -Target codex `
  -Launcher "$rig\npm-codex\codex.cmd" -ExtraPath "$rig\node"
```

Keep the launcher, engine data, and all package-manager caches below `$rig`.
The script stops only the engine and child processes belonging to that row.

## Scenarios

The scenario names cover npm installations, pnpm and bun global installs,
release-channel updates, package-manager ownership, interactive updater
prompts, failed installs, and a native installer. Each scenario prints the
check result, update phase, and the provider's final version.
