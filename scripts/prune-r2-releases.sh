#!/usr/bin/env bash
# Keep the `zerun-releases` R2 bucket to what clients actually fetch.
#
# Nothing in the product downloads an older release: install.sh resolves the
# headless tarball from `latest.txt`, and the desktop updater
# (crates/update/src/lib.rs, windows.rs) and the Android updater
# (AndroidUpdater.kt) build their file names from `manifest.json`'s version. So
# every version but the newest is dead weight for clients, and only a handful of
# per-release artifacts are ever fetched:
#
#   zerun-<ver>-linux-x86_64.tar.gz      install.sh + Linux updater
#   zerun-<ver>-linux-aarch64.tar.gz     install.sh
#   zerun-<ver>-macos-arm64-app.tar.gz   macOS updater
#   zerun-<ver>-windows-x86_64.exe       Windows in-place updater
#   zerun-<ver>-android.apk              Android updater
#
# Everything else (dmg, portable zip, installer exe, standalone .sha256) is a
# manual download: README sends users to GitHub Releases, and checksums come
# from manifest.json, which stays on R2 as the live pointer. `manifest.json` and
# `latest.txt` are never deleted here.
#
# Asset names come from the GitHub Releases API, so this also covers the legacy
# `zeron-*` spelling of early releases. Dry-run by default; `--apply` deletes.
#
# Usage: scripts/prune-r2-releases.sh [--keep N] [--protect TAG]... [--all-assets]
#                                    [--bucket NAME] [--apply]
#   --keep N       newest releases kept whole (default 2)
#   --protect TAG  release tag to keep regardless of age (repeatable; the
#                  release workflow passes the tag it just published)
#   --all-assets   keep every asset of kept releases, not just the fetched five
#   --bucket NAME  R2 bucket (default zerun-releases)
#   --apply        delete instead of printing the plan
#
# Requires an authenticated `gh` and an authenticated wrangler (CI:
# CLOUDFLARE_API_TOKEN; locally: a wrangler login).
set -euo pipefail

keep=2
bucket=zerun-releases
apply=0
all_assets=0
protected=()

while (($#)); do
  case "$1" in
    --keep) keep="${2:?--keep needs a count}"; shift 2 ;;
    --protect) protected+=("${2:?--protect needs a tag}"); shift 2 ;;
    --bucket) bucket="${2:?--bucket needs a name}"; shift 2 ;;
    --all-assets) all_assets=1; shift ;;
    --apply) apply=1; shift ;;
    -h|--help) sed -n '2,32p' "$0"; exit 0 ;;
    *) echo "prune-r2-releases: unknown argument: $1" >&2; exit 2 ;;
  esac
done

[[ "$keep" =~ ^[0-9]+$ ]] || { echo "prune-r2-releases: --keep must be a number" >&2; exit 2; }
command -v gh >/dev/null || { echo "prune-r2-releases: gh not found" >&2; exit 1; }
command -v jq >/dev/null || { echo "prune-r2-releases: jq not found" >&2; exit 1; }

# No `gh auth status` preflight: it validates against /user, which the
# Actions-scoped GITHUB_TOKEN cannot call. A repo-scoped read is what this
# script actually needs.
repo="$(gh repo view --json nameWithOwner -q .nameWithOwner)" \
  || { echo "prune-r2-releases: gh is not authenticated for this repository (set GH_TOKEN)" >&2; exit 1; }
releases="$(
  gh api --paginate "repos/$repo/releases?per_page=100" \
    --jq '.[] | {tag: .tag_name, at: .created_at, assets: [.assets[].name]}' \
    | jq -s 'sort_by(.at) | reverse'
)"

keep_json="$(printf '%s' "$releases" | jq -c --argjson n "$keep" '.[0:$n] | map(.tag)')"
protected_json="$(printf '%s\n' "${protected[@]+"${protected[@]}"}" | jq -R . | jq -sc 'map(select(length > 0))')"
kept="$(jq -cn --argjson keep "$keep_json" --argjson extra "$protected_json" '$keep + $extra | unique')"

echo "keeping: $(printf '%s' "$kept" | jq -r 'join(", ")')"

plan="$(
  printf '%s' "$releases" | jq -r \
    --argjson kept "$kept" '
      .[] | .tag as $tag | select(($kept | index($tag)) == null)
      # every asset of a release outside the keep window
      | .assets[] | select(. != "manifest.json") | "\($tag)\t\(.)"
    '
  if (( ! all_assets )); then
    # Inside the keep window, drop the manual-download-only artifacts too.
    printf '%s' "$releases" | jq -r \
      --argjson kept "$kept" '
        .[] | .tag as $tag | select(($kept | index($tag)) != null)
        | .assets[]
        | select(. != "manifest.json")
        | select(test("-(linux-x86_64|linux-aarch64)\\.tar\\.gz$|-macos-arm64-app\\.tar\\.gz$|-windows-x86_64\\.exe$|-android\\.apk$") | not)
        | "\($tag)\t\(.)"
      '
  fi
)"

plan="$(printf '%s' "$plan" | grep -v '^$' || true)"
if [[ -z "$plan" ]]; then
  echo "nothing to prune"
  exit 0
fi

count=0
while IFS=$'\t' read -r tag name; do
  [[ -n "$name" ]] || continue
  key="$bucket/$name"
  if (( apply )); then
    echo "delete $key"
    npx wrangler@4 r2 object delete "$key" --remote || true
  else
    echo "would delete $key ($tag)"
  fi
  count=$((count + 1))
done <<<"$plan"

if (( apply )); then
  echo "deleted $count object(s) from $bucket"
else
  echo "$count object(s) would be pruned from $bucket — re-run with --apply"
fi
