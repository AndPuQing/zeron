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

Merge upstream into the trunk by hand:

```sh
git checkout dev
git fetch upstream
git merge upstream/main
```

To land an upstream PR before upstream merges it:

```sh
git fetch upstream pull/<number>/head:pr-<number>
git merge pr-<number>
```

When upstream later merges the same PR, the merge into `dev` is usually clean
because the patches are identical.

## Deliberate divergence

Keep this list small and current; it is the checklist for resolving merge
conflicts.

| Area | Change |
| --- | --- |
| CI branch wiring | `main` → `dev` in workflow triggers and cache conditions |
| `deploy.yml` | landing/www jobs removed; edge deploys on `dev` |
| Edge | `zerun-edge` worker, `zerun-blobs`/`zerun-releases` R2 buckets, host `zerun.puqing.work`, account `fc5a16c75e508b812ee6edd119fd32ae` |
| Endpoints | `apps/zeron`, `crates/client` and iOS sign-in point at `https://zerun.puqing.work` |
| URL scheme | `zerun-dev` (OAuth callback and `zerun-dev://open/chat/...` deep links) |
| Branding | display name `Zerun`; bundles `work.puqing.zerun[.ios]`; binary/product names stay `zeron` for now |
| Update feed | `{edge}/releases` (R2); advisory links point at `github.com/AndPuQing/zeron` |

## Fork configuration (not in git)

| Item | Where | Status |
| --- | --- | --- |
| WorkOS client id | `edge/wrangler.jsonc`, `apps/zeron/src/main.rs`, `crates/client/src/auth.rs`, iOS `SignInViewController.swift` | configured (staging environment `client_01M4AN0G973H42GQJY8RAAC0RS`) |
| WorkOS API key | `wrangler secret put WORKOS_API_KEY` in `edge/` | configured on `zerun-edge`; deployed auth routes recognize the secret |
| WorkOS redirects | WorkOS Dashboard → Applications → Redirects | mobile, CLI and desktop loopback authorization requests accepted (including an alternate port) |
| APNs key | `wrangler secret put APNS_KEY_P8` / `APNS_KEY_ID`, vars `APNS_TEAM_ID` / `APNS_TOPIC` | pending (topic = iOS bundle id) |
| Apple Team ID | GitHub repo variable `APPLE_TEAM_ID` + `DEVELOPMENT_TEAM` in the Xcode project | pending |
| `CLOUDFLARE_API_TOKEN` | GitHub repo secret in `AndPuQing/zeron` | configured; actual deployment permissions await CI verification |
| `MACOS_CERT_P12` / `MACOS_CERT_PASSWORD` | GitHub repo secret (Developer ID) | pending |
| `AC_API_KEY_P8` / `AC_API_KEY_ID` / `AC_API_ISSUER_ID` | GitHub repo secrets | pending |

Allowed redirect URIs for the fork's WorkOS staging application:

- `zerun-dev://callback` — mobile/native sign-in.
- `https://zerun.puqing.work/auth/cli/callback` — the paste-code flow used by
  `zeron login`.
- `http://127.0.0.1:*/callback` — desktop loopback sign-in. The default port
  is `27641`; `ZERON_CALLBACK_PORT` can override it. WorkOS supports port
  wildcards for loopback addresses.
