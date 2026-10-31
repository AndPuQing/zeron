#!/usr/bin/env bash
# Keep the `zerun-releases` R2 bucket to what clients actually fetch.
#
# Nothing in the product downloads an older release: install.sh resolves the
# headless tarball from `latest.txt`, and the desktop updater
# (crates/update/src/lib.rs, windows.rs) and the Android updater
# (AndroidUpdater.kt) build their file names from `manifest.json`'s version. So
# every version but the newest is dead weight for clients, and the release
# workflow uploads only the handful of per-release artifacts that are fetched:
#
#   zerun-<ver>-linux-x86_64.tar.gz      install.sh + Linux updater
#   zerun-<ver>-linux-aarch64.tar.gz     install.sh
#   zerun-<ver>-macos-arm64-app.tar.gz   macOS updater
#   zerun-<ver>-windows-x86_64.exe       Windows in-place updater
#   zerun-<ver>-android.apk              Android updater
#
# Everything else (dmg, portable zip, installer exe, standalone .sha256) is a
# manual download from GitHub Releases and never reaches R2. `manifest.json`
# and `latest.txt` are the live pointers and are never deleted here.
#
# Every publish prunes, so only the release that just left the keep window
# still has objects on R2. A normal run therefore deletes the fetched artifacts
# of the few releases right behind the window (`--lookback`, which also covers
# a publish whose prune did not run) instead of re-deleting all of history:
# deleting an absent R2 object succeeds, so a full sweep on every release only
# costs minutes and grows with each one. `--full` is that sweep, for a one-off
# cleanup: every asset of every older release, plus the manual-download assets
# of kept ones, as uploaded before the bucket was capped.
#
# Asset names come from the GitHub Releases API, so this also covers the legacy
# `zeron-*` spelling of early releases. Drafts never reach R2 and hold no place
# in the window. Dry-run by default; `--apply` deletes.
#
# Usage: scripts/prune-r2-releases.sh [--keep N] [--lookback N] [--full]
#                                    [--protect TAG]... [--repo OWNER/NAME]
#                                    [--bucket NAME] [--apply]
#   --keep N          newest releases kept on R2 (default 2)
#   --lookback N      releases behind the keep window to sweep (default 3)
#   --full            sweep every older release and every non-fetched asset
#   --protect TAG     release tag to keep regardless of age (repeatable; the
#                     release workflow passes the tag it just published)
#   --repo OWNER/NAME releases to read (default $GITHUB_REPOSITORY, else the
#                     `origin` remote: gh alone would prefer `upstream`)
#   --bucket NAME     R2 bucket (default zerun-releases)
#   --apply           delete instead of printing the plan
#
# With --apply, exits 1 when any delete fails (after trying the rest).
#
# Requires an authenticated `gh` and an authenticated wrangler (CI:
# CLOUDFLARE_API_TOKEN; locally: a wrangler login).
set -euo pipefail

keep=2
lookback=3
full=0
bucket=zerun-releases
repo="${GITHUB_REPOSITORY:-}"
apply=0
protected=()

while (($#)); do
  case "$1" in
    --keep) keep="${2:?--keep needs a count}"; shift 2 ;;
    --lookback) lookback="${2:?--lookback needs a count}"; shift 2 ;;
    --full) full=1; shift ;;
    --protect) protected+=("${2:?--protect needs a tag}"); shift 2 ;;
    --repo) repo="${2:?--repo needs OWNER/NAME}"; shift 2 ;;
    --bucket) bucket="${2:?--bucket needs a name}"; shift 2 ;;
    --apply) apply=1; shift ;;
    -h|--help) sed -n '2,50p' "$0"; exit 0 ;;
    *) echo "prune-r2-releases: unknown argument: $1" >&2; exit 2 ;;
  esac
done

[[ "$keep" =~ ^[0-9]+$ ]] || { echo "prune-r2-releases: --keep must be a number" >&2; exit 2; }
[[ "$lookback" =~ ^[0-9]+$ ]] || { echo "prune-r2-releases: --lookback must be a number" >&2; exit 2; }
command -v gh >/dev/null || { echo "prune-r2-releases: gh not found" >&2; exit 1; }
command -v jq >/dev/null || { echo "prune-r2-releases: jq not found" >&2; exit 1; }

