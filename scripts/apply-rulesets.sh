#!/usr/bin/env bash
# Apply repository rulesets from .github/rulesets/*.json
#
# Usage:
#   scripts/apply-rulesets.sh [owner/repo]
#
# If the argument is omitted, the current `gh repo view` (or `origin` remote)
# is used. Upserts by name: creates if missing, updates in place if present.
set -euo pipefail

repo="${1:-$(gh repo view --json nameWithOwner --jq .nameWithOwner 2>/dev/null || true)}"
if [[ -z "$repo" ]]; then
  echo "error: pass owner/repo or run inside a gh-linked repo" >&2
  exit 2
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
dir="$root/.github/rulesets"

if [[ ! -d "$dir" ]]; then
  echo "error: $dir not found" >&2
  exit 2
fi

shopt -s nullglob
files=("$dir"/*.json)
if [[ ${#files[@]} -eq 0 ]]; then
  echo "no ruleset JSON files found in $dir"
  exit 0
fi

# Fetch existing rulesets once to avoid repeated API calls.
existing="$(gh api "/repos/$repo/rulesets" --paginate)"

# Detect whether push rulesets are supported.
# GitHub restricts push rulesets to org-owned repos and private repos.
repo_visibility="$(gh api "/repos/$repo" --jq '.visibility')"
repo_owner_type="$(gh api "/repos/$repo" --jq '.owner.type')"

for f in "${files[@]}"; do
  name="$(python3 -c "import json,sys;print(json.load(open(sys.argv[1]))['name'])" "$f")"
  target="$(python3 -c "import json,sys;print(json.load(open(sys.argv[1]))['target'])" "$f")"

  if [[ "$target" == "push" ]]; then
    if [[ "$repo_owner_type" != "Organization" && "$repo_visibility" != "private" ]]; then
      echo "==> Skipping ruleset '$name' (push rulesets require org-owned or private repos)"
      continue
    fi
  fi

  id="$(printf '%s' "$existing" | python3 -c "
import json, sys
name = sys.argv[1]
data = json.load(sys.stdin)
for rs in data:
    if rs['name'] == name:
        print(rs['id'])
        break
" "$name")"

  if [[ -n "$id" ]]; then
    echo "==> Updating ruleset '$name' (id=$id)"
    gh api -X PUT "/repos/$repo/rulesets/$id" --input "$f" >/dev/null
  else
    echo "==> Creating ruleset '$name'"
    gh api -X POST "/repos/$repo/rulesets" --input "$f" >/dev/null
  fi
done

echo
echo "Done. Current rulesets on $repo:"
gh api "/repos/$repo/rulesets" --jq '.[] | "  [\(.id)] \(.name)  target=\(.target)  enforcement=\(.enforcement)"'
