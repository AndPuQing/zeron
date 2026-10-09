# Development workflow

Zerun develops through task branches and pull requests targeting `dev`.
Read [FORK.md](FORK.md) before changing deployment, installation identity,
mobile bindings, or release behavior.

## Branches

- `dev` is the integration and release branch. Do not develop or commit directly
  on it, and do not push task changes directly to it.
- `main` mirrors upstream and is read-only for fork development.
- Create a fresh branch for each independent task, using `feat/`, `fix/`,
  `docs/`, or `chore/` followed by a short descriptive name.
- Fetch `origin/dev` before choosing the task baseline. Record its commit in
  the task plan. Preserve existing local work; use a separate worktree when
  the current checkout belongs to another task.
- Push task branches only to `origin`. Treat `upstream` as read-only.

For a clean checkout:

```sh
git fetch --no-tags origin dev
git switch --no-track -c feat/<task-name> origin/dev
```

Rebase an unpublished task branch when updating its baseline. Once a branch
is shared, preserve its history; merge `origin/dev` into it if needed. Do not
force-push, move release tags, or rewrite shared commits as routine cleanup.

## Design and commits

For a feature spanning multiple layers, first write a design under
`docs/design/`. Include behavior, ownership, storage, interfaces, compatibility,
failure handling, and acceptance criteria. Mark proposals as proposed rather
than implying they are implemented.

Keep design documents focused on behavior, architecture, and maintenance.
Update them to describe the implemented behavior as the feature changes.
Record task progress, commit references, validation results, and pending
checks in the PR description or delivery reply rather than committing work
logs or acceptance reports.

Commit complete, reviewable units of work. Include a change's meaningful tests
with its implementation. Git tags identify releases and follow the existing
release process.

Use descriptive commits, normally `feat:`, `fix:`, `docs:`, `refactor:`,
`test:`, `build:`, or `chore:`. Stage explicit paths and inspect the staged
diff. Keep generated assets, credentials, private transcripts, and unrelated
changes out of the commit unless the task specifically requires those assets.

## Validation

Run checks appropriate to the affected behavior and the repository's CI.
Account for platform-specific launches and remote device routing when touched.
Use isolated profiles and scripted agents for deterministic integration tests;
record real-provider checks separately from fixture results.

- Rust core: select the relevant packages and integration tests; the common
  CI entry is `scripts/ci/test-core.sh`.
- Desktop: affected `zeron-ui` tests and native fixtures where relevant.
- Edge: `npm ci`, `npm run typecheck`, and `npm test` in `edge/`.
- Mobile: shared Rust tests and affected platform tests; regenerate committed
  Swift UniFFI bindings when required by [FORK.md](FORK.md).

For a documentation-only change, inspect the diff, validate local links,
and run `git diff --check`. Do not report a build, test, deployment, or live
acceptance result that was not performed. Record missing tooling or failed
checks explicitly, and resolve relevant failures before marking an
implementation ready for review.

## Pull requests

Open a draft PR for work that is intentionally still in design or implementation.
Use `dev` as the base and the task branch as the head. Keep the description
current as scope changes; describe the final behavior, implementation decisions
needed for review, validation, and outstanding work.

When the requested implementation and relevant checks are complete, mark the
PR ready for review. Opening a PR does not authorize merging it, deploying it,
or publishing a release. Use separate authorization for those actions.
