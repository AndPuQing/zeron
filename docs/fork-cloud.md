# Cloud source in this fork

This fork carries the upstream Cloud control plane *in source only*.
`edge/wrangler.jsonc` binds no Cloud Durable Objects, Workflows, credential
vault service or billing cron. Its WorkOS user directory is separate from Cloud.

## WorkOS user directory

The edge binds `USER_DIRECTORY` to `UserDirectory` with migration `v6`; upstream
`v5` adds Cloud classes and is deliberately absent here. This directory records
WorkOS profiles, join dates, organizations and memberships independently of
sandbox hosting or billing. Each recorded user receives a stable user number;
updates and deletion do not change or reuse it.

To receive user events, configure `https://edge.550w.host/workos/webhook` in this
fork's WorkOS environment for the created, updated and deleted events of
`user`, `organization` and `organization_membership`, then set the endpoint's
signing secret as `WORKOS_WEBHOOK_SECRET` on
`zerun-edge`. Without that secret, webhook delivery answers 503 for retries.
`GET /admin/users` and `GET /admin/organizations` (JSON or `?format=csv`)
require the operator's `ADMIN_TOKEN`;
without it, the endpoint answers 404. A deploy does not create the WorkOS webhook
subscription or backfill existing users.

Workspace creation at `POST /auth/orgs` is limited to one organization per
account. Concurrent requests share a creation claim, and retries reuse an
existing organization. An existing admin membership also counts; being a
regular member of another organization does not prevent creating one. A
request made while creation is still in progress asks the user to retry later.
This protection uses the existing directory binding and does not require the
optional webhook subscription or admin token. Admin exports include the
recorded organization creator when available.

The same optional admin token protects badge management: `GET`/`POST
`/admin/badges` lists or defines badges; `POST /admin/badges/grant` and
`POST /admin/badges/revoke` assign or remove them. User exports include held
badges. These routes also return 404 without `ADMIN_TOKEN`; syncing or deploying
this source does not configure a token or grant any badges.

## What that means at runtime

- `GET /cloud` answers HTTP 200 with `available: false`, so nothing on the
  client needs to interpret an error.
- The gate is `cloudAvailable` in `edge/src/cloud/routes.ts`: it needs
  `CLOUD_ACCOUNTS`, the four lifecycle Workflow bindings, a sandbox provider
  and a runner key; none are bound here.
- `cloudOffered` additionally requires `CLOUD_ACCESS`, and returns false under
  `AUTH_MODE=workos` when it is unset (this deployment runs `workos`).
- The client keeps the ordinary local experience; a 404/501 from another
  route still reads as "not offered", while a 503 on the status read itself is
  a retryable error.

Cloud is not free to switch on: a deployment that wants it has to own the
infrastructure below, plus billing.

## What a deployment would need

Configuration (`edge/wrangler.jsonc`):

- Durable Objects `CLOUD_ACCOUNTS` and `CLOUD_INDEX`, with a migration that
  adds both classes.
- Workflow bindings `CLOUD_PROVISION`, `CLOUD_WAKE`, `CLOUD_SLEEP` and
  `CLOUD_DELETE` (`src/cloud/workflows.ts`). Workflow names are account-wide, so
  a second deployment must not reuse them.
- The `VAULT` service binding, pointing at a deployed vault worker.
- The hourly reconcile cron (`"17 * * * *"`).

Secrets, by purpose (the availability gate above reads only the bindings, the
sandbox provider and the runner key):

| Secret | Used for |
| --- | --- |
| `BOAT_API_KEY` | sandbox provider (`BOAT_API_BASE`) |
| `RUNNER_JWT_PRIVATE_KEY` / `RUNNER_JWT_PUBLIC_KEY` | signing runner JWTs |
| `CLOUD_ACCESS` | who sees Cloud: `everyone`, or user/org ids |
| `ADMIN_TOKEN` | `GET /admin/cloud/usage` (404 without it) |
| `STRIPE_SECRET_KEY` / `STRIPE_WEBHOOK_SECRET` | billing (billing reads unavailable until set) |

Vars: `STRIPE_PRICE_BASE` / `_PRO` / `_MAX` / `_CREDITS`,
`STRIPE_PORTAL_CONFIGURATION`, `CLOUD_MACHINE`, `CLOUD_IDLE_MINUTES`,
`CLOUD_MAX_AWAKE`, `CLOUD_TTL_SECONDS`, `CLOUD_STARTING_CREDITS`,
`CLOUD_EDGE_URL`, `CLOUD_TEMPLATE`.

The credential vault (`edge/vault`) is a worker of its own with no HTTP
surface; `edge/vault/wrangler.jsonc` carries the fork's name and deliberately
no account id. It is not deployed by this repository's workflow — running it
needs its own Cloudflare account, its KEK material and a separate deploy step.

## Keeping the fork's configuration

- `edge/wrangler.jsonc` stays on this fork's worker (`zerun-edge`), routes
  (`edge.550w.host`), buckets and WorkOS client; no upstream account id, no
  `env.staging` block, no Stripe ids.
- `.github/workflows/deploy.yml` deploys the edge only; it has no vault job and
  no approval environment.
- The upstream staging helper (`scripts/staging-app.sh`) and the upstream
  `CODEOWNERS` review rules are not part of this fork.