# No `gh auth status` preflight: it validates against /user, which the
# Actions-scoped GITHUB_TOKEN cannot call. A repo-scoped read is what this
# script actually needs.
if [[ -z "$repo" ]]; then
  origin="$(git remote get-url origin 2>/dev/null)" \
    || { echo "prune-r2-releases: no origin remote; pass --repo OWNER/NAME" >&2; exit 1; }
  repo="$(gh repo view "$origin" --json nameWithOwner -q .nameWithOwner)" \
    || { echo "prune-r2-releases: gh cannot read $origin (set GH_TOKEN or pass --repo)" >&2; exit 1; }
fi
releases="$(
  gh api --paginate "repos/$repo/releases?per_page=100" \
    --jq '.[] | select(.draft | not) | {tag: .tag_name, at: .created_at, assets: [.assets[].name]}' \
    | jq -s 'sort_by(.at) | reverse'
)" || { echo "prune-r2-releases: cannot list releases of $repo (set GH_TOKEN)" >&2; exit 1; }

keep_json="$(printf '%s' "$releases" | jq -c --argjson n "$keep" '.[0:$n] | map(.tag)')"
protected_json="$(printf '%s\n' "${protected[@]+"${protected[@]}"}" | jq -R . | jq -sc 'map(select(length > 0))')"
kept="$(jq -cn --argjson keep "$keep_json" --argjson extra "$protected_json" '$keep + $extra | unique')"

echo "repository: $repo"
echo "keeping: $(printf '%s' "$kept" | jq -r 'join(", ")')"

# The artifacts the release workflow uploads to R2.
fetched='-(linux-x86_64|linux-aarch64)\.tar\.gz$|-macos-arm64-app\.tar\.gz$|-windows-x86_64\.exe$|-android\.apk$'

if (( full )); then
  echo "sweeping: every older release"
  plan="$(
    printf '%s' "$releases" | jq -r \
      --argjson kept "$kept" '
        .[] | .tag as $tag | select(($kept | index($tag)) == null)
        | .assets[] | select(. != "manifest.json") | "\($tag)\t\(.)"
      '
    printf '%s' "$releases" | jq -r \
      --argjson kept "$kept" --arg fetched "$fetched" '
        .[] | .tag as $tag | select(($kept | index($tag)) != null)
        | .assets[]
        | select(. != "manifest.json")
        | select(test($fetched) | not)
        | "\($tag)\t\(.)"
      '
  )"
else
  # The releases right behind the window: older than the newest kept one and
  # not kept themselves, newest first.
  swept="$(
    printf '%s' "$releases" | jq -c \
      --argjson kept "$kept" --argjson n "$lookback" '
        map(select(.tag as $tag | ($kept | index($tag)) == null))
        | .[0:$n]
      '
  )"
  echo "sweeping: $(printf '%s' "$swept" | jq -r 'map(.tag) | if length == 0 then "none" else join(", ") end')"
  plan="$(
    printf '%s' "$swept" | jq -r --arg fetched "$fetched" '
      .[] | .tag as $tag | .assets[] | select(test($fetched)) | "\($tag)\t\(.)"
    '
  )"
fi

plan="$(printf '%s' "$plan" | grep -v '^$' || true)"
if [[ -z "$plan" ]]; then
  echo "nothing to prune"
  exit 0
fi

count=0
failed=0
while IFS=$'\t' read -r tag name; do
  [[ -n "$name" ]] || continue
  key="$bucket/$name"
  if (( apply )); then
    echo "delete $key"
    if ! npx wrangler@4 r2 object delete "$key" --remote; then
      failed=$((failed + 1))
      if [[ -n "${GITHUB_ACTIONS:-}" ]]; then
        echo "::warning title=R2 prune::could not delete $key ($tag)"
      else
        echo "prune-r2-releases: could not delete $key ($tag)" >&2
      fi
    fi
  else
    echo "would delete $key ($tag)"
  fi
  count=$((count + 1))
done <<<"$plan"

if (( apply )); then
  # R2 reports success for an absent key too, so these are requests sent, not
  # objects known to have existed.
  echo "sent $((count - failed)) delete request(s) to $bucket; $failed failed"
  (( failed == 0 )) || exit 1
else
  echo "$count object(s) would be pruned from $bucket — re-run with --apply"
fi
