# PR #42: native background-process UI captures

Product source: `f78f1aa5f2fa28e053d4a9eb40209b0b0cfbcb05` in AndPuQing/zeron.
Captured on 2026-10-30 with the production GPUI Shell, Composer and background
tray, in an isolated Linux/X11 window. The app used the opaque material option.

These are native application screenshots. The interaction sheet crops and
stacks their pixels with explanatory labels; it does not redraw controls.

## Scope

The capture driver registers only `mock::MockHarness` and uses the repository's
`ZERON_MOCK_BACKGROUND=1` scenario. Task lists, output reads and Stop requests
use the real local engine/RPC path. The transcript text is fixture copy.
Commands/log contents are Mock data: no dev server, test watcher, API process,
provider login or model request is started. This is desktop visual/interaction
evidence, and does not establish native Claude/Codex or mobile transport results.
The full-window model picker says "No agents available" because production
pickers hide the fixture-only Mock provider.

## Files

- `interaction-states.png`: collapsed, output open, stopping and confirmed removal.
- `output-dark.png`, `output-light.png`: complete native windows with the first command's output open.
- `collapsed-dark.png`, `expanded-dark.png`, `stopping-dark.png`, `stopped-dark.png`: original captures for the interaction sheet.
- `background-preview.rs`: the temporary capture driver, copied outside the crate after use.

## Reproduce

Use an isolated checkout of the product revision above and the normal Linux
native build dependencies (WebKitGTK 4.1, JSON-GLib, X11, a supported graphics
backend, and the project's voice/runtime dependencies). Copy the driver to
`crates/ui/examples/background-preview.rs` temporarily, then build:

```sh
CARGO_PROFILE_DEV_DEBUG=0 cargo build --locked -p zeron-ui   --example background-preview --features appshots-fixture   --config 'profile.dev.package."*".opt-level=0'   --config 'profile.dev.package.zeron-visual.opt-level=0'   --config 'profile.dev.package.zeron-visual-gpu.opt-level=0'
ZERON_MOCK_BACKGROUND=1 target/debug/examples/background-preview /path/to/captures
```

The driver creates a temporary data directory and a random local IPC port.
It writes its PID/task inventory to `ready.json`. Click the Background header,
then the first command row to open output. Stop is the square button on that
row; the Mock driver confirms after 250 ms. Use `xwd -id WINDOW -out FILE` to
capture the native window and convert the true-color XWD to PNG.

Write `light` or `dark` to the capture directory's `mode` file to switch themes;
create its `quit` file to exit and shut down the temporary engine. Remove the
temporary example after use. No production UI source changes are required.
