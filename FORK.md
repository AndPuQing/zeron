# Fork notes

This is a self-hosted fork of [zeronsh/zeron](https://github.com/zeronsh/zeron).
It is not a mirror: it ships its own edge (Cloudflare), its own iOS TestFlight
builds, and its own desktop update feed.

## Branches

- **`dev`** — the fork trunk and the repository default branch. All fork work
  and releases live here; CI workflows are wired to `dev`.
- **`main`** — a read-only mirror of `upstream/main` (zeronsh/zeron). Never
  commit here. Refresh it by hand when needed:
  `git fetch upstream && git push origin upstream/main:main`.

Remotes:

- `origin` — this fork.
- `upstream` — zeronh/zeron (read-only).

## Syncing upstream

Select changes individually rather than merging the entire source branch.
Fetch it read-only, then apply the needed commits:

```sh
git fetch --no-tags upstream main
git cherry-pick <commit>...
```

Write commit messages around this repository's resulting behavior. Do not add
cross-repository issue or pull request references, source commit trailers, or
author mentions. Push changes only to `origin`.

Never mirror source tags into this fork: a tag marks the tree actually released.

## Deliberate divergence

Keep this list small and current; it is the checklist for resolving cherry-pick
conflicts.

| Area | Change |
| --- | --- |
| CI branch wiring | `main` → `dev` in workflow triggers and cache conditions |
| `deploy.yml` | landing/www jobs removed; edge deploys on `dev` |
| Edge | `zerun-edge` worker, `zerun-blobs`/`zerun-releases` R2 buckets, host `edge.550w.host`, account `808eccb28c4b8ff2386cb40c20704b22` |
| Endpoints | `apps/zeron`, `crates/client` and iOS sign-in point at `https://edge.550w.host` |
| URL scheme | `zerun-dev` (OAuth callback, `zerun-dev://open/chat/...` deep links and Live Activity return links) |
| Branding | display name `Zerun`; desktop executable `zerun`, macOS bundle `Zerun.app`, bundles `work.puqing.zerun[.ios]`; internal Rust package names remain `zeron-*` |
| Desktop isolation | Unix data `~/.zerun`, Windows data `%LOCALAPPDATA%\Zerun`, Linux service `zerun.service`, Windows installer AppId `94F099C7-A9D5-5A32-B951-50AE16E29A01`, IPC 27655, loopback callback 27642; overrides use `ZERUN_*` |
| Android | app ID `work.puqing.zerun.android`, display name `Zerun`, callback `zerun-dev`, workspace version; local China mirrors, official sources in CI; signed APKs join the release/download feed |
| Live Activity | extension bundle `work.puqing.zerun.ios.LiveActivity`; returns to the app via `zerun-dev://voice` |
| Mobile bindings | regenerate committed UniFFI Swift bindings when forked Rust exports or their documentation change |
| Update feed | `{edge}/releases` (R2); advisory links point at `github.com/AndPuQing/zeron` |
| Versioning | the fork releases independently, starting at `v0.3.0`; fork tags are never shared with upstream |

## Fork configuration (not in git)

| Item | Where | Status |
| --- | --- | --- |
| WorkOS client id | `edge/wrangler.jsonc`, `apps/zeron/src/main.rs`, `crates/client/src/auth.rs`, iOS `SignInViewController.swift` | configured (staging environment `client_01M4AN0G973H42GQJY8RAAC0RS`) |
| WorkOS API key | `wrangler secret put WORKOS_API_KEY` in `edge/` | configured on `zerun-edge`; deployed auth routes recognize the secret |
| WorkOS redirects | WorkOS Dashboard → Applications → Redirects | mobile, CLI and desktop loopback authorization requests accepted (including an alternate port) |
| APNs key | `wrangler secret put APNS_KEY_P8` / `APNS_KEY_ID`, vars `APNS_TEAM_ID` / `APNS_TOPIC` | pending (topic = iOS bundle id) |
| Apple Team ID | GitHub repo variable `APPLE_TEAM_ID` + `DEVELOPMENT_TEAM` for the app and Live Activity targets in Xcode | pending |
| Android signing | repo secrets `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD` | dedicated key; required by the signed Android build |
| `CLOUDFLARE_API_TOKEN` | GitHub repo secret in `AndPuQing/zeron` | configured; Worker deployment verified in CI |
| `MACOS_CERT_P12` / `MACOS_CERT_PASSWORD` | GitHub repo secret (Developer ID) | pending |
| `AC_API_KEY_P8` / `AC_API_KEY_ID` / `AC_API_ISSUER_ID` | GitHub repo secrets | pending |

The iOS app embeds `work.puqing.zerun.ios.LiveActivity`. Apple signing setup
must use the same Team for the app and extension and provision both bundle
identifiers.

Allowed redirect URIs for the fork's WorkOS staging application:

- `zerun-dev://callback` — mobile/native sign-in.
- `https://edge.550w.host/auth/cli/callback` — the paste-code flow used by
  `zerun login`.
- `http://127.0.0.1:*/callback` — desktop loopback sign-in. The default port
  is `27642`; `ZERUN_CALLBACK_PORT` can override it. WorkOS supports port
  wildcards for loopback addresses.
