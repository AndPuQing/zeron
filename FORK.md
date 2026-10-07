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
| (add entries as setup lands) | edge URL, WorkOS client id, bundle IDs, team ID, update feed |
