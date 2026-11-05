# Zerun Experimental

An optional desktop interface built with [zgx](https://zgx.zeron.sh) and Luau.
The existing `zerun` desktop remains available. This preview includes onboarding,
chat and agent selection, files, terminals, browser panels, plugins and templates.
It is still experimental; use the stable desktop for production service installs
and automatic updates.

## Build and run

Use the repository's Rust toolchain and Python 3. On macOS, install the Xcode
command line tools and Zig 0.16.0 for the terminal renderer. On Linux, install a
C/C++ compiler, pkg-config and the development packages for D-Bus, Wayland and
xkbcommon; a desktop session and graphics driver are needed to open the window.

From the repository root:

```sh
scripts/experimental-desktop.sh
scripts/experimental-desktop.sh --release
scripts/experimental-desktop.sh gallery
```

The helper builds both the desktop and its matching `zerun-engine`, without
building the stable desktop's UI dependencies. Do not pair
this interface with an older installed engine: plugins, templates and registry
agents require the interfaces in this checkout.

The UI is its own Cargo workspace. Its pinned renderer patches stay separate
from the stable desktop's GPUI dependencies:

```sh
cargo check --locked --manifest-path apps/zgx/Cargo.toml --all-targets
cargo test --locked --manifest-path apps/zgx/Cargo.toml
```

## Account and storage

Sign-in uses the fork's configured edge and WorkOS application. Normal setup
asks about appearance, organizations, invitations, GitHub and agent providers.
Public username registration and profile cards are omitted.

The edge must include the accompanying onboarding, private profile and plugin
signing-key routes. A self-hosted edge should be updated before using these
features; no additional Cloud resources or public profile site are required.

The experimental desktop uses `~/.zerun-experimental` and IPC port `27872`
(on Windows, `%LOCALAPPDATA%\Zerun Experimental`). Its login callback defaults
to loopback port `27873`. Stable desktops keep their existing profiles.
Agent CLI credentials remain those of the current user unless an isolated
agent home is explicitly selected. Signing in can sync conversations from the
same account and organization, even though local app state is separate.

Optional overrides:

| Variable | Purpose |
| --- | --- |
| `ZERUN_ZGX_ENGINE_BIN` | Matching engine executable |
| `ZERUN_ZGX_ENGINE_DATA` | Experimental engine data directory |
| `ZERUN_ZGX_ENGINE_PORT` | Experimental engine IPC port |
| `ZERUN_ZGX_ENGINE_SPAWN=0` | Connect without starting an engine |
| `ZERUN_ZGX_ENGINE_HOME` | Separate home for agent CLI credentials |
| `ZERUN_ZGX_EDGE` | Custom edge URL |
| `ZERUN_ZGX_WORKOS_CLIENT` | Custom WorkOS client ID |
| `ZERUN_ZGX_CALLBACK_PORT` | Loopback sign-in callback port |
| `ZERUN_ZGX_UI` | Alternate UI source directory |

Stable IPC ports and the default stable profile directories are refused.
Updates require rebuilding this preview; it never installs a stable desktop
update over itself. No experimental release feed is configured.

## Development

`ui/` holds the Luau interface and reloads when saved. `src/` owns the desktop
window, engine connection and native platform features. Use `gallery` to inspect
the design system, or `main`, `onboarding`, `fixture=<file>.luau` and `--inspect`
for specific entry points. Developer fixtures use sample data.

`cargo test` runs Rust host regressions and the Luau interface tests. CI reserves
one macOS runner for this workspace through the shared admission planner, builds
the matching engine, and runs both sets of checks. It does not publish a release.

See [plugins](docs/plugins.md), [templates](docs/templates.md) and
[localization](docs/i18n.md) for their development interfaces.

## Layout and themes

The default interface has resizable sidebars, a browser and terminal panel,
and keyboard navigation. Settings → Themes manages built-in and imported
themes independently of interface templates. Templates can be previewed before
keeping them; the host menu always offers a way back to the default interface.

On Linux, browser tabs need WebKitGTK 4.1 at runtime. Without it, the panel
shows a placeholder for the page.
